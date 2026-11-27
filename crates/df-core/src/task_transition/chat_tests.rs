//! Tests of the chat rules of the task status machine (archiving, no done, no cancel).

use super::*;
use TaskStatus::*;

fn chat(status: TaskStatus) -> TaskSnapshot {
    TaskSnapshot {
        status,
        kind: TaskKind::Chat,
        ..TaskSnapshot::new_task()
    }
}

fn archived(status: TaskStatus) -> TaskSnapshot {
    TaskSnapshot {
        archived: true,
        ..chat(status)
    }
}

fn board() -> Principal {
    Principal::system("test-board")
}

fn sets_archived(next: &Transition, by: Option<Principal>) -> bool {
    next.effects.contains(&Effect::SetArchived(by))
}

fn audits(next: &Transition, wanted: &str) -> bool {
    next.effects
        .iter()
        .any(|e| matches!(e, Effect::Audit { action, .. } if *action == wanted))
}

#[test]
fn a_chat_cannot_be_marked_done_or_cancelled() {
    for status in [Waiting, Blocked, Failed, Queued] {
        let done = transition(&chat(status), TaskEvent::MarkedDone(DoneBy::Board));
        assert_eq!(done, Err(Rejected::Chat), "{status}");
        let cancelled = transition(&chat(status), TaskEvent::Cancelled { via: None });
        assert_eq!(cancelled, Err(Rejected::Chat), "{status}");
    }
}

#[test]
fn an_idle_chat_is_archived_with_its_status_kept() {
    let next = transition(&chat(Waiting), TaskEvent::Archived { by: board() }).unwrap();
    assert_eq!(next.status, Waiting);
    assert!(sets_archived(&next, Some(board())));
    assert!(audits(&next, "chat.archive"));
}

#[test]
fn a_chat_in_a_run_or_with_an_open_request_is_not_archived() {
    for status in [Queued, Running] {
        let result = transition(&chat(status), TaskEvent::Archived { by: board() });
        assert_eq!(result, Err(Rejected::ArchiveBusy(status)), "{status}");
    }
    let open = TaskSnapshot {
        pending_approvals: true,
        ..chat(Waiting)
    };
    assert_eq!(
        transition(&open, TaskEvent::Archived { by: board() }),
        Err(Rejected::ArchiveOpenRequest)
    );
}

#[test]
fn archiving_applies_to_open_chats_and_unarchiving_to_archived_ones() {
    let task = TaskSnapshot::new_task();
    assert_eq!(
        transition(&task, TaskEvent::Archived { by: board() }),
        Err(Rejected::NotAChat)
    );
    assert_eq!(
        transition(&task, TaskEvent::Unarchived),
        Err(Rejected::NotAChat)
    );
    assert_eq!(
        transition(&archived(Waiting), TaskEvent::Archived { by: board() }),
        Err(Rejected::AlreadyArchived)
    );
    assert_eq!(
        transition(&chat(Waiting), TaskEvent::Unarchived),
        Err(Rejected::NotArchived)
    );
}

#[test]
fn unarchiving_queues_the_chat_only_when_messages_wait() {
    let quiet = transition(&archived(Waiting), TaskEvent::Unarchived).unwrap();
    assert_eq!(quiet.status, Waiting);
    assert!(sets_archived(&quiet, None));
    assert!(audits(&quiet, "chat.unarchive"));
    let waiting_message = TaskSnapshot {
        undelivered_messages: true,
        ..archived(Waiting)
    };
    let next = transition(&waiting_message, TaskEvent::Unarchived).unwrap();
    assert_eq!(next.status, Queued);
}

#[test]
fn a_child_ending_queues_its_message_on_an_archived_chat() {
    let ended = ChildEnded {
        key: "DF-20".into(),
        title: "Review".into(),
        how: ChildEnd::Done,
        detail: String::new(),
        summary: String::new(),
        pr_url: None,
        artifacts: Vec::new(),
        waiting_dependents: Vec::new(),
    };
    let next = transition(&archived(Waiting), TaskEvent::ChildEnded(ended)).unwrap();
    assert!(
        next.effects
            .iter()
            .any(|e| matches!(e, Effect::QueueMessage { .. }))
    );
}
