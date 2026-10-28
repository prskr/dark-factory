//! Generated Protobuf and gRPC bindings for Dark Factory contracts.
#![allow(clippy::all, clippy::pedantic)]

pub use pbjson_types;

pub mod transcript {
    pub mod v1 {
        tonic::include_proto!("darkfactory.transcript.v1");
        include!(concat!(
            env!("OUT_DIR"),
            "/darkfactory.transcript.v1.serde.rs"
        ));
    }
}

pub mod agent {
    pub mod v1 {
        tonic::include_proto!("darkfactory.agent.v1");
        include!(concat!(env!("OUT_DIR"), "/darkfactory.agent.v1.serde.rs"));
    }
}

pub mod gateway {
    pub mod v1 {
        tonic::include_proto!("darkfactory.gateway.v1");
        include!(concat!(env!("OUT_DIR"), "/darkfactory.gateway.v1.serde.rs"));
    }
}

#[cfg(test)]
mod tests {
    use super::agent::v1::{
        agent_channel_client::AgentChannelClient,
        agent_channel_server::{AgentChannel, AgentChannelServer},
    };
    use super::transcript::v1::*;
    use bytes::Bytes;
    use pbjson_types::Timestamp;
    use serde_json::Value;

    #[test]
    fn agent_channel_client_and_server_types_exist() {
        fn _assert_types<T: AgentChannel>(svc: T) {
            let _server = AgentChannelServer::new(svc);
        }
        let _client_type: Option<AgentChannelClient<tonic::transport::Channel>> = None;
    }

    #[test]
    fn gateway_services_and_flow_event_json_exist() {
        use super::gateway::v1::{
            FlowEvent, Verdict, gateway_log_server::GatewayLogServer,
            gateway_policy_client::GatewayPolicyClient,
        };
        let _client_type: Option<GatewayPolicyClient<tonic::transport::Channel>> = None;
        fn _server<T: super::gateway::v1::gateway_log_server::GatewayLog>(svc: T) {
            let _ = GatewayLogServer::new(svc);
        }
        let event = FlowEvent {
            host: "example.com".into(),
            verdict: Verdict::Denied.into(),
            reason: "not-allowlisted".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<FlowEvent>(&json).unwrap(), event);
        assert!(json.contains("VERDICT_DENIED"), "{json}");
    }

    fn base_event(kind: event::Kind) -> Event {
        Event {
            schema_version: 1,
            occurred_at: Some(Timestamp {
                seconds: 1_760_000_000,
                nanos: 123_000_000,
            }),
            turn_id: "turn-01".to_string(),
            kind: Some(kind),
        }
    }

    fn assert_roundtrip(event: &Event) -> Value {
        let json_str = serde_json::to_string(event).expect("serialise Event to canonical JSON");
        let value: Value = serde_json::from_str(&json_str).expect("parse JSON value");
        let decoded: Event =
            serde_json::from_str(&json_str).expect("deserialise Event from canonical JSON");
        assert_eq!(&decoded, event);

        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["turnId"], "turn-01");
        assert!(value["occurredAt"].is_string());
        value
    }

    #[test]
    fn roundtrip_all_event_kinds_in_canonical_protobuf_json() {
        // 1. TurnStarted
        let v = assert_roundtrip(&base_event(event::Kind::TurnStarted(TurnStarted {
            prompt_ids: vec!["p-1".into(), "p-2".into()],
        })));
        assert_eq!(
            v["turnStarted"]["promptIds"],
            serde_json::json!(["p-1", "p-2"])
        );

        // 2. UserMessage (with Human, Agent, System principals and ContentBlock kinds)
        let v = assert_roundtrip(&base_event(event::Kind::UserMessage(UserMessage {
            prompt_id: "p-1".into(),
            author: Some(Principal {
                kind: Some(principal::Kind::Human(HumanPrincipal {
                    user_id: "u-1".into(),
                    display_name: "Peter".into(),
                })),
            }),
            content: vec![
                ContentBlock {
                    kind: Some(content_block::Kind::Text(TextContent {
                        text: "Hello world".into(),
                    })),
                    hidden: false,
                },
                ContentBlock {
                    kind: Some(content_block::Kind::Image(ImageContent {
                        mime_type: "image/png".into(),
                        data: Some(BlobData {
                            source: Some(blob_data::Source::Sha256("deadbeef".into())),
                        }),
                    })),
                    hidden: false,
                },
                ContentBlock {
                    kind: Some(content_block::Kind::ResourceLink(ResourceLink {
                        uri: "file:///README.md".into(),
                        name: "README.md".into(),
                        mime_type: "text/markdown".into(),
                        title: Some("Readme".into()),
                        size_bytes: Some(4096),
                    })),
                    hidden: false,
                },
                ContentBlock {
                    kind: Some(content_block::Kind::Resource(EmbeddedResource {
                        uri: "file:///blob.bin".into(),
                        mime_type: "application/octet-stream".into(),
                        contents: Some(embedded_resource::Contents::Blob(BlobData {
                            source: Some(blob_data::Source::Inline(Bytes::from_static(b"abc"))),
                        })),
                    })),
                    hidden: false,
                },
            ],
        })));
        assert_eq!(v["userMessage"]["promptId"], "p-1");
        assert_eq!(v["userMessage"]["author"]["human"]["displayName"], "Peter");
        // uint64 size_bytes must serialize as a string in canonical protobuf JSON
        assert_eq!(
            v["userMessage"]["content"][2]["resourceLink"]["sizeBytes"],
            "4096"
        );

        // 3. AgentMessageChunk
        let v = assert_roundtrip(&base_event(event::Kind::AgentMessageChunk(
            AgentMessageChunk {
                message_id: "m-1".into(),
                content: Some(ContentBlock {
                    kind: Some(content_block::Kind::Text(TextContent {
                        text: "Working on it...".into(),
                    })),
                    hidden: false,
                }),
            },
        )));
        assert_eq!(v["agentMessageChunk"]["messageId"], "m-1");

        // 4. AgentThoughtChunk
        let v = assert_roundtrip(&base_event(event::Kind::AgentThoughtChunk(
            AgentThoughtChunk {
                message_id: "t-1".into(),
                content: Some(ContentBlock {
                    kind: Some(content_block::Kind::Text(TextContent {
                        text: "Thinking...".into(),
                    })),
                    hidden: false,
                }),
            },
        )));
        assert_eq!(v["agentThoughtChunk"]["messageId"], "t-1");

        // 5. ToolCall
        let v = assert_roundtrip(&base_event(event::Kind::ToolCall(ToolCall {
            tool_call_id: "tc-1".into(),
            title: "Edit src/lib.rs".into(),
            kind: ToolKind::Edit as i32,
            status: ToolCallStatus::InProgress as i32,
            content: vec![ToolCallContent {
                kind: Some(tool_call_content::Kind::Diff(Diff {
                    path: "src/lib.rs".into(),
                    old_text: Some("old".into()),
                    new_text: "new".into(),
                })),
            }],
            locations: vec![Location {
                path: "src/lib.rs".into(),
                line: Some(42),
            }],
            raw_input_json: r#"{"path":"src/lib.rs"}"#.into(),
            raw_output_json: String::new(),
            origin: ToolOrigin::Harness as i32,
            factory_call_id: String::new(),
            truncated: false,
        })));
        assert_eq!(v["toolCall"]["toolCallId"], "tc-1");
        assert_eq!(v["toolCall"]["kind"], "TOOL_KIND_EDIT");
        assert_eq!(v["toolCall"]["status"], "TOOL_CALL_STATUS_IN_PROGRESS");
        assert_eq!(v["toolCall"]["origin"], "TOOL_ORIGIN_HARNESS");

        // 6. ToolCallUpdate
        let v = assert_roundtrip(&base_event(event::Kind::ToolCallUpdate(ToolCallUpdate {
            tool_call_id: "tc-1".into(),
            title: Some("Edited src/lib.rs".into()),
            kind: Some(ToolKind::Edit as i32),
            status: Some(ToolCallStatus::Completed as i32),
            content: Some(ToolCallContentList { items: vec![] }),
            locations: Some(LocationList { items: vec![] }),
            raw_input_json: None,
            raw_output_json: Some(r#"{"ok":true}"#.into()),
            truncated: Some(true),
        })));
        assert_eq!(v["toolCallUpdate"]["toolCallId"], "tc-1");
        assert_eq!(v["toolCallUpdate"]["status"], "TOOL_CALL_STATUS_COMPLETED");
        assert_eq!(v["toolCallUpdate"]["truncated"], true);

        // 7. Plan
        let v = assert_roundtrip(&base_event(event::Kind::Plan(Plan {
            entries: vec![PlanEntry {
                content: "Write unit tests".into(),
                priority: PlanEntryPriority::High as i32,
                status: PlanEntryStatus::InProgress as i32,
            }],
        })));
        assert_eq!(
            v["plan"]["entries"][0]["priority"],
            "PLAN_ENTRY_PRIORITY_HIGH"
        );
        assert_eq!(
            v["plan"]["entries"][0]["status"],
            "PLAN_ENTRY_STATUS_IN_PROGRESS"
        );

        // 8. TurnEnded (checking enum names + 64-bit integer fields as strings)
        let v = assert_roundtrip(&base_event(event::Kind::TurnEnded(TurnEnded {
            stop_reason: StopReason::EndTurn as i32,
            usage: vec![Usage {
                model_id: "claude-sonnet-4-5".into(),
                input_tokens: 9_007_199_254_740_993,
                output_tokens: 2048,
                cache_read_tokens: 512,
                cache_write_tokens: 128,
            }],
            usage_source: UsageSource::Harness as i32,
            detail: String::new(),
        })));
        assert_eq!(v["turnEnded"]["stopReason"], "STOP_REASON_END_TURN");
        assert_eq!(v["turnEnded"]["usageSource"], "USAGE_SOURCE_HARNESS");
        assert_eq!(
            v["turnEnded"]["usage"][0]["inputTokens"],
            "9007199254740993"
        );
        assert_eq!(v["turnEnded"]["usage"][0]["outputTokens"], "2048");

        // 9. Notice
        let v = assert_roundtrip(&base_event(event::Kind::Notice(Notice {
            level: NoticeLevel::Warning as i32,
            code: "workspace.diverged".into(),
            message: "Task branch diverged from origin".into(),
        })));
        assert_eq!(v["notice"]["level"], "NOTICE_LEVEL_WARNING");
        assert_eq!(v["notice"]["code"], "workspace.diverged");
    }
}
