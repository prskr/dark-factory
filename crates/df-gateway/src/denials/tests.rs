use std::net::IpAddr;
use std::sync::Mutex as StdMutex;

use super::*;
use crate::policy::Sandbox;

/// Records every report; fails the first `failures` calls (refused if `refuse` is set) and never
/// answers if `hang` is set.
#[derive(Default)]
struct FakeSink {
    calls: StdMutex<Vec<pb::Denial>>,
    failures: StdMutex<usize>,
    refuse: bool,
    hang: bool,
}

impl FakeSink {
    fn failing(failures: usize) -> Self {
        Self {
            failures: StdMutex::new(failures),
            ..Self::default()
        }
    }

    fn calls(&self) -> Vec<pb::Denial> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl DenialSink for FakeSink {
    async fn report(&self, denial: pb::Denial) -> Result<(), CallError> {
        self.calls.lock().unwrap().push(denial);
        if self.hang {
            std::future::pending::<()>().await;
        }
        let mut failures = self.failures.lock().unwrap();
        if *failures > 0 {
            *failures -= 1;
            if self.refuse {
                return Err(CallError::Permanent("invalid argument".into()));
            }
            return Err(CallError::Transient("unavailable".into()));
        }
        Ok(())
    }
}

fn queued(queue: &mut mpsc::Receiver<pb::Denial>) -> Vec<(String, String, u32)> {
    let mut reports = Vec::new();
    while let Ok(denial) = queue.try_recv() {
        reports.push((denial.run_id, denial.host, denial.port));
    }
    reports
}

fn policy_with_runs(runs: &[&str]) -> Policy {
    Policy::new(
        1,
        runs.iter().enumerate().map(|(i, run)| {
            (
                IpAddr::from([10, 0, 0, u8::try_from(i).unwrap()]),
                Sandbox {
                    run_id: (*run).into(),
                    task_key: "DF-7".into(),
                    run_number: 1,
                    project_key: "DF".into(),
                    persona_id: "p".into(),
                    hosts: Vec::new(),
                },
            )
        }),
    )
}

#[tokio::test(start_paused = true)]
async fn fifty_denials_within_a_minute_are_reported_once() {
    let (reports, mut queue) = DenialReports::new(QUEUE_CAPACITY);
    for _ in 0..50 {
        reports.denied("run-7", "statsig.anthropic.com", 443);
        tokio::time::advance(Duration::from_secs(1)).await;
    }
    assert_eq!(
        queued(&mut queue),
        [("run-7".into(), "statsig.anthropic.com".into(), 443)]
    );

    tokio::time::advance(Duration::from_secs(10)).await;
    reports.denied("run-7", "statsig.anthropic.com", 443);
    assert_eq!(
        queued(&mut queue).len(),
        1,
        "a minute later it is reported again"
    );
}

#[tokio::test(start_paused = true)]
async fn each_run_host_and_port_is_reported_on_its_own() {
    let (reports, mut queue) = DenialReports::new(QUEUE_CAPACITY);
    reports.denied("run-7", "pypi.org", 443);
    reports.denied("run-7", "pypi.org", 8443);
    reports.denied("run-8", "pypi.org", 443);
    reports.denied("run-7", "files.pythonhosted.org", 443);
    assert_eq!(queued(&mut queue).len(), 4);
}

#[tokio::test(start_paused = true)]
async fn the_host_is_reported_normalized() {
    let (reports, mut queue) = DenialReports::new(QUEUE_CAPACITY);
    reports.denied("run-7", "PyPI.org.", 443);
    reports.denied("run-7", "pypi.org", 443);
    assert_eq!(
        queued(&mut queue),
        [("run-7".into(), "pypi.org".into(), 443)]
    );
}

#[tokio::test(start_paused = true)]
async fn hosts_no_allowlist_entry_can_name_are_not_reported() {
    let (reports, mut queue) = DenialReports::new(QUEUE_CAPACITY);
    for host in ["*.evil.com", "*", "exa_mple.com", "a..com", " pypi.org", ""] {
        reports.denied("run-7", host, 443);
    }
    assert!(queued(&mut queue).is_empty());
    assert_eq!(reports.remembered(), 0);
}

#[tokio::test(start_paused = true)]
async fn reports_older_than_the_interval_are_forgotten() {
    let (reports, mut queue) = DenialReports::new(QUEUE_CAPACITY);
    reports.denied("run-7", "a.example.com", 443);
    tokio::time::advance(REPORT_INTERVAL).await;
    reports.denied("run-7", "b.example.com", 443);
    assert_eq!(queued(&mut queue).len(), 2);
    assert_eq!(reports.remembered(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_report_dropped_on_a_full_queue_is_not_remembered() {
    let (reports, mut queue) = DenialReports::new(1);
    reports.denied("run-7", "a.example.com", 443);
    reports.denied("run-7", "b.example.com", 443);
    assert_eq!(queued(&mut queue).len(), 1);
    reports.denied("run-7", "b.example.com", 443);
    assert_eq!(
        queued(&mut queue),
        [("run-7".into(), "b.example.com".into(), 443)]
    );
}

#[tokio::test(start_paused = true)]
async fn runs_that_left_the_snapshot_are_forgotten() {
    let (reports, mut queue) = DenialReports::new(QUEUE_CAPACITY);
    reports.denied("run-7", "pypi.org", 443);
    reports.denied("run-8", "pypi.org", 443);
    queued(&mut queue);

    reports.retain_runs(Some(&policy_with_runs(&["run-8"])));
    assert_eq!(reports.remembered(), 1);
    reports.denied("run-8", "pypi.org", 443);
    assert!(queued(&mut queue).is_empty(), "run-8 is still remembered");

    reports.retain_runs(None);
    assert_eq!(reports.remembered(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_failed_report_is_retried_exactly_once() {
    for (failures, calls) in [(0, 1), (1, 2), (5, 2)] {
        let sink = Arc::new(FakeSink::failing(failures));
        let (queue_tx, queue) = mpsc::channel(4);
        queue_tx.send(denial()).await.unwrap();
        drop(queue_tx);
        send_reports(queue, sink.clone(), CancellationToken::new()).await;
        assert_eq!(sink.calls().len(), calls, "{failures} failures");
    }
}

#[tokio::test(start_paused = true)]
async fn a_refused_report_is_not_retried() {
    let sink = Arc::new(FakeSink {
        refuse: true,
        ..FakeSink::failing(5)
    });
    let (queue_tx, queue) = mpsc::channel(4);
    queue_tx.send(denial()).await.unwrap();
    drop(queue_tx);
    send_reports(queue, sink.clone(), CancellationToken::new()).await;
    assert_eq!(sink.calls().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_call_that_never_answers_times_out_and_is_retried_once() {
    let sink = Arc::new(FakeSink {
        hang: true,
        ..FakeSink::default()
    });
    let (queue_tx, queue) = mpsc::channel(4);
    queue_tx.send(denial()).await.unwrap();
    drop(queue_tx);
    let started = Instant::now();
    send_reports(queue, sink.clone(), CancellationToken::new()).await;
    assert_eq!(sink.calls().len(), 2);
    assert!(started.elapsed() >= CALL_TIMEOUT * 2);
}

#[tokio::test(start_paused = true)]
async fn the_reporter_ends_when_stopped() {
    let sink = Arc::new(FakeSink::default());
    let (_queue_tx, queue) = mpsc::channel(4);
    let stop = CancellationToken::new();
    stop.cancel();
    send_reports(queue, sink.clone(), stop).await;
    assert!(sink.calls().is_empty());
}

fn denial() -> pb::Denial {
    pb::Denial {
        run_id: "run-7".into(),
        host: "pypi.org".into(),
        port: 443,
        at: None,
    }
}
