//! Tests of the run state rules, one per guard of the state machine.

use super::*;
use RunEffect::{Publish, SandboxClosed, SandboxProvisioned, SandboxProvisioning, Task};
use RunState::*;

fn snapshot(state: RunState) -> RunSnapshot {
    RunSnapshot {
        run_id: RunId::new(),
        state,
        stop_requested_at: None,
        connected_at: None,
        provisioning_began: false,
        finalize_reason: None,
        undelivered_messages: false,
        started_at: Timestamp::now(),
    }
}

fn connected(state: RunState) -> RunSnapshot {
    RunSnapshot {
        connected_at: Some(Timestamp::now()),
        ..snapshot(state)
    }
}

fn stopped(state: RunState) -> RunSnapshot {
    RunSnapshot {
        stop_requested_at: Some(Timestamp::now()),
        ..connected(state)
    }
}

fn apply(run: &RunSnapshot, event: RunEvent) -> RunTransition {
    run_transition(run, event).unwrap()
}

fn end(run: &RunSnapshot, outcome: RunOutcome) -> RunEvent {
    RunEvent::Ended(RunEnd::failure(run.run_id, outcome, "detail"))
}

#[test]
fn provisioning_starts_only_for_a_pending_run() {
    let next = apply(&snapshot(Pending), RunEvent::ProvisioningStarted);
    assert_eq!(next.state, Provisioning);
    assert!(next.set.stamp_provisioning_started);
    assert_eq!(next.effects, [SandboxProvisioning, Publish]);
    for state in RunState::ALL.iter().filter(|s| **s != Pending) {
        let next = apply(&snapshot(*state), RunEvent::ProvisioningStarted);
        assert!(!next.changes(), "{state}");
        assert_eq!(next.state, *state);
    }
}

#[test]
fn a_started_sandbox_connects_a_pending_or_provisioning_run() {
    for state in [Pending, Provisioning] {
        let event = RunEvent::SandboxStarted {
            pod_ip: "10.0.0.7".into(),
        };
        let next = apply(&snapshot(state), event);
        assert_eq!(next.state, Connecting, "{state}");
        assert_eq!(next.set.pod_ip.as_deref(), Some("10.0.0.7"));
        assert!(next.set.stamp_sandbox_started);
        assert_eq!(next.effects, [Publish]);
    }
    for state in RunState::ALL
        .iter()
        .filter(|s| !matches!(s, Pending | Provisioning))
    {
        let event = RunEvent::SandboxStarted {
            pod_ip: "10.0.0.7".into(),
        };
        assert!(!apply(&snapshot(*state), event).changes(), "{state}");
    }
}

#[test]
fn a_started_sandbox_announces_provisioned_only_after_its_provisioning_began() {
    let run = RunSnapshot {
        provisioning_began: true,
        ..snapshot(Provisioning)
    };
    let event = RunEvent::SandboxStarted {
        pod_ip: "ip".into(),
    };
    let next = apply(&run, event);
    assert_eq!(next.effects, [SandboxProvisioned, Publish]);
}

#[test]
fn the_first_connect_moves_an_early_run_to_preparing() {
    for state in [Pending, Provisioning, Connecting] {
        let next = apply(&snapshot(state), RunEvent::Connected);
        assert_eq!(next.state, Preparing, "{state}");
        assert!(next.set.stamp_connected);
        assert_eq!(next.effects, [Publish]);
    }
}

#[test]
fn a_connect_of_a_run_stopped_before_it_connected_finalizes_it_for_the_stop() {
    for state in [Pending, Provisioning, Connecting] {
        let run = RunSnapshot {
            stop_requested_at: Some(Timestamp::now()),
            ..snapshot(state)
        };
        let next = apply(&run, RunEvent::Connected);
        assert_eq!(next.state, Finalizing, "{state}");
        assert_eq!(next.set.finalize_reason, Some(FinalizeReason::Stop));
        assert!(next.set.stamp_connected);
        assert_eq!(next.effects, [Publish]);
    }
}

#[test]
fn a_connect_leaves_the_state_of_a_later_run_and_records_the_time_once() {
    let next = apply(&snapshot(Running), RunEvent::Connected);
    assert_eq!(next.state, Running);
    assert!(next.set.stamp_connected);
    let next = apply(&connected(Running), RunEvent::Connected);
    assert!(!next.changes(), "a repeated connect changes nothing");
}

#[test]
fn a_connect_of_a_terminal_run_changes_nothing() {
    for state in [Succeeded, Failed, Cancelled] {
        let next = apply(&snapshot(state), RunEvent::Connected);
        assert!(!next.changes(), "{state}");
        assert_eq!(next.state, state);
    }
}

#[test]
fn a_turn_start_moves_a_commandable_run_to_running_and_the_task_with_it() {
    for state in [Preparing, AwaitingInput] {
        let next = apply(&connected(state), RunEvent::TurnStarted);
        assert_eq!(next.state, Running, "{state}");
        assert_eq!(next.effects, [Publish, Task(TaskCue::TurnStarted)]);
    }
}

#[test]
fn a_turn_end_moves_a_commandable_run_to_awaiting_input_and_the_task_with_it() {
    for state in [Preparing, Running] {
        let next = apply(&connected(state), RunEvent::TurnEnded);
        assert_eq!(next.state, AwaitingInput, "{state}");
        assert_eq!(next.effects, [Publish, Task(TaskCue::TurnEnded)]);
    }
}

#[test]
fn a_repeated_turn_event_changes_nothing() {
    assert!(!apply(&connected(Running), RunEvent::TurnStarted).changes());
    assert!(!apply(&connected(AwaitingInput), RunEvent::TurnEnded).changes());
}

#[test]
fn a_finalizing_or_terminal_run_is_never_reopened_by_a_turn_event() {
    for state in [Finalizing, Succeeded, Failed, Cancelled] {
        for event in [RunEvent::TurnStarted, RunEvent::TurnEnded] {
            let next = apply(&connected(state), event);
            assert!(!next.changes(), "{state}");
            assert_eq!(next.state, state);
        }
    }
}

#[test]
fn an_idle_run_without_an_undelivered_message_finalizes_for_idle() {
    let next = apply(
        &connected(AwaitingInput),
        RunEvent::Finalize(FinalizeReason::Idle),
    );
    assert_eq!(next.state, Finalizing);
    assert_eq!(next.set.finalize_reason, Some(FinalizeReason::Idle));
    assert_eq!(next.effects, [Publish]);
}

#[test]
fn an_idle_run_with_an_undelivered_message_does_not_finalize_for_idle() {
    let run = RunSnapshot {
        undelivered_messages: true,
        ..connected(AwaitingInput)
    };
    assert!(!apply(&run, RunEvent::Finalize(FinalizeReason::Idle)).changes());
}

#[test]
fn only_an_idle_run_finalizes_for_idle() {
    for state in RunState::ALL.iter().filter(|s| **s != AwaitingInput) {
        let next = apply(&connected(*state), RunEvent::Finalize(FinalizeReason::Idle));
        assert!(!next.changes(), "{state}");
    }
}

#[test]
fn a_done_task_finalizes_an_idle_run_whatever_waits() {
    let run = RunSnapshot {
        undelivered_messages: true,
        ..connected(AwaitingInput)
    };
    let next = apply(&run, RunEvent::Finalize(FinalizeReason::Done));
    assert_eq!(next.state, Finalizing);
    assert_eq!(next.set.finalize_reason, Some(FinalizeReason::Done));
    assert!(
        !apply(
            &connected(Running),
            RunEvent::Finalize(FinalizeReason::Done)
        )
        .changes()
    );
}

#[test]
fn the_budget_finalizes_every_run_that_is_not_ending() {
    for state in [Preparing, Running, AwaitingInput] {
        let next = apply(
            &connected(state),
            RunEvent::Finalize(FinalizeReason::Budget),
        );
        assert_eq!(next.state, Finalizing, "{state}");
        assert_eq!(next.set.finalize_reason, Some(FinalizeReason::Budget));
    }
    for state in [Finalizing, Succeeded, Failed, Cancelled] {
        let next = apply(
            &connected(state),
            RunEvent::Finalize(FinalizeReason::Budget),
        );
        assert!(!next.changes(), "{state}");
    }
}

fn finalizing_for(reason: FinalizeReason) -> RunSnapshot {
    RunSnapshot {
        finalize_reason: Some(reason),
        ..connected(Finalizing)
    }
}

#[test]
fn the_budget_takes_over_a_run_that_is_finalizing_for_idle() {
    let next = apply(
        &finalizing_for(FinalizeReason::Idle),
        RunEvent::Finalize(FinalizeReason::Budget),
    );
    assert_eq!(next.state, Finalizing);
    assert_eq!(next.set.finalize_reason, Some(FinalizeReason::Budget));
    assert_eq!(next.effects, [Publish]);
}

#[test]
fn the_budget_leaves_the_reason_of_a_run_finalizing_for_a_stop_a_done_task_or_the_budget() {
    for reason in [
        FinalizeReason::Stop,
        FinalizeReason::Done,
        FinalizeReason::Budget,
    ] {
        let next = apply(
            &finalizing_for(reason),
            RunEvent::Finalize(FinalizeReason::Budget),
        );
        assert!(!next.changes(), "{reason}");
    }
}

#[test]
fn an_idle_finalize_leaves_a_finalizing_run_alone() {
    for current in FinalizeReason::ALL {
        let next = apply(
            &finalizing_for(*current),
            RunEvent::Finalize(FinalizeReason::Idle),
        );
        assert!(!next.changes(), "idle on {current}");
    }
}

#[test]
fn done_takes_over_a_run_finalizing_for_idle() {
    let next = apply(
        &finalizing_for(FinalizeReason::Idle),
        RunEvent::Finalize(FinalizeReason::Done),
    );
    assert_eq!(next.state, Finalizing);
    assert_eq!(next.set.finalize_reason, Some(FinalizeReason::Done));
    assert_eq!(next.effects, [Publish]);
}

#[test]
fn done_leaves_a_run_finalizing_for_the_budget_a_stop_or_done_alone() {
    for current in [
        FinalizeReason::Budget,
        FinalizeReason::Stop,
        FinalizeReason::Done,
    ] {
        let next = apply(
            &finalizing_for(current),
            RunEvent::Finalize(FinalizeReason::Done),
        );
        assert!(!next.changes(), "done on {current}");
    }
}

#[test]
fn a_stop_marks_a_connected_run_and_finalizes_it() {
    for state in [Preparing, Running, AwaitingInput] {
        let next = apply(
            &connected(state),
            RunEvent::StopRequested(StopScope::ActiveOnly),
        );
        assert_eq!(next.state, Finalizing, "{state}");
        assert!(next.set.stamp_stop_requested);
        assert_eq!(next.set.finalize_reason, Some(FinalizeReason::Stop));
        assert_eq!(next.effects, [Publish]);
    }
}

#[test]
fn a_stop_marks_an_unconnected_run_and_leaves_its_state_to_the_scheduler() {
    for state in [Pending, Provisioning, Connecting] {
        let next = apply(
            &snapshot(state),
            RunEvent::StopRequested(StopScope::ActiveOnly),
        );
        assert_eq!(next.state, state);
        assert!(next.set.stamp_stop_requested);
        assert_eq!(next.set.finalize_reason, None);
    }
}

#[test]
fn a_stop_leaves_a_finalizing_run_alone_unless_the_task_is_cancelled() {
    let run = connected(Finalizing);
    assert!(!apply(&run, RunEvent::StopRequested(StopScope::ActiveOnly)).changes());
    let next = apply(
        &run,
        RunEvent::StopRequested(StopScope::IncludingFinalizing),
    );
    assert_eq!(next.state, Finalizing);
    assert!(next.set.stamp_stop_requested);
    assert_eq!(next.set.finalize_reason, Some(FinalizeReason::Stop));
}

#[test]
fn a_stop_of_a_stopped_run_changes_nothing() {
    for scope in [StopScope::ActiveOnly, StopScope::IncludingFinalizing] {
        assert!(!apply(&stopped(Finalizing), RunEvent::StopRequested(scope)).changes());
        assert!(!apply(&stopped(Running), RunEvent::StopRequested(scope)).changes());
    }
}

#[test]
fn a_stop_of_a_terminal_run_is_rejected() {
    for state in [Succeeded, Failed, Cancelled] {
        for scope in [StopScope::ActiveOnly, StopScope::IncludingFinalizing] {
            assert_eq!(
                run_transition(&connected(state), RunEvent::StopRequested(scope)),
                Err(Rejected::Stop(state))
            );
        }
    }
}

#[test]
fn an_end_moves_a_run_to_the_state_of_its_outcome_and_raises_the_task_event() {
    for outcome in RunOutcome::ALL {
        let run = connected(Running);
        let next = apply(&run, end(&run, *outcome));
        assert_eq!(next.state, outcome.run_state(), "{outcome}");
        assert_eq!(next.set.end.as_ref().map(|e| e.outcome), Some(*outcome));
        assert_eq!(next.effects, [Task(TaskCue::RunEnded), Publish]);
    }
}

#[test]
fn an_end_of_a_run_whose_provisioning_began_closes_the_sandbox_events_first() {
    let run = RunSnapshot {
        provisioning_began: true,
        ..snapshot(Provisioning)
    };
    let next = apply(&run, end(&run, RunOutcome::ProvisioningFailed));
    assert_eq!(
        next.effects,
        [SandboxClosed, Task(TaskCue::RunEnded), Publish]
    );
}

#[test]
fn an_end_of_a_terminal_run_changes_nothing() {
    for state in [Succeeded, Failed, Cancelled] {
        let run = connected(state);
        let next = apply(&run, end(&run, RunOutcome::Failed));
        assert!(!next.changes(), "{state}");
        assert_eq!(next.state, state);
    }
}

fn cfg() -> RunsConfig {
    RunsConfig {
        max_wall_clock_seconds: 1000,
        connect_timeout_seconds: 60,
        cancel_grace_seconds: 100,
        ..RunsConfig::default()
    }
}

fn ago(seconds: i64) -> Timestamp {
    Timestamp::now() - SignedDuration::from_secs(seconds)
}

fn overdue_end(run: &RunSnapshot) -> Option<(RunOutcome, String)> {
    match overdue(run, Timestamp::now(), &cfg())? {
        RunEvent::Ended(end) => Some((end.outcome, end.detail)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_stopped_run_without_a_connection_ends_at_once() {
    for state in [Pending, Provisioning, Connecting] {
        let run = RunSnapshot {
            stop_requested_at: Some(Timestamp::now()),
            ..snapshot(state)
        };
        assert_eq!(
            overdue_end(&run),
            Some((RunOutcome::Cancelled, "stopped".into())),
            "{state}"
        );
    }
}

#[test]
fn a_stop_that_was_not_obeyed_ends_the_run_after_the_stop_timeout() {
    let late = RunSnapshot {
        stop_requested_at: Some(ago(i64::try_from(STOP_TIMEOUT_SECONDS).unwrap() + 1)),
        ..connected(Finalizing)
    };
    assert_eq!(
        overdue_end(&late),
        Some((RunOutcome::Cancelled, "stop-timeout".into()))
    );
    let early = RunSnapshot {
        stop_requested_at: Some(ago(5)),
        ..connected(Finalizing)
    };
    assert_eq!(overdue_end(&early), None);
}

#[test]
fn a_run_that_did_not_connect_in_time_fails_with_a_connect_timeout() {
    let run = RunSnapshot {
        started_at: ago(61),
        ..snapshot(Provisioning)
    };
    let (outcome, detail) = overdue_end(&run).unwrap();
    assert_eq!(outcome, RunOutcome::ConnectTimeout);
    assert_eq!(detail, "the sandbox did not connect within 60 s");
    let fresh = RunSnapshot {
        started_at: ago(30),
        ..snapshot(Provisioning)
    };
    assert_eq!(overdue_end(&fresh), None);
}

#[test]
fn a_connected_run_fails_after_budget_plus_two_grace_periods() {
    for state in [Preparing, Running, AwaitingInput, Finalizing] {
        let run = RunSnapshot {
            connected_at: Some(ago(1201)),
            ..snapshot(state)
        };
        let (outcome, _) = overdue_end(&run).unwrap();
        assert_eq!(outcome, RunOutcome::Failed, "{state}");
        let within = RunSnapshot {
            connected_at: Some(ago(1100)),
            ..snapshot(state)
        };
        assert_eq!(overdue_end(&within), None, "{state}");
    }
}

#[test]
fn the_overdue_branches_apply_in_the_order_of_the_enforcement() {
    // A stopped, unconnected run that is also past its connect timeout is `stopped`.
    let run = RunSnapshot {
        started_at: ago(500),
        stop_requested_at: Some(ago(500)),
        ..snapshot(Pending)
    };
    assert_eq!(
        overdue_end(&run),
        Some((RunOutcome::Cancelled, "stopped".into()))
    );
    // A stop past its timeout wins over the hard stop.
    let run = RunSnapshot {
        connected_at: Some(ago(5000)),
        stop_requested_at: Some(ago(500)),
        ..snapshot(Finalizing)
    };
    assert_eq!(
        overdue_end(&run),
        Some((RunOutcome::Cancelled, "stop-timeout".into()))
    );
}

#[test]
fn a_terminal_run_is_never_overdue() {
    let run = RunSnapshot {
        stop_requested_at: Some(ago(5000)),
        ..connected(Failed)
    };
    assert_eq!(overdue(&run, Timestamp::now(), &cfg()), None);
}

#[test]
fn an_overdue_event_ends_the_run_on_the_snapshot_and_otherwise_changes_nothing() {
    let late = RunSnapshot {
        started_at: ago(61),
        ..snapshot(Pending)
    };
    let event = RunEvent::Overdue {
        now: Timestamp::now(),
        limits: cfg(),
    };
    let next = apply(&late, event.clone());
    assert_eq!(next.state, Failed);
    // The run connected after the candidate list was read.
    let moved = RunSnapshot {
        started_at: ago(61),
        ..connected(Running)
    };
    assert!(!apply(&moved, event).changes());
}

#[test]
fn an_unrepresentable_budget_means_no_deadline() {
    let cfg = RunsConfig {
        max_wall_clock_seconds: u64::MAX,
        ..cfg()
    };
    let run = RunSnapshot {
        connected_at: Some(ago(100_000)),
        ..snapshot(Running)
    };
    assert_eq!(overdue(&run, Timestamp::now(), &cfg), None);
    assert_eq!(cancel_deadline(Timestamp::now(), &cfg), None);
}

#[test]
fn a_finalize_reason_names_its_command() {
    assert_eq!(FinalizeReason::Idle.command(), RunCommand::Release);
    for reason in [
        FinalizeReason::Budget,
        FinalizeReason::Stop,
        FinalizeReason::Done,
    ] {
        assert!(
            matches!(reason.command(), RunCommand::Cancel(_)),
            "{reason}"
        );
    }
    assert_eq!(
        FinalizeReason::Done.command(),
        RunCommand::Cancel("task marked done")
    );
}

#[test]
fn a_finalize_reason_round_trips_through_its_name() {
    for reason in FinalizeReason::ALL {
        assert_eq!(reason.as_str().parse::<FinalizeReason>().unwrap(), *reason);
    }
    assert!("other".parse::<FinalizeReason>().is_err());
}

fn event_name(event: &RunEvent) -> &'static str {
    match event {
        RunEvent::ProvisioningStarted => "ProvisioningStarted",
        RunEvent::SandboxStarted { .. } => "SandboxStarted",
        RunEvent::Connected => "Connected",
        RunEvent::TurnStarted => "TurnStarted",
        RunEvent::TurnEnded => "TurnEnded",
        RunEvent::Finalize(_) => "Finalize",
        RunEvent::StopRequested(_) => "StopRequested",
        RunEvent::Ended(_) => "Ended",
        RunEvent::Overdue { .. } => "Overdue",
    }
}

#[test]
fn the_state_diagram_names_every_state_event_and_finalize_reason() {
    let doc = include_str!("../../docs/run-transitions.md");
    for state in RunState::ALL {
        assert!(doc.contains(state.as_str()), "state {state}");
    }
    for reason in FinalizeReason::ALL {
        assert!(doc.contains(reason.as_str()), "reason {reason}");
    }
    let events = [
        RunEvent::ProvisioningStarted,
        RunEvent::SandboxStarted {
            pod_ip: String::new(),
        },
        RunEvent::Connected,
        RunEvent::TurnStarted,
        RunEvent::TurnEnded,
        RunEvent::Finalize(FinalizeReason::Idle),
        RunEvent::StopRequested(StopScope::ActiveOnly),
        end(&snapshot(Running), RunOutcome::Failed),
        RunEvent::Overdue {
            now: Timestamp::now(),
            limits: cfg(),
        },
    ];
    for event in &events {
        assert!(
            doc.contains(event_name(event)),
            "event {}",
            event_name(event)
        );
    }
}
