//! Outbox sequence numbers, ack trimming, backpressure, discard and run state.

use df_proto::agent::v1::{AgentMessage, agent_message};

#[tokio::test]
async fn test_outbox_seq_assignment_and_disk_recovery() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let runtime_dir = temp_dir.path().join("runtime");

    // 1. Initial run: append 3 messages
    {
        let mut outbox =
            crate::outbox::Outbox::open(&runtime_dir, "run-100", 1024 * 1024, 64 * 1024)
                .expect("open outbox");

        let seq1 = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                        detail: "Starting".to_string(),
                    },
                )),
            })
            .expect("append 1");
        assert_eq!(seq1, 1);

        let seq2 = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Running as i32,
                        detail: "Work in progress".to_string(),
                    },
                )),
            })
            .expect("append 2");
        assert_eq!(seq2, 2);

        let seq3 = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Finalizing as i32,
                        detail: "Wrapping up".to_string(),
                    },
                )),
            })
            .expect("append 3");
        assert_eq!(seq3, 3);
    } // Outbox dropped (simulating process restart)

    // 2. Restart recovery: re-open same outbox directory
    {
        let mut outbox =
            crate::outbox::Outbox::open(&runtime_dir, "run-100", 1024 * 1024, 64 * 1024)
                .expect("reopen outbox");
        assert_eq!(outbox.run_state().run_id, "run-100");
        assert_eq!(outbox.run_state().next_seq, 4);

        // Read unacknowledged after seq 1 (should return seq 2 and 3)
        let unacked = outbox.read_unacknowledged(1).expect("read unacked");
        assert_eq!(unacked.len(), 2);
        assert_eq!(unacked[0].seq, 2);
        assert_eq!(unacked[1].seq, 3);

        // Append after restart: next seq is 4
        let seq4 = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Finalizing as i32,
                        detail: "Done".to_string(),
                    },
                )),
            })
            .expect("append 4");
        assert_eq!(seq4, 4);
    }
}

#[tokio::test]
async fn test_outbox_ack_trimming_deletes_segments() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let runtime_dir = temp_dir.path().join("runtime");

    // Use tiny segment size (15 bytes) so each message creates a new segment file
    let mut outbox =
        crate::outbox::Outbox::open(&runtime_dir, "run-101", 1024 * 1024, 15).expect("open outbox");

    for i in 1..=5 {
        let seq = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Running as i32,
                        detail: format!("Step {i}"),
                    },
                )),
            })
            .expect("append");
        assert_eq!(seq, i);
    }

    // Verify multiple segment files exist
    let outbox_dir = runtime_dir.join("outbox");
    let seg_count = std::fs::read_dir(&outbox_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
        .count();
    assert!(
        seg_count >= 4,
        "expected at least 4 segments, got {seg_count}"
    );

    // Acknowledge up to seq 3
    outbox.acknowledge(3).expect("ack 3");

    // Segments with max seq <= 3 should be deleted
    let remaining_unacked = outbox.read_unacknowledged(0).expect("read unacked");
    assert_eq!(remaining_unacked.len(), 2);
    assert_eq!(remaining_unacked[0].seq, 4);
    assert_eq!(remaining_unacked[1].seq, 5);

    let remaining_seg_count = std::fs::read_dir(&outbox_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
        .count();
    assert!(
        remaining_seg_count < seg_count,
        "segment files should have been trimmed: before={seg_count}, after={remaining_seg_count}"
    );
}

#[tokio::test]
async fn test_outbox_bounded_backpressure() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let runtime_dir = temp_dir.path().join("runtime");

    // Bound outbox to 120 bytes, with segment size 40 bytes
    let mut outbox =
        crate::outbox::Outbox::open(&runtime_dir, "run-102", 120, 40).expect("open outbox");

    // Append 3 messages (~35 bytes each) until outbox is nearly full (3 * 35 = 105 bytes)
    outbox
        .append(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                    detail: "M1".repeat(15),
                },
            )),
        })
        .expect("append 1");

    outbox
        .append(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                    detail: "M2".repeat(15),
                },
            )),
        })
        .expect("append 2");

    outbox
        .append(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                    detail: "M3".repeat(15),
                },
            )),
        })
        .expect("append 3");

    // Outbox size should now be near or exceeding remaining space for a new message
    let outbox_arc = std::sync::Arc::new(tokio::sync::Mutex::new(outbox));
    let outbox_clone = std::sync::Arc::clone(&outbox_arc);

    // Spawning a task to append Msg4 via append_with_backpressure, which will wait on backpressure without holding the outbox lock
    let append_handle = tokio::spawn(async move {
        crate::outbox::Outbox::append_with_backpressure(
            &outbox_clone,
            AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Running as i32,
                        detail: "M4".repeat(15),
                    },
                )),
            },
        )
        .await
    });

    // Give it a moment to block
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Acknowledge msg 1 and 2 to free space
    {
        let mut ob = outbox_arc.lock().await;
        ob.acknowledge(2).expect("ack 2");
    }

    // Now append should unblock and complete successfully
    let res = tokio::time::timeout(std::time::Duration::from_secs(2), append_handle)
        .await
        .expect("append should not timeout")
        .expect("join ok")
        .expect("append succeeded");

    assert_eq!(res, 4);
}

#[tokio::test]
async fn test_outbox_discard_on_new_run() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let runtime_dir = temp_dir.path().join("runtime");

    // Run 1: append messages
    {
        let mut outbox =
            crate::outbox::Outbox::open(&runtime_dir, "run-old", 1024 * 1024, 64 * 1024)
                .expect("open outbox");
        outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                        detail: "Old run message".to_string(),
                    },
                )),
            })
            .expect("append");
        assert_eq!(outbox.read_unacknowledged(0).unwrap().len(), 1);
    }

    // Run 2: open with different run id
    {
        let mut outbox =
            crate::outbox::Outbox::open(&runtime_dir, "run-new", 1024 * 1024, 64 * 1024)
                .expect("open outbox with new run_id");
        assert_eq!(outbox.run_state().run_id, "run-new");
        assert_eq!(outbox.run_state().next_seq, 1);
        assert_eq!(outbox.read_unacknowledged(0).unwrap().len(), 0);

        // Appending to new run gets seq 1
        let seq = outbox
            .append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                        detail: "New run message".to_string(),
                    },
                )),
            })
            .expect("append in new run");
        assert_eq!(seq, 1);
    }
}

#[test]
fn test_save_run_state_is_atomic() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    let mut outbox = Outbox::open(temp.path(), "run-atomic-1", 1024 * 1024, 64 * 1024).unwrap();
    outbox
        .set_last_pushed_sha(Some("abcdef123456".to_string()))
        .unwrap();

    let run_json_path = temp.path().join("run.json");
    assert!(run_json_path.exists());
    let tmp_path = temp.path().join("run.tmp");
    assert!(
        !tmp_path.exists(),
        "temp file should be renamed over target"
    );

    let content = std::fs::read_to_string(&run_json_path).unwrap();
    assert!(content.contains("abcdef123456"));
}

#[test]
fn test_open_without_run_id_keeps_state() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    {
        let mut ob = Outbox::open(temp.path(), "run-a", 1024 * 1024, 64 * 1024).unwrap();
        ob.set_last_pushed_sha(Some("abc".to_string())).unwrap();
        ob.append(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                    detail: "kept".to_string(),
                },
            )),
        })
        .unwrap();
    }

    let ob = Outbox::open(temp.path(), "", 1024 * 1024, 64 * 1024).unwrap();
    assert_eq!(ob.run_state().run_id, "run-a");
    assert_eq!(ob.run_state().last_pushed_sha.as_deref(), Some("abc"));
    assert_eq!(ob.run_state().next_seq, 2);
    assert_eq!(ob.read_unacknowledged(0).unwrap().len(), 1);
}

#[test]
fn test_next_seq_survives_restart_after_ack() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    {
        let mut ob = Outbox::open(temp.path(), "run-a", 1024 * 1024, 64 * 1024).unwrap();
        for _ in 0..3 {
            ob.append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                        detail: String::new(),
                    },
                )),
            })
            .unwrap();
        }
        ob.acknowledge(3).unwrap();
        assert!(ob.is_empty());
    }

    // All segments are gone; the next seq must still not restart at 1
    let ob = Outbox::open(temp.path(), "", 1024 * 1024, 64 * 1024).unwrap();
    assert_eq!(ob.run_state().next_seq, 4);
}

#[test]
fn test_corrupt_run_json_is_repaired() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("run.json"), "{not json").unwrap();
    let ob = Outbox::open(temp.path(), "run-x", 1024 * 1024, 64 * 1024).unwrap();
    assert_eq!(ob.run_state().run_id, "run-x");
    let stored = std::fs::read_to_string(temp.path().join("run.json")).unwrap();
    assert!(serde_json::from_str::<crate::outbox::RunState>(&stored).is_ok());
}

#[tokio::test]
async fn test_oversized_message_does_not_block_empty_outbox() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    let ob = Outbox::open(temp.path(), "run-a", 10, 64 * 1024).unwrap();
    let ob = std::sync::Arc::new(tokio::sync::Mutex::new(ob));
    let msg = AgentMessage {
        seq: 0,
        body: Some(agent_message::Body::PhaseChanged(
            df_proto::agent::v1::PhaseChanged {
                phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                detail: "larger than the whole outbox".to_string(),
            },
        )),
    };
    let seq = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        Outbox::append_with_backpressure(&ob, msg),
    )
    .await
    .expect("must not wait forever")
    .unwrap();
    assert_eq!(seq, 1);
}

#[tokio::test]
async fn test_append_or_stop_appends_when_stopped_while_full() {
    use crate::outbox::Outbox;

    let temp = tempfile::tempdir().unwrap();
    let ob = std::sync::Arc::new(tokio::sync::Mutex::new(
        Outbox::open(temp.path(), "run-a", 10, 64 * 1024).unwrap(),
    ));
    let msg = |detail: &str| AgentMessage {
        seq: 0,
        body: Some(agent_message::Body::PhaseChanged(
            df_proto::agent::v1::PhaseChanged {
                phase: df_proto::agent::v1::RunPhase::Preparing as i32,
                detail: detail.to_string(),
            },
        )),
    };
    // Fills the outbox beyond its (soft) limit
    Outbox::append_with_backpressure(&ob, msg("first message"))
        .await
        .unwrap();

    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let ob2 = std::sync::Arc::clone(&ob);
    let waiting = tokio::spawn(async move {
        Outbox::append_or_stop(&ob2, msg("second message"), &mut stop_rx).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !waiting.is_finished(),
        "must wait for space while not stopped"
    );

    stop_tx.send(true).unwrap();
    let seq = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
        .await
        .expect("stop must release the wait")
        .unwrap()
        .unwrap();
    assert_eq!(seq, 2);
    assert_eq!(ob.lock().await.read_unacknowledged(0).unwrap().len(), 2);
}
