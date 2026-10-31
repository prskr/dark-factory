//! mTLS connect, control plane restart and replay, leftover outbox, certificate rotation, outages and shutdown flush.

#![expect(
    clippy::too_many_lines,
    reason = "scenario tests set up and check a whole run"
)]

use df_proto::agent::v1::{
    AgentMessage, ControlMessage, Hello, HelloAck, agent_message, control_message,
};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

use crate::connection::{TlsFiles, connect_channel};
use crate::test_harness::{MockControlPlane, TestPki};

#[tokio::test]
async fn test_mtls_connect_success_with_server_name_verification() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki generation failed");

    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start failed");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    pki.write_client_tls_dir(temp_dir.path())
        .expect("write certs");
    let tls_files = TlsFiles::load_from_dir(temp_dir.path()).expect("load certs");

    let addr_str = format!("https://{}", server.addr);
    let mut client = connect_channel(&addr_str, server_name, &tls_files)
        .await
        .expect("client connect failed");

    let (out_tx, out_rx) = tokio::sync::mpsc::channel(10);
    let out_stream = ReceiverStream::new(out_rx);

    let response = client
        .connect(out_stream)
        .await
        .expect("connect rpc failed");
    let mut in_stream = response.into_inner();

    // 1. Client sends Hello message
    let hello_msg = AgentMessage {
        seq: 0,
        body: Some(agent_message::Body::Hello(Hello {
            runtime_version: "0.1.0".to_string(),
            capabilities: vec!["fake-cap".to_string()],
            previous_run_id: String::new(),
        })),
    };
    out_tx.send(hello_msg.clone()).await.expect("send hello");

    // 2. Server receives Hello
    let received_by_server = server
        .recv_agent_message()
        .await
        .expect("server received message");
    assert_eq!(received_by_server.seq, 0);
    match received_by_server.body {
        Some(agent_message::Body::Hello(h)) => {
            assert_eq!(h.runtime_version, "0.1.0");
            assert_eq!(h.capabilities, vec!["fake-cap".to_string()]);
        }
        other => panic!("expected Hello, got {other:?}"),
    }

    // 3. Server sends HelloAck
    let ack_msg = ControlMessage {
        ack: 0,
        body: Some(control_message::Body::HelloAck(HelloAck {
            run_id: "run-42".to_string(),
            last_persisted_seq: 0,
            capabilities: vec![],
        })),
    };
    server
        .send_control_message(ack_msg)
        .await
        .expect("server send ack");

    // 4. Client receives HelloAck
    let received_by_client = in_stream
        .next()
        .await
        .expect("client received item")
        .expect("client ok");
    match received_by_client.body {
        Some(control_message::Body::HelloAck(ack)) => {
            assert_eq!(ack.run_id, "run-42");
        }
        other => panic!("expected HelloAck, got {other:?}"),
    }

    server.shutdown();
}

#[tokio::test]
async fn test_mtls_connect_fails_on_server_name_mismatch() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki generation failed");

    let server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start failed");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    pki.write_client_tls_dir(temp_dir.path())
        .expect("write certs");
    let tls_files = TlsFiles::load_from_dir(temp_dir.path()).expect("load certs");

    let addr_str = format!("https://{}", server.addr);
    // Configured client server_name does NOT match the certificate SAN
    let wrong_server_name = "rogue-server.darkfactory.local";

    let connect_res = connect_channel(&addr_str, wrong_server_name, &tls_files).await;
    match connect_res {
        Err(e) => {
            // Expected connection / TLS handshake failure
            let err_str = e.to_string();
            assert!(
                err_str.contains("transport error")
                    || err_str.contains("certificate")
                    || err_str.contains("handshake")
                    || err_str.contains("dns")
                    || err_str.contains("InvalidDnsName"),
                "unexpected error message: {err_str}"
            );
        }
        Ok(mut client) => {
            // In case lazy handshake happened, RPC must fail
            let (_tx, rx) = tokio::sync::mpsc::channel::<AgentMessage>(1);
            let rpc_res = client.connect(ReceiverStream::new(rx)).await;
            assert!(
                rpc_res.is_err(),
                "RPC should fail when server name mismatches"
            );
        }
    }

    server.shutdown();
}

#[tokio::test]
async fn test_mtls_connect_fails_on_untrusted_ca() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki generation failed");

    // Server presents cert signed by rogue CA
    let (rogue_cert, rogue_key) = pki
        .generate_server_cert_for_name(server_name)
        .expect("rogue cert");
    let server = MockControlPlane::start(&rogue_cert, &rogue_key, &pki.ca_cert_pem)
        .await
        .expect("server start failed");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    pki.write_client_tls_dir(temp_dir.path())
        .expect("write certs");
    let tls_files = TlsFiles::load_from_dir(temp_dir.path()).expect("load certs");

    let addr_str = format!("https://{}", server.addr);
    let connect_res = connect_channel(&addr_str, server_name, &tls_files).await;
    match connect_res {
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                err_str.contains("transport error")
                    || err_str.contains("certificate")
                    || err_str.contains("handshake")
                    || err_str.contains("UnknownIssuer"),
                "unexpected error message: {err_str}"
            );
        }
        Ok(mut client) => {
            let (_tx, rx) = tokio::sync::mpsc::channel::<AgentMessage>(1);
            let rpc_res = client.connect(ReceiverStream::new(rx)).await;
            assert!(rpc_res.is_err(), "RPC should fail when CA is untrusted");
        }
    }

    server.shutdown();
}

#[tokio::test]
async fn test_mtls_connect_fails_on_untrusted_client() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki generation failed");

    let server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start failed");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    pki.write_client_tls_dir(temp_dir.path())
        .expect("write certs");

    // Replace client cert/key with untrusted ones signed by an unknown rogue CA
    let (rogue_client_cert, rogue_client_key) =
        TestPki::generate_untrusted_client().expect("rogue client");
    std::fs::write(temp_dir.path().join("tls.crt"), rogue_client_cert).expect("write rogue cert");
    std::fs::write(temp_dir.path().join("tls.key"), rogue_client_key).expect("write rogue key");

    let tls_files = TlsFiles::load_from_dir(temp_dir.path()).expect("load certs");

    let addr_str = format!("https://{}", server.addr);
    let connect_res = connect_channel(&addr_str, server_name, &tls_files).await;
    match connect_res {
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                err_str.contains("transport error")
                    || err_str.contains("certificate")
                    || err_str.contains("handshake")
                    || err_str.contains("UnknownIssuer"),
                "unexpected error message: {err_str}"
            );
        }
        Ok(mut client) => {
            let (_tx, rx) = tokio::sync::mpsc::channel::<AgentMessage>(1);
            let rpc_res = client.connect(ReceiverStream::new(rx)).await;
            assert!(
                rpc_res.is_err(),
                "RPC should fail when client certificate is untrusted"
            );
        }
    }

    server.shutdown();
}

#[tokio::test]
async fn test_scenario_control_plane_restart_and_outbox_replay() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    // Pre-populate outbox with 20 messages
    let outbox = {
        let mut ob =
            crate::outbox::Outbox::open(&runtime_dir, "run-restart", 1024 * 1024, 64 * 1024)
                .expect("open outbox");
        for i in 1..=20 {
            ob.append(AgentMessage {
                seq: 0,
                body: Some(agent_message::Body::PhaseChanged(
                    df_proto::agent::v1::PhaseChanged {
                        phase: df_proto::agent::v1::RunPhase::Running as i32,
                        detail: format!("Task step {i}"),
                    },
                )),
            })
            .expect("append");
        }
        std::sync::Arc::new(tokio::sync::Mutex::new(ob))
    };

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir,
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(config, outbox, reconnect_policy);
    let handle = cm.spawn();

    // 1. Control plane receives Hello
    let hello = server.recv_agent_message().await.expect("recv hello");
    assert_eq!(hello.seq, 0);

    // 2. Control plane responds with HelloAck where last_persisted_seq = 15
    server
        .send_control_message(ControlMessage {
            ack: 15,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-restart".to_string(),
                last_persisted_seq: 15,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    // 3. Control plane receives replayed messages 16 through 20
    for expected_seq in 16..=20 {
        let replayed = server
            .recv_agent_message()
            .await
            .expect("recv replayed message");
        assert_eq!(replayed.seq, expected_seq);
    }

    // 4. Runtime sends message 21
    handle
        .send(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Finalizing as i32,
                    detail: "Continuing with 21".to_string(),
                },
            )),
        })
        .await
        .expect("send 21");

    let msg21 = server.recv_agent_message().await.expect("recv 21");
    assert_eq!(msg21.seq, 21);

    handle.shutdown();
    server.shutdown();
}

#[tokio::test]
async fn test_scenario_leftover_from_crashed_run_emits_notice() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    // Outbox holds messages from a crashed run
    let outbox = {
        let mut ob =
            crate::outbox::Outbox::open(&runtime_dir, "run-crashed", 1024 * 1024, 64 * 1024)
                .expect("open outbox");
        ob.append(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Running as i32,
                    detail: "Crashed run left behind".to_string(),
                },
            )),
        })
        .expect("append");
        std::sync::Arc::new(tokio::sync::Mutex::new(ob))
    };

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir,
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(
        config,
        std::sync::Arc::clone(&outbox),
        reconnect_policy,
    );
    let handle = cm.spawn();

    // 1. Control plane receives Hello with previous_run_id = "run-crashed"
    let hello = server.recv_agent_message().await.expect("recv hello");
    match hello.body {
        Some(agent_message::Body::Hello(h)) => {
            assert_eq!(h.previous_run_id, "run-crashed");
        }
        other => panic!("expected Hello, got {other:?}"),
    }

    // 2. Control plane acknowledges with a new run_id
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-fresh-assignment".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    // 3. Runtime detects mismatch, discards old outbox, and emits Notice with code "runtime.outbox_discarded"
    let notice_msg = server
        .recv_agent_message()
        .await
        .expect("recv notice message");
    assert_eq!(notice_msg.seq, 1);
    match notice_msg.body {
        Some(agent_message::Body::TranscriptEvent(ev)) => match ev.kind {
            Some(df_proto::transcript::v1::event::Kind::Notice(n)) => {
                assert_eq!(n.code, "runtime.outbox_discarded");
                assert_eq!(
                    n.level,
                    df_proto::transcript::v1::NoticeLevel::Warning as i32
                );
            }
            other => panic!("expected Notice, got {other:?}"),
        },
        other => panic!("expected TranscriptEvent, got {other:?}"),
    }

    // Verify outbox state reflects the new run_id
    assert_eq!(
        outbox.lock().await.run_state().run_id,
        "run-fresh-assignment"
    );

    handle.shutdown();
    server.shutdown();
}

#[tokio::test]
async fn test_scenario_fully_acknowledged_leftover_is_discarded_silently() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    // Outbox of a previous run whose messages were all acknowledged
    let outbox = {
        let mut ob =
            crate::outbox::Outbox::open(&runtime_dir, "run-crashed", 1024 * 1024, 64 * 1024)
                .expect("open outbox");
        ob.append(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Running as i32,
                    detail: "Crashed run left behind".to_string(),
                },
            )),
        })
        .expect("append");
        ob.acknowledge(1).expect("acknowledge");
        std::sync::Arc::new(tokio::sync::Mutex::new(ob))
    };

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir,
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(
        config,
        std::sync::Arc::clone(&outbox),
        reconnect_policy,
    );
    let handle = cm.spawn();

    // 1. Control plane receives Hello with previous_run_id = "run-crashed"
    let hello = server.recv_agent_message().await.expect("recv hello");
    match hello.body {
        Some(agent_message::Body::Hello(h)) => {
            assert_eq!(h.previous_run_id, "run-crashed");
        }
        other => panic!("expected Hello, got {other:?}"),
    }

    // 2. Control plane acknowledges with a new run_id
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-fresh-assignment".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    // 3. The outbox is reset to the new run without a notice. Discard and notice happen under
    // one lock, so a new run id with an empty outbox means no notice was appended.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let ob = outbox.lock().await;
            if ob.run_state().run_id == "run-fresh-assignment" {
                assert!(ob.is_empty(), "no notice may be queued");
                break;
            }
            drop(ob);
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("outbox was reset to the new run");

    handle.shutdown();
    server.shutdown();
}

#[tokio::test]
async fn test_scenario_certificate_rotated() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::outbox::Outbox::open(&runtime_dir, "run-cert", 1024 * 1024, 64 * 1024)
            .expect("open outbox"),
    ));

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir: tls_dir.clone(),
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(config, outbox, reconnect_policy);
    let handle = cm.spawn();

    // 1. First connection
    let _ = server.recv_agent_message().await.expect("recv first hello");
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-cert".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    assert_eq!(
        server
            .connection_count
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );

    // 2. Rotate certificate files on disk (simulating cert-manager csi-driver replacing files)
    let (rotated_cert, rotated_key) = pki.generate_rotated_client_cert().expect("rotated cert");
    std::fs::write(tls_dir.join("tls.crt"), rotated_cert).expect("write rotated cert");
    std::fs::write(tls_dir.join("tls.key"), rotated_key).expect("write rotated key");

    // 3. Drop active stream
    server.drop_active_stream().await;

    // 4. ConnectionManager reconnects with the new certificate files
    let hello2 = server
        .recv_agent_message()
        .await
        .expect("recv second hello after reconnect");
    assert_eq!(hello2.seq, 0);

    assert_eq!(
        server
            .connection_count
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );

    handle.shutdown();
    server.shutdown();
}

#[tokio::test]
async fn test_scenario_short_outage_events_queue_in_outbox() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::outbox::Outbox::open(&runtime_dir, "run-outage", 1024 * 1024, 64 * 1024)
            .expect("open outbox"),
    ));

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir: tls_dir.clone(),
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(
        config,
        std::sync::Arc::clone(&outbox),
        reconnect_policy,
    );
    let handle = cm.spawn();

    // Initial connection
    let _ = server.recv_agent_message().await.expect("recv hello");
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-outage".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    // Server drops stream, causing an outage
    server.drop_active_stream().await;

    // Give connection loop a moment to observe the stream drop
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // During the outage, the runtime queues messages
    handle
        .send(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Running as i32,
                    detail: "Queued during outage 1".to_string(),
                },
            )),
        })
        .await
        .expect("queue 1");

    handle
        .send(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::PhaseChanged(
                df_proto::agent::v1::PhaseChanged {
                    phase: df_proto::agent::v1::RunPhase::Running as i32,
                    detail: "Queued during outage 2".to_string(),
                },
            )),
        })
        .await
        .expect("queue 2");

    // Reconnect happens, Hello is received (skipping any in-flight message before the drop)
    loop {
        let msg = server.recv_agent_message().await.expect("recv message");
        if matches!(msg.body, Some(agent_message::Body::Hello(_))) {
            break;
        }
    }

    // Server acknowledges with last_persisted_seq = 0
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-outage".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack after outage");

    // Both queued events are received in order
    let q1 = server.recv_agent_message().await.expect("recv q1");
    assert_eq!(q1.seq, 1);
    let q2 = server.recv_agent_message().await.expect("recv q2");
    assert_eq!(q2.seq, 2);

    handle.shutdown();
    server.shutdown();
}

#[tokio::test]
async fn test_scenario_prompt_resent_after_reconnect_delivered_once() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::outbox::Outbox::open(&runtime_dir, "run-idemp", 1024 * 1024, 64 * 1024)
            .expect("open outbox"),
    ));

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir: tls_dir.clone(),
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(config, outbox, reconnect_policy);
    let mut handle = cm.spawn();

    // 1. Initial connection
    let _ = server.recv_agent_message().await.expect("recv hello");
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-idemp".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    // 2. Control plane sends prompt-1
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::Prompt(df_proto::agent::v1::Prompt {
                prompt_id: "prompt-1".to_string(),
                author: None,
                content: vec![df_proto::transcript::v1::ContentBlock {
                    kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                        df_proto::transcript::v1::TextContent {
                            text: "Do something".to_string(),
                        },
                    )),
                    hidden: false,
                }],
                queued_at: None,
            })),
        })
        .await
        .expect("send prompt-1");

    // 3. Runtime receives prompt-1
    let received_prompt = handle.recv().await.expect("handle received prompt-1");
    match received_prompt.body {
        Some(control_message::Body::Prompt(p)) => assert_eq!(p.prompt_id, "prompt-1"),
        other => panic!("expected Prompt, got {other:?}"),
    }

    // 4. Stream drops
    server.drop_active_stream().await;

    // 5. Reconnect happens, Hello is received
    let _ = server
        .recv_agent_message()
        .await
        .expect("recv hello after reconnect");

    // Server re-sends HelloAck
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-idemp".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack after reconnect");

    // 6. Control plane re-sends prompt-1
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::Prompt(df_proto::agent::v1::Prompt {
                prompt_id: "prompt-1".to_string(),
                author: None,
                content: vec![df_proto::transcript::v1::ContentBlock {
                    kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                        df_proto::transcript::v1::TextContent {
                            text: "Do something".to_string(),
                        },
                    )),
                    hidden: false,
                }],
                queued_at: None,
            })),
        })
        .await
        .expect("send duplicate prompt-1");

    // 7. Control plane sends new prompt-2
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::Prompt(df_proto::agent::v1::Prompt {
                prompt_id: "prompt-2".to_string(),
                author: None,
                content: vec![df_proto::transcript::v1::ContentBlock {
                    kind: Some(df_proto::transcript::v1::content_block::Kind::Text(
                        df_proto::transcript::v1::TextContent {
                            text: "Do next thing".to_string(),
                        },
                    )),
                    hidden: false,
                }],
                queued_at: None,
            })),
        })
        .await
        .expect("send prompt-2");

    // 8. Runtime MUST receive prompt-2 directly, ignoring the duplicate prompt-1!
    let next_prompt = tokio::time::timeout(std::time::Duration::from_secs(2), handle.recv())
        .await
        .expect("did not timeout waiting for prompt")
        .expect("received message");

    match next_prompt.body {
        Some(control_message::Body::Prompt(p)) => {
            assert_eq!(
                p.prompt_id, "prompt-2",
                "duplicate prompt-1 should have been filtered"
            );
        }
        other => panic!("expected prompt-2, got {other:?}"),
    }

    handle.shutdown();
    server.shutdown();
}

#[tokio::test]
async fn test_scenario_pending_tool_call_resent_after_reconnect() {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let tls_dir = temp_dir.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).expect("write certs");
    let runtime_dir = temp_dir.path().join("runtime");

    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::outbox::Outbox::open(&runtime_dir, "run-tool", 1024 * 1024, 64 * 1024)
            .expect("open outbox"),
    ));

    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir: tls_dir.clone(),
        data_dir: temp_dir.path().join("data"),
        run_dir: runtime_dir.clone(),
    };

    let reconnect_policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };

    let cm = crate::connection::ConnectionManager::new(config, outbox, reconnect_policy);
    let mut handle = cm.spawn();

    // 1. Initial connection
    let _ = server.recv_agent_message().await.expect("recv hello");
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-tool".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack");

    // 2. Runtime sends ToolCallRequest
    handle
        .send(AgentMessage {
            seq: 0,
            body: Some(agent_message::Body::ToolCallRequest(
                df_proto::agent::v1::ToolCallRequest {
                    call_id: "call-xyz".to_string(),
                    tool_name: "test_tool".to_string(),
                    arguments_json: "{}".to_string(),
                },
            )),
        })
        .await
        .expect("send tool call");

    let received_call = server
        .recv_agent_message()
        .await
        .expect("server recv tool call");
    assert_eq!(received_call.seq, 1);

    // 3. Control plane acknowledges persistence (ack = 1), but drops before sending ToolCallResult!
    server
        .send_control_message(ControlMessage { ack: 1, body: None })
        .await
        .expect("send ack");

    // Drop active stream
    server.drop_active_stream().await;

    // 4. Reconnect happens, Hello is received
    let _ = server
        .recv_agent_message()
        .await
        .expect("recv hello after drop");

    // Control plane acknowledges Hello with last_persisted_seq = 1
    server
        .send_control_message(ControlMessage {
            ack: 1,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-tool".to_string(),
                last_persisted_seq: 1,
                capabilities: vec![],
            })),
        })
        .await
        .expect("send ack after reconnect");

    // 5. Runtime MUST re-send the pending ToolCallRequest with the exact same call_id!
    let resent_call = server
        .recv_agent_message()
        .await
        .expect("server received re-sent tool call");
    match resent_call.body {
        Some(agent_message::Body::ToolCallRequest(req)) => {
            assert_eq!(req.call_id, "call-xyz");
            assert_eq!(req.tool_name, "test_tool");
        }
        other => panic!("expected ToolCallRequest, got {other:?}"),
    }

    // 6. Control plane responds with ToolCallResult
    server
        .send_control_message(ControlMessage {
            ack: resent_call.seq,
            body: Some(control_message::Body::ToolCallResult(
                df_proto::agent::v1::ToolCallResult {
                    call_id: "call-xyz".to_string(),
                    is_error: false,
                    content: vec![],
                    structured_content_json: "{\"status\":\"ok\"}".to_string(),
                    end_run_after_turn: false,
                    await_input_after_turn: false,
                },
            )),
        })
        .await
        .expect("send tool result");

    // 7. Runtime receives result
    let res = handle.recv().await.expect("recv tool call result");
    match res.body {
        Some(control_message::Body::ToolCallResult(r)) => {
            assert_eq!(r.call_id, "call-xyz");
            assert_eq!(r.structured_content_json, "{\"status\":\"ok\"}");
        }
        other => panic!("expected ToolCallResult, got {other:?}"),
    }

    handle.shutdown();
    server.shutdown();
}

fn flush_test_report() -> AgentMessage {
    AgentMessage {
        seq: 0,
        body: Some(agent_message::Body::RunReport(
            df_proto::agent::v1::RunReport {
                outcome: df_proto::agent::v1::RunOutcome::EndTurn as i32,
                detail: "done".to_string(),
                retry_after: None,
                harness_session_id: String::new(),
                workspace: None,
            },
        )),
    }
}

/// Starts a connection manager against a mock control plane and completes the first handshake.
async fn start_flush_test_connection(
    run_id: &str,
) -> (
    crate::connection::ConnectionHandle,
    MockControlPlane,
    tempfile::TempDir,
) {
    let server_name = "control-plane.darkfactory.local";
    let pki = TestPki::generate(server_name).expect("pki");
    let mut server =
        MockControlPlane::start(&pki.server_cert_pem, &pki.server_key_pem, &pki.ca_cert_pem)
            .await
            .expect("server start");
    let temp = tempfile::tempdir().unwrap();
    let tls_dir = temp.path().join("tls");
    pki.write_client_tls_dir(&tls_dir).unwrap();
    let runtime_dir = temp.path().join("runtime");
    let outbox = std::sync::Arc::new(tokio::sync::Mutex::new(
        crate::outbox::Outbox::open(&runtime_dir, run_id, 1024 * 1024, 64 * 1024).unwrap(),
    ));
    let config = crate::config::RuntimeConfig {
        control_plane_addr: format!("https://{}", server.addr),
        server_name: server_name.to_string(),
        tls_dir,
        data_dir: temp.path().join("data"),
        run_dir: runtime_dir,
    };
    let policy = crate::connection::ReconnectPolicy {
        initial_delay: std::time::Duration::from_millis(20),
        max_delay: std::time::Duration::from_millis(100),
        backoff_factor: 1.5,
    };
    let handle = crate::connection::ConnectionManager::new(config, outbox, policy).spawn();
    let _ = server.recv_agent_message().await.expect("hello");
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: run_id.to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .unwrap();
    (handle, server, temp)
}

/// Receives messages until the `RunReport` arrives, acknowledges it, and returns its seq.
async fn receive_and_ack_report(server: &mut MockControlPlane) -> u64 {
    let report = loop {
        let msg = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            server.recv_agent_message(),
        )
        .await
        .expect("RunReport must be delivered")
        .expect("stream open");
        if matches!(msg.body, Some(agent_message::Body::RunReport(_))) {
            break msg;
        }
    };
    server
        .send_control_message(ControlMessage {
            ack: report.seq,
            body: None,
        })
        .await
        .unwrap();
    report.seq
}

#[tokio::test]
async fn test_final_report_is_flushed_at_shutdown() {
    let (mut handle, mut server, _temp) = start_flush_test_connection("run-flush").await;
    handle.send(flush_test_report()).await.unwrap();
    handle.shutdown();

    assert_eq!(receive_and_ack_report(&mut server).await, 1);
    let task = handle.task.take().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .expect("connection manager must finish once the report is acked")
        .unwrap();
    server.shutdown();
}

#[tokio::test]
async fn test_final_report_is_delivered_after_shutdown_while_disconnected() {
    let (mut handle, mut server, _temp) = start_flush_test_connection("run-flush-offline").await;
    server.drop_active_stream().await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    handle.send(flush_test_report()).await.unwrap();
    handle.shutdown();

    // The manager must reconnect instead of exiting
    loop {
        let msg = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            server.recv_agent_message(),
        )
        .await
        .expect("manager must reconnect after shutdown")
        .expect("stream open");
        if matches!(msg.body, Some(agent_message::Body::Hello(_))) {
            break;
        }
    }
    server
        .send_control_message(ControlMessage {
            ack: 0,
            body: Some(control_message::Body::HelloAck(HelloAck {
                run_id: "run-flush-offline".to_string(),
                last_persisted_seq: 0,
                capabilities: vec![],
            })),
        })
        .await
        .unwrap();
    receive_and_ack_report(&mut server).await;
    let task = handle.task.take().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .expect("connection manager must finish once the report is acked")
        .unwrap();
    server.shutdown();
}

#[test]
fn test_discarded_outbox_notice_names_the_loss_or_that_it_is_unknown() {
    use crate::connection::discarded_outbox_notice;

    assert_eq!(discarded_outbox_notice(Ok(0)), None);
    assert_eq!(
        discarded_outbox_notice(Ok(3)).as_deref(),
        Some("3 unsent message(s) from the previous run were discarded")
    );
    // An unreadable outbox may have held unsent messages: warn without a count.
    let unknown = discarded_outbox_notice(Err(anyhow::anyhow!("permission denied")));
    assert!(
        unknown
            .as_deref()
            .is_some_and(|m| m.contains("could not be read")),
        "{unknown:?}"
    );
}
