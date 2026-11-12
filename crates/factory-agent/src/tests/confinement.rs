//! How `run_session` starts the harness with and without Landlock (design D4, D5).

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use df_proto::agent::v1::{AgentMessage, RunOutcome, agent_message};
use df_proto::transcript::v1::{NoticeLevel, event};

use super::RunScenario;
use crate::runner::Confinement;
use crate::sandbox::probe::{LandlockProbe, LandlockSupport};

const SCRIPT: &str = r#"
sessions:
  new:
    - - say: "Done"
      - end: end_turn
"#;

/// A kernel that offers `support`.
struct FixedProbe(LandlockSupport);

impl LandlockProbe for FixedProbe {
    fn support(&self) -> LandlockSupport {
        self.0
    }
}

fn enforced(support: LandlockSupport) -> Confinement {
    Confinement {
        mode: df_core::LandlockMode::Enforce,
        probe: Arc::new(FixedProbe(support)),
    }
}

/// Stands in for `factory-agent sandbox -- <command>`: records its arguments and runs the command.
fn recording_wrapper(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let wrapper = dir.join("wrapper.sh");
    let record = dir.join("wrapper-args");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nshift 2\nexec \"$@\"\n",
            record.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    (wrapper, record)
}

fn landlock_notices(sent: &[AgentMessage]) -> Vec<df_proto::transcript::v1::Notice> {
    sent.iter()
        .filter_map(|m| match &m.body {
            Some(agent_message::Body::TranscriptEvent(ev)) => match &ev.kind {
                Some(event::Kind::Notice(n)) if n.code == "sandbox.landlock_unavailable" => {
                    Some(n.clone())
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn an_older_abi_fails_the_run_with_a_notice_and_starts_no_harness() {
    let Some(scenario) = RunScenario::new("run-abi5", SCRIPT) else {
        return;
    };
    let aux = tempfile::tempdir().unwrap();
    let (wrapper, record) = recording_wrapper(aux.path());
    let scenario = scenario
        .wrapper(wrapper)
        .confinement(enforced(LandlockSupport::Abi(5)));
    let (result, sent) = scenario.empty_prompt().run(|_, _| {}).await;

    assert_eq!(result.outcome, RunOutcome::Failed);
    assert!(result.run_report.detail.contains("ABI 5 found, 6 required"));
    let notices = landlock_notices(&sent);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].level, NoticeLevel::Error as i32);
    assert!(notices[0].message.contains("sandbox.landlock: disabled"));
    assert!(!record.exists(), "the harness must not start");
    // Checked before the workspace is prepared: no clone, no push of earlier commits.
    assert!(
        !sent.iter().any(|m| matches!(
            &m.body,
            Some(agent_message::Body::RuntimeTiming(t))
                if t.operation() == df_proto::agent::v1::RuntimeOperation::WorkspacePrepare
        )),
        "the workspace must not be prepared"
    );
}

#[tokio::test]
async fn a_kernel_without_landlock_fails_the_run_with_a_notice() {
    let Some(scenario) = RunScenario::new("run-no-landlock", SCRIPT) else {
        return;
    };
    let scenario = scenario.confinement(enforced(LandlockSupport::Unavailable));
    let (result, sent) = scenario.empty_prompt().run(|_, _| {}).await;

    assert_eq!(result.outcome, RunOutcome::Failed);
    let notices = landlock_notices(&sent);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].message.contains("not available"));
}

#[tokio::test]
async fn abi_six_starts_the_harness_through_the_sandbox_wrapper() {
    let Some(scenario) = RunScenario::new("run-abi6", SCRIPT) else {
        return;
    };
    let aux = tempfile::tempdir().unwrap();
    let (wrapper, record) = recording_wrapper(aux.path());
    let fake_bin = scenario.fake_bin();
    let scenario = scenario
        .wrapper(wrapper)
        .confinement(enforced(LandlockSupport::Abi(6)));
    let (result, sent) = scenario.release_up_front().run(|_, _| {}).await;

    assert_eq!(result.outcome, RunOutcome::EndTurn, "{result:?}");
    assert!(landlock_notices(&sent).is_empty());
    let args = std::fs::read_to_string(record).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert_eq!(&args[..3], ["sandbox", "--", fake_bin.to_str().unwrap()]);
    assert_eq!(args[3], "--script");
}

#[tokio::test]
async fn disabled_starts_the_harness_directly_and_warns() {
    let (logs, _guard) = df_telemetry::testing::capture("warn");
    let Some(scenario) = RunScenario::new("run-landlock-off", SCRIPT) else {
        return;
    };
    let aux = tempfile::tempdir().unwrap();
    let (wrapper, record) = recording_wrapper(aux.path());
    let scenario = scenario
        .wrapper(wrapper)
        .confinement(Confinement::new(df_core::LandlockMode::Disabled));
    let (result, _) = scenario.release_up_front().run(|_, _| {}).await;

    assert_eq!(result.outcome, RunOutcome::EndTurn, "{result:?}");
    assert!(!record.exists(), "the wrapper must not be used");
    let text = logs.text();
    assert!(
        text.contains("Landlock disabled by instance configuration"),
        "{text}"
    );
    assert!(text.contains("run-landlock-off"), "{text}");
}

#[tokio::test]
async fn a_wrapper_that_exits_125_before_the_harness_speaks_ends_the_run_failed() {
    let Some(scenario) = RunScenario::new("run-wrapper-125", SCRIPT) else {
        return;
    };
    let aux = tempfile::tempdir().unwrap();
    let wrapper = aux.path().join("failing-wrapper.sh");
    std::fs::write(
        &wrapper,
        "#!/bin/sh\necho 'sandbox: cannot apply the Landlock domain' >&2\nexit 125\n",
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let scenario = scenario
        .wrapper(wrapper)
        .confinement(enforced(LandlockSupport::Abi(6)));

    let (result, _) = scenario.empty_prompt().run(|_, _| {}).await;

    assert_eq!(result.outcome, RunOutcome::Failed);
    assert_eq!(result.run_report.outcome, RunOutcome::Failed as i32);
}

#[tokio::test]
async fn disabled_hides_the_runtime_variables_from_the_harness_but_keeps_the_kept_ones() {
    let Some(scenario) = RunScenario::new("run-env-disabled", SCRIPT) else {
        return;
    };
    let aux = tempfile::tempdir().unwrap();
    let record = aux.path().join("env");
    let dump = aux.path().join("dump-env.sh");
    std::fs::write(
        &dump,
        format!(
            "#!/bin/sh\nenv > '{}'\nexec '{}' \"$@\"\n",
            record.display(),
            scenario.fake_bin().display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&dump, std::fs::Permissions::from_mode(0o755)).unwrap();
    // The environment the runtime was started with, as the control plane sets it.
    let scenario = [
        ("DF_CONTROL_PLANE_ADDR", "10.0.0.1:9443"),
        ("DF_CONTROL_PLANE_SERVER_NAME", "control-plane"),
        ("DF_TLS_DIR", "/run/factory/tls"),
        ("DF_LANDLOCK", "disabled"),
        ("DF_LOG_FORMAT", "json"),
    ]
    .into_iter()
    .fold(scenario, |scenario, (name, value)| {
        scenario.runtime_var(name, value)
    })
    .harness_bin(dump)
    .confinement(Confinement::new(df_core::LandlockMode::Disabled));

    let (result, _) = scenario.release_up_front().run(|_, _| {}).await;

    assert_eq!(result.outcome, RunOutcome::EndTurn, "{result:?}");
    let env = std::fs::read_to_string(record).unwrap();
    let names: Vec<&str> = env
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name))
        .collect();
    for kept in ["DF_RUNTIME_SOCK", "DF_RUN_DIR", "DF_LOG_FORMAT", "PATH"] {
        assert!(names.contains(&kept), "{kept} in {names:?}");
    }
    for hidden in [
        "DF_CONTROL_PLANE_ADDR",
        "DF_CONTROL_PLANE_SERVER_NAME",
        "DF_TLS_DIR",
        "DF_LANDLOCK",
    ] {
        assert!(!names.contains(&hidden), "{hidden} reached the harness");
    }
}
