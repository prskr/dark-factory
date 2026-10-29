//! Translation of ACP updates into transcript events.

#[test]
fn test_scenario_text_after_a_tool_call() {
    use crate::transcript::TranscriptTranslator;
    use agent_client_protocol::schema::v1::{
        ContentBlock as AcpContentBlock, ContentChunk, TextContent as AcpTextContent,
        ToolCall as AcpToolCall, ToolCallId,
    };
    use df_proto::transcript::v1::{
        content_block::Kind as ContentBlockKind, event::Kind as EventKind,
    };

    let mut translator = TranscriptTranslator::new("turn-1");

    // 1. Agent writes text chunk: "Hello"
    let c1 = ContentChunk::new(AcpContentBlock::Text(AcpTextContent::new("Hello")));
    let ev1 = translator.translate_agent_message_chunk(&c1);
    let (msg_id_1, text1) = match ev1.kind.unwrap() {
        EventKind::AgentMessageChunk(chunk) => {
            let text = match chunk.content.unwrap().kind.unwrap() {
                ContentBlockKind::Text(t) => t.text,
                _ => panic!("Expected text"),
            };
            (chunk.message_id, text)
        }
        _ => panic!("Expected AgentMessageChunk"),
    };
    assert_eq!(msg_id_1, "msg-1");
    assert_eq!(text1, "Hello");

    // 2. Agent writes more text immediately: " world"
    let c2 = ContentChunk::new(AcpContentBlock::Text(AcpTextContent::new(" world")));
    let ev2 = translator.translate_agent_message_chunk(&c2);
    let (msg_id_2, text2) = match ev2.kind.unwrap() {
        EventKind::AgentMessageChunk(chunk) => {
            let text = match chunk.content.unwrap().kind.unwrap() {
                ContentBlockKind::Text(t) => t.text,
                _ => panic!("Expected text"),
            };
            (chunk.message_id, text)
        }
        _ => panic!("Expected AgentMessageChunk"),
    };
    // Must be the same message id
    assert_eq!(msg_id_2, "msg-1");
    assert_eq!(text2, " world");

    // 3. Agent calls a tool
    let tool_call = AcpToolCall::new(ToolCallId::new("call-1"), "read_file");
    let ev_tool = translator.translate_tool_call(&tool_call);
    match ev_tool.kind.unwrap() {
        EventKind::ToolCall(tc) => {
            assert_eq!(tc.tool_call_id, "call-1");
            assert_eq!(tc.title, "read_file");
        }
        _ => panic!("Expected ToolCall"),
    }

    // 4. Agent writes more text: "The file contains..."
    let c3 = ContentChunk::new(AcpContentBlock::Text(AcpTextContent::new(
        "The file contains...",
    )));
    let ev3 = translator.translate_agent_message_chunk(&c3);
    let (msg_id_3, text3) = match ev3.kind.unwrap() {
        EventKind::AgentMessageChunk(chunk) => {
            let text = match chunk.content.unwrap().kind.unwrap() {
                ContentBlockKind::Text(t) => t.text,
                _ => panic!("Expected text"),
            };
            (chunk.message_id, text)
        }
        _ => panic!("Expected AgentMessageChunk"),
    };
    // Must carry a DIFFERENT message id!
    assert_ne!(msg_id_3, msg_id_1);
    assert_eq!(msg_id_3, "msg-2");
    assert_eq!(text3, "The file contains...");

    // 5. Agent writes another chunk: " done."
    let c4 = ContentChunk::new(AcpContentBlock::Text(AcpTextContent::new(" done.")));
    let ev4 = translator.translate_agent_message_chunk(&c4);
    let msg_id_4 = match ev4.kind.unwrap() {
        EventKind::AgentMessageChunk(chunk) => chunk.message_id,
        _ => panic!("Expected AgentMessageChunk"),
    };
    assert_eq!(msg_id_4, "msg-2");
}

#[test]
fn test_scenario_large_tool_output() {
    use crate::transcript::{MAX_RAW_IO_BYTES, TranscriptTranslator};
    use agent_client_protocol::schema::v1::{
        Content, ContentBlock as AcpContentBlock, TextContent as AcpTextContent,
        ToolCall as AcpToolCall, ToolCallContent as AcpToolCallContent, ToolCallId,
        ToolCallUpdate as AcpToolCallUpdate, ToolCallUpdateFields,
    };
    use df_proto::transcript::v1::{
        content_block::Kind as ContentBlockKind, event::Kind as EventKind,
        tool_call_content::Kind as ToolCallContentKind,
    };

    let mut translator = TranscriptTranslator::new("turn-1");

    // Output of 200 KiB
    let large_output = "a".repeat(200 * 1024);

    // Case 1: In ToolCall.raw_output
    let mut call = AcpToolCall::new(ToolCallId::new("call-large-1"), "bash");
    call.raw_output = Some(serde_json::Value::String(large_output.clone()));
    let ev1 = translator.translate_tool_call(&call);
    match ev1.kind.unwrap() {
        EventKind::ToolCall(tc) => {
            assert!(tc.truncated, "Event must be marked truncated");
            assert!(
                tc.raw_output_json.len() <= MAX_RAW_IO_BYTES,
                "raw_output_json len {} must be <= 64 KiB",
                tc.raw_output_json.len()
            );
            assert_eq!(tc.raw_output_json.len(), MAX_RAW_IO_BYTES);
        }
        _ => panic!("Expected ToolCall"),
    }

    // Case 2: In ToolCallUpdate.content
    let fields =
        ToolCallUpdateFields::new().content(vec![AcpToolCallContent::Content(Content::new(
            AcpContentBlock::Text(AcpTextContent::new(large_output.clone())),
        ))]);
    let update = AcpToolCallUpdate::new(ToolCallId::new("call-large-2"), fields);
    let ev2 = translator.translate_tool_call_update(&update);
    match ev2.kind.unwrap() {
        EventKind::ToolCallUpdate(tcu) => {
            assert_eq!(tcu.truncated, Some(true), "Update must be marked truncated");
            let content_list = tcu.content.expect("content list");
            assert_eq!(content_list.items.len(), 1);
            match content_list.items[0].kind.as_ref().unwrap() {
                ToolCallContentKind::Content(block) => match block.kind.as_ref().unwrap() {
                    ContentBlockKind::Text(t) => {
                        assert!(
                            t.text.len() <= MAX_RAW_IO_BYTES,
                            "Text len {} must be <= 64 KiB",
                            t.text.len()
                        );
                        assert_eq!(t.text.len(), MAX_RAW_IO_BYTES);
                    }
                    _ => panic!("Expected text content block"),
                },
                ToolCallContentKind::Diff(_) => panic!("Expected content kind"),
            }
        }
        _ => panic!("Expected ToolCallUpdate"),
    }
}

#[test]
fn test_transcript_plan_and_tool_origin() {
    use crate::transcript::TranscriptTranslator;
    use agent_client_protocol::schema::v1::{
        Plan as AcpPlan, PlanEntry as AcpPlanEntry, PlanEntryPriority as AcpPlanPriority,
        PlanEntryStatus as AcpPlanStatus, StopReason as AcpStopReason, ToolCall as AcpToolCall,
        ToolCallId,
    };
    use df_proto::transcript::v1::{
        PlanEntryPriority, PlanEntryStatus, StopReason, ToolOrigin, Usage, UsageSource,
        event::Kind as EventKind,
    };

    let mut translator = TranscriptTranslator::new("turn-1");

    // 1. ToolOrigin determination
    let factory_call = AcpToolCall::new(ToolCallId::new("c1"), "mcp__factory__complete_task");
    let ev_f = translator.translate_tool_call(&factory_call);
    match ev_f.kind.unwrap() {
        EventKind::ToolCall(tc) => {
            assert_eq!(tc.origin, ToolOrigin::Factory as i32);
        }
        _ => panic!("Expected ToolCall"),
    }

    let harness_call = AcpToolCall::new(ToolCallId::new("c2"), "git_diff");
    let ev_h = translator.translate_tool_call(&harness_call);
    match ev_h.kind.unwrap() {
        EventKind::ToolCall(tc) => {
            assert_eq!(tc.origin, ToolOrigin::Harness as i32);
        }
        _ => panic!("Expected ToolCall"),
    }

    // 2. Plan translation
    let plan = AcpPlan::new(vec![
        AcpPlanEntry::new("Step 1", AcpPlanPriority::High, AcpPlanStatus::Completed),
        AcpPlanEntry::new("Step 2", AcpPlanPriority::Medium, AcpPlanStatus::InProgress),
        AcpPlanEntry::new("Step 3", AcpPlanPriority::Low, AcpPlanStatus::Pending),
    ]);
    let ev_plan = translator.translate_plan(&plan);
    match ev_plan.kind.unwrap() {
        EventKind::Plan(p) => {
            assert_eq!(p.entries.len(), 3);
            assert_eq!(p.entries[0].content, "Step 1");
            assert_eq!(p.entries[0].priority, PlanEntryPriority::High as i32);
            assert_eq!(p.entries[0].status, PlanEntryStatus::Completed as i32);
            assert_eq!(p.entries[1].priority, PlanEntryPriority::Medium as i32);
            assert_eq!(p.entries[1].status, PlanEntryStatus::InProgress as i32);
            assert_eq!(p.entries[2].priority, PlanEntryPriority::Low as i32);
            assert_eq!(p.entries[2].status, PlanEntryStatus::Pending as i32);
        }
        _ => panic!("Expected Plan"),
    }

    // 3. TurnEnded translation
    let usage = vec![Usage {
        model_id: "claude-3-5-sonnet".to_string(),
        input_tokens: 1500,
        output_tokens: 300,
        cache_read_tokens: 50,
        cache_write_tokens: 10,
    }];
    let ev_ended = translator.translate_turn_ended(
        AcpStopReason::EndTurn,
        usage,
        UsageSource::Harness,
        "finished normally",
    );
    match ev_ended.kind.unwrap() {
        EventKind::TurnEnded(te) => {
            assert_eq!(te.stop_reason, StopReason::EndTurn as i32);
            assert_eq!(te.usage_source, UsageSource::Harness as i32);
            assert_eq!(te.detail, "finished normally");
            assert_eq!(te.usage.len(), 1);
            assert_eq!(te.usage[0].input_tokens, 1500);
        }
        _ => panic!("Expected TurnEnded"),
    }
}

#[test]
fn test_only_the_mcp_factory_prefix_marks_factory_origin() {
    use crate::transcript::TranscriptTranslator;
    use agent_client_protocol::schema::v1::{ToolCall as AcpToolCall, ToolCallId};
    use df_proto::transcript::v1::ToolOrigin;

    let mut translator = TranscriptTranslator::new("turn_1");

    // Tool call with "factory" in title but ordinary tool name must be Harness origin
    let mut call1 = AcpToolCall::new(
        ToolCallId::new("call1"),
        "Read crates/factory-agent/src/lib.rs",
    );
    call1.name = Some("read_file".to_string());
    let ev1 = translator.translate_tool_call(&call1);
    match ev1.kind {
        Some(df_proto::transcript::v1::event::Kind::ToolCall(tc)) => {
            assert_eq!(tc.origin, ToolOrigin::Harness as i32);
        }
        _ => panic!("Expected ToolCall event"),
    }

    // Factory tool must start with mcp__factory__
    let mut call2 = AcpToolCall::new(ToolCallId::new("call2"), "mcp__factory__complete_task");
    call2.name = Some("mcp__factory__complete_task".to_string());
    let ev2 = translator.translate_tool_call(&call2);
    match ev2.kind {
        Some(df_proto::transcript::v1::event::Kind::ToolCall(tc)) => {
            assert_eq!(tc.origin, ToolOrigin::Factory as i32);
        }
        _ => panic!("Expected ToolCall event"),
    }
}
