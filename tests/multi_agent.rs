use agent_sessions::*;
use serde_json::{Value, json};
use std::io::Cursor;

fn import(agent: Agent, v: Value) -> SessionImport {
    import_session_from(
        agent,
        Cursor::new(serde_json::to_vec(&v).unwrap()),
        &ReadOptions::default(),
    )
    .unwrap()
}
fn lines(agent: Agent, rows: &[Value], opts: &ReadOptions) -> SessionImport {
    let text = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    import_session_from(agent, Cursor::new(text), opts).unwrap()
}
fn texts(result: &SessionImport) -> Vec<(Role, &str)> {
    result
        .events
        .iter()
        .filter_map(|e| {
            if let Event::Message(m) = &e.value {
                Some((m.role, m.text.as_str()))
            } else {
                None
            }
        })
        .collect()
}
fn tools(result: &SessionImport) -> Vec<&str> {
    result
        .events
        .iter()
        .filter_map(|e| {
            if let Event::ToolCall(c) = &e.value {
                Some(c.name.as_str())
            } else {
                None
            }
        })
        .collect()
}
fn usages(result: &SessionImport) -> Vec<&Usage> {
    result
        .events
        .iter()
        .filter_map(|e| {
            if let Event::Usage(u) = &e.value {
                Some(u)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn qwen_native_parts_tools_usage_and_whitespace() {
    let result = lines(
        Agent::QwenCode,
        &[
            json!({"type":"user","sessionId":"q","uuid":"u","message":{"role":"user","parts":[{"text":" 中文\n "}]}}),
            json!({"type":"assistant","uuid":"a","message":{"role":"model","parts":[{"text":"first"},{"text":"second"},{"functionCall":{"id":"c","name":"read_file","args":{"path":"demo"}}}]},"model":"qwen-test","usageMetadata":{"promptTokenCount":12,"candidatesTokenCount":3,"cachedContentTokenCount":4,"thoughtsTokenCount":2}}),
            json!({"type":"tool_result","message":{"role":"user","parts":[{"functionResponse":{"id":"c","name":"read_file","response":{"output":"ok","count":2}}}]}}),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::User, " 中文\n "), (Role::Assistant, "first\nsecond")]
    );
    assert_eq!(tools(&result), vec!["read_file"]);
    let u = usages(&result)[0];
    assert_eq!(u.counts.input, Some(12));
    assert_eq!(u.counts.output, Some(3));
    assert_eq!(u.counts.exclusive(u.semantics).unwrap().input, Some(8));
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.output==Some(json!({"output":"ok","count":2})))));
}
#[test]
fn qwen_usage_only_does_not_validate_message_bodies() {
    let result = lines(
        Agent::QwenCode,
        &[json!({"type":"assistant","message":{},"usageMetadata":{"promptTokenCount":8}})],
        &ReadOptions {
            include: EventKinds::USAGE,
            ..Default::default()
        },
    );
    assert_eq!(usages(&result)[0].counts.input, Some(8));
    assert_eq!(usages(&result)[0].counts.output, None);
}
#[test]
fn pi_records_preserve_branch_ids_tools_and_cache_semantics() {
    let result = lines(
        Agent::Pi,
        &[
            json!({"type":"session","id":"pi","cwd":"/synthetic/project"}),
            json!({"type":"model_change","modelId":"model-test"}),
            json!({"type":"message","id":"u","parentId":null,"message":{"role":"user","content":"same","timestamp":1700000000123_i64}}),
            json!({"type":"message","id":"a","parentId":"u","message":{"role":"assistant","content":[{"type":"text","text":"same"},{"type":"toolCall","id":"t","name":"bash","arguments":{"command":"pwd"}}],"usage":{"input":10,"output":5,"cacheRead":4,"cacheWrite":2,"reasoning":1,"totalTokens":21}}}),
            json!({"type":"message","id":"r","message":{"role":"toolResult","toolCallId":"t","isError":false,"content":[{"type":"text","text":"done"}]}}),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(tools(&result), vec!["bash"]);
    assert_eq!(texts(&result).len(), 2);
    let user = result
        .events
        .iter()
        .find(|e| matches!(&e.value,Event::Message(m) if m.role==Role::User))
        .unwrap();
    assert_eq!(user.at.unwrap().timestamp_millis(), 1700000000123);
    assert!(result.events.iter().any(|e| matches!(&e.value, Event::Message(m) if m.role == Role::Assistant && m.parent_id.as_deref() == Some("u"))));
    let u = usages(&result)[0];
    let exclusive = u.counts.exclusive(u.semantics).unwrap();
    assert_eq!(exclusive.input, Some(10));
    assert_eq!(exclusive.output, Some(4));
}
#[test]
fn codebuddy_official_stream_envelopes_share_claude_events() {
    let result = lines(
        Agent::CodeBuddy,
        &[
            json!({"type":"user","session_id":"cb","message":{"role":"user","content":"hello"}}),
            json!({"type":"assistant","message":{"role":"assistant","id":"m","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":8,"output_tokens":2}}}),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::User, "hello"), (Role::Assistant, "hi")]
    );
    assert_eq!(usages(&result)[0].counts.output, Some(2));
}
#[test]
fn copilot_persisted_messages_execution_and_optional_ephemeral_usage() {
    let result = lines(
        Agent::CopilotCli,
        &[
            json!({"id":"s","type":"session.start","data":{"sessionId":"co","context":{"cwd":"/synthetic"},"selectedModel":"test"}}),
            json!({"id":"u","type":"user.message","data":{"content":"help"}}),
            json!({"id":"a","type":"assistant.message","agentId":"sub","data":{"messageId":"m","content":"done","toolRequests":[{"toolCallId":"t","name":"read"}]}}),
            json!({"id":"t","type":"tool.execution_start","data":{"toolCallId":"t","toolName":"read","arguments":{"path":"demo"}}}),
            json!({"id":"r","type":"tool.execution_complete","data":{"toolCallId":"t","success":false,"error":{"message":"missing"}}}),
            json!({"id":"usage","type":"assistant.usage","ephemeral":true,"data":{"model":"test","inputTokens":8,"outputTokens":2}}),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(tools(&result), vec!["read"]);
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(&e.value,Event::Message(m) if m.is_sidechain))
    );
    assert!(result.events.iter().any(
        |e| matches!(&e.value,Event::ToolResult(r) if r.is_error==Some(true)&&r.text=="missing")
    ));
    assert_eq!(usages(&result)[0].semantics, TokenSemantics::Unknown);
}
#[test]
fn gemini_json_document_preserves_tools_and_usage() {
    let result = import(
        Agent::GeminiCli,
        json!({"sessionId":"g","projectHash":"project","messages":[
            {"id":"u","type":"user","content":"request"},
            {"id":"a","type":"gemini","content":[{"text":"answer"}],"toolCalls":[{"id":"c","name":"read","args":{"path":"demo"},"status":"success","result":[{"text":"result"}]}],"tokens":{"input":10,"output":4,"cached":3,"thoughts":2,"total":16}}
        ]}),
    );
    assert!(result.summary.is_supported());
    assert_eq!(tools(&result), vec!["read"]);
    assert_eq!(usages(&result)[0].counts.output, Some(4));
    assert_eq!(
        result.events.last().unwrap().session_id.as_deref(),
        Some("g")
    );
    assert!(
        result
            .events
            .iter()
            .any(|e| e.sources == vec![ImportSource::JsonPointer("/messages/1".into())])
    );
}
#[test]
fn gemini_jsonl_replays_content_tool_patches_reorder_removal_and_rewind() {
    let result = lines(
        Agent::GeminiCli,
        &[
            json!({"sessionId":"g","projectHash":"p"}),
            json!({"id":"u","type":"user","content":"request"}),
            json!({"id":"a","type":"gemini","content":"old","toolCalls":[{"id":"t","name":"read","args":{},"result":null}]}),
            json!({"id":"removed","type":"user","content":"removed"}),
            json!({"$patch":{"updates":[{"id":"a","content":[{"text":"new"}],"toolCalls":[{"id":"t","result":[{"text":"done"}]}]}],"removeIds":["removed"],"orderIds":["u","a"]}}),
            json!({"id":"rewound","type":"gemini","content":"rewound"}),
            json!({"$rewindTo":"a"}),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::User, "request"), (Role::Assistant, "new")]
    );
    let assistant = result
        .events
        .iter()
        .find(|e| matches!(&e.value,Event::Message(m) if m.role==Role::Assistant))
        .unwrap();
    assert_eq!(assistant.sources.len(), 2);
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(&e.value,Event::ToolResult(r) if r.text=="done"))
    );
}
#[test]
fn gemini_checkpoint_replaces_history_and_unknown_patch_is_visible() {
    let result = lines(
        Agent::GeminiCli,
        &[
            json!({"sessionId":"g","projectHash":"p"}),
            json!({"id":"old","type":"user","content":"old"}),
            json!({"$set":{"messages":[{"id":"new","type":"user","content":"new"}],"summary":"summary"}}),
            json!({"$patch":{"id":"new","futureField":true}}),
        ],
        &ReadOptions::default(),
    );
    assert_eq!(texts(&result), vec![(Role::User, "new")]);
    assert!(!result.summary.is_supported());
    assert_eq!(result.summary.unknown_types["gemini:patch:futureField"], 1);
}
#[test]
fn kimi_context_preserves_openai_tools_and_system_prompt() {
    let result = lines(
        Agent::KimiCli,
        &[
            json!({"role":"_system_prompt","content":"system"}),
            json!({"role":"_checkpoint","id":1}),
            json!({"role":"user","content":"request"}),
            json!({"role":"assistant","content":"answer","tool_calls":[{"id":"t","type":"function","function":{"name":"Shell","arguments":"{\"command\":\"pwd\"}"}}]}),
            json!({"role":"tool","tool_call_id":"t","content":"done"}),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(tools(&result), vec!["Shell"]);
    assert_eq!(texts(&result).len(), 3);
}
#[test]
fn kimi_wire_assembles_text_and_argument_chunks_in_original_order() {
    let wire = |kind: &str, payload: Value| json!({"timestamp":1700000000.25,"message":{"type":kind,"payload":payload}});
    let result = lines(
        Agent::KimiCli,
        &[
            json!({"type":"metadata","protocol_version":"1.0"}),
            wire("TurnBegin", json!({"user_input":"request"})),
            wire("StepBegin", json!({"n":1})),
            wire("ContentPart", json!({"type":"text","text":"hello "})),
            wire("ContentPart", json!({"type":"text","text":"world"})),
            wire(
                "ToolCall",
                json!({"type":"function","id":"t","function":{"name":"Shell","arguments":"{"}}),
            ),
            wire(
                "ToolCallPart",
                json!({"arguments_part":"\"command\":\"pwd\"}"}),
            ),
            wire(
                "ToolResult",
                json!({"tool_call_id":"t","return_value":{"is_error":false,"output":"done"}}),
            ),
            wire("TurnEnd", json!({})),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::User, "request"), (Role::Assistant, "hello world")]
    );
    let assistant = result
        .events
        .iter()
        .find(|e| matches!(&e.value,Event::Message(m) if m.role==Role::Assistant))
        .unwrap();
    assert_eq!(assistant.sources.len(), 2);
    assert_eq!(assistant.at.unwrap().timestamp_subsec_millis(), 250);
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolCall(c) if c.arguments==ToolArgs::RawString("{\"command\":\"pwd\"}".into()))));
}
#[test]
fn kimi_code_v2_journal_uses_native_records_and_one_usage_ledger() {
    let usage = json!({"inputOther":8,"output":3,"inputCacheRead":2,"inputCacheCreation":0});
    let result = lines(
        Agent::KimiCli,
        &[
            json!({"type":"metadata","protocol_version":"1.5","created_at":"2026-10-07T00:00:00Z"}),
            json!({"type":"turn.prompt","input":{"text":"request"},"time":1700000000000_i64}),
            json!({"type":"context.append_message","time":1700000000000_i64,"message":{"role":"user","content":[{"type":"text","text":"request"}],"toolCalls":[]}}),
            json!({"type":"llm.request","time":1700000000001_i64,"model":"synthetic-model","provider":"synthetic"}),
            json!({"type":"context.append_loop_event","time":1700000000123_i64,"event":{"type":"content.part","uuid":"part","stepUuid":"step","part":{"type":"text","text":" 中文\n "}}}),
            json!({"type":"context.append_loop_event","event":{"type":"tool.call","uuid":"call","stepUuid":"step","toolCallId":"t","name":"read_file","args":{"path":"fixture.txt"}}}),
            json!({"type":"context.append_loop_event","event":{"type":"tool.result","toolCallId":"t","parentUuid":"call","result":{"output":"native output","isError":false}}}),
            json!({"type":"context.append_loop_event","event":{"type":"step.end","usage":usage}}),
            json!({"type":"usage.record","model":"synthetic-model","usage":usage,"usageScope":"main"}),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::User, "request"), (Role::Assistant, " 中文\n ")]
    );
    assert_eq!(tools(&result), vec!["read_file"]);
    assert_eq!(usages(&result).len(), 1);
    assert_eq!(usages(&result)[0].counts.input, Some(8));
    assert_eq!(usages(&result)[0].counts.cache_read, Some(2));
    let assistant = result
        .events
        .iter()
        .find(|e| matches!(&e.value,Event::Message(m) if m.role==Role::Assistant))
        .unwrap();
    assert_eq!(assistant.at.unwrap().timestamp_millis(), 1700000000123);
    assert_eq!(assistant.message_id.as_deref(), Some("step"));
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.call_id.as_deref()==Some("t")&&r.text=="native output"&&r.is_error==Some(false))));
    let unknown = lines(
        Agent::KimiCli,
        &[json!({"type":"context.append_loop_event","event":{"type":"future.event"}})],
        &ReadOptions::default(),
    );
    assert!(!unknown.summary.is_supported());
}
#[test]
fn kimi_code_v2_usage_projection_skips_bodies_and_preserves_flat_calls() {
    let only_usage = lines(
        Agent::KimiCli,
        &[
            json!({"type":"context.append_message","message":{}}),
            json!({"type":"context.append_loop_event","event":{"type":"content.part","part":false}}),
            json!({"type":"usage.record","usage":{"inputOther":2,"output":1}}),
        ],
        &ReadOptions {
            include: EventKinds::USAGE,
            ..Default::default()
        },
    );
    assert!(only_usage.summary.is_supported());
    assert_eq!(usages(&only_usage).len(), 1);
    let body = lines(
        Agent::KimiCli,
        &[
            json!({"type":"context.append_message","message":{"role":"assistant","content":[],"toolCalls":[{"type":"function","id":"c","name":"read","arguments":"{raw}"}]}}),
            json!({"type":"context.append_message","message":{"role":"tool","toolCallId":"c","content":[{"type":"text","text":"done"}],"isError":true}}),
        ],
        &ReadOptions::default(),
    );
    assert!(body.summary.is_supported());
    assert!(body.events.iter().any(|e|matches!(&e.value,Event::ToolCall(c) if c.arguments==ToolArgs::RawString("{raw}".into()))));
    assert!(body.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.call_id.as_deref()==Some("c")&&r.text=="done"&&r.is_error==Some(true))));
}
#[test]
fn cline_and_roo_native_api_history() {
    for agent in [Agent::Cline, Agent::RooCode] {
        let result = import(
            agent,
            json!([
                {"role":"user","content":"request","ts":1700000000000_i64},
                {"role":"assistant","content":[{"type":"text","text":"answer"},{"type":"tool_use","id":"t","name":"read_file","input":{"path":"demo"}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"done"}]}
            ]),
        );
        assert!(result.summary.is_supported());
        assert_eq!(tools(&result), vec!["read_file"]);
        assert!(
            result
                .events
                .iter()
                .any(|e| matches!(&e.value,Event::ToolResult(r) if r.text=="done"))
        );
    }
}
#[test]
fn opencode_export_nested_parts_and_native_timestamps() {
    let result = import(
        Agent::OpenCode,
        json!({"info":{"id":"s","directory":"/synthetic"},"messages":[
            {"info":{"id":"u","role":"user","time":{"created":1700000000123_i64}},"parts":[{"type":"text","text":"request"}]},
            {"info":{"id":"a","role":"assistant","time":{"created":1700000000456_i64},"modelID":"test","tokens":{"input":10,"output":4,"reasoning":1,"cache":{"read":2,"write":3}}},"parts":[{"type":"text","text":"answer"},{"type":"tool","callID":"t","tool":"read","state":{"status":"completed","input":{"path":"demo"},"output":"done"}}]}
        ]}),
    );
    assert!(result.summary.is_supported());
    assert_eq!(tools(&result), vec!["read"]);
    assert_eq!(usages(&result)[0].counts.cache_read, Some(2));
    assert_eq!(
        result
            .events
            .iter()
            .find(|e| e.message_id.as_deref() == Some("u"))
            .unwrap()
            .at
            .unwrap()
            .timestamp_millis(),
        1700000000123
    );
}
#[test]
fn goose_official_export_preserves_native_tool_results_and_usage() {
    let result = import(
        Agent::Goose,
        json!({"id":"g","working_dir":"/synthetic","conversation":[
            {"id":"u","role":"user","created":1700000000,"content":[{"type":"text","text":"request"}],"metadata":{}},
            {"id":"a","role":"assistant","created":1700000001,"content":[{"type":"text","text":"answer"},{"type":"toolRequest","id":"t","toolCall":{"status":"success","value":{"name":"developer__shell","arguments":{"command":"pwd"}}}}],"metadata":{"usage":{"inputTokens":10,"outputTokens":2}}},
            {"id":"r","role":"user","created":1700000002,"content":[{"type":"toolResponse","id":"t","toolResult":{"status":"success","value":{"content":[{"type":"text","text":"done"}],"isError":false}}}],"metadata":{}}
        ]}),
    );
    assert!(result.summary.is_supported());
    assert_eq!(tools(&result), vec!["developer__shell"]);
    assert_eq!(usages(&result)[0].counts.output, Some(2));
}
#[test]
fn continue_history_reads_camel_case_tools_and_usage() {
    let result = import(
        Agent::Continue,
        json!({"sessionId":"c","workspaceDirectory":"/synthetic","history":[
            {"message":{"role":"user","content":"request"}},
            {"message":{"role":"assistant","content":"answer","toolCalls":[{"id":"t","type":"function","function":{"name":"read","arguments":"{}"}}],"usage":{"promptTokens":10,"completionTokens":2}}},
            {"message":{"role":"tool","toolCallId":"t","content":"done"}}
        ]}),
    );
    assert!(result.summary.is_supported());
    assert_eq!(tools(&result), vec!["read"]);
    assert_eq!(usages(&result)[0].counts.output, Some(2));
}
#[test]
fn unknown_content_prevents_supported_claim_without_dropping_known_text() {
    let result = import(
        Agent::GeminiCli,
        json!({"sessionId":"g","messages":[{"id":"u","type":"user","content":[{"text":"known"},{"type":"new-media","data":"synthetic"}]}]}),
    );
    assert!(result.summary.is_complete());
    assert!(!result.summary.is_supported());
    assert_eq!(texts(&result), vec![(Role::User, "known")]);
}
#[test]
fn native_counters_reject_negative_fractional_and_wrong_types() {
    for value in [json!(-1), json!(1.5), json!("10")] {
        let v = json!({"sessionId":"g","messages":[{"id":"a","type":"gemini","content":"answer","tokens":{"input":value}}]});
        assert!(
            import_session_from(
                Agent::GeminiCli,
                Cursor::new(v.to_string()),
                &ReadOptions::default()
            )
            .is_err()
        );
    }
}
#[test]
fn snapshot_import_preserves_size_tail_and_io_error_contracts() {
    let opts = ReadOptions {
        max_file_bytes: Some(4),
        ..Default::default()
    };
    assert!(matches!(
        import_session_from(
            Agent::GeminiCli,
            Cursor::new("{\"sessionId\":\"g\"}"),
            &opts
        ),
        Err(ImportError::Stream(StreamError::TooLarge { .. }))
    ));
    let opts = ReadOptions {
        max_line_bytes: Some(4),
        ..Default::default()
    };
    assert!(matches!(
        import_session_from(
            Agent::GeminiCli,
            Cursor::new("{\"sessionId\":\"g\"}"),
            &opts
        ),
        Err(ImportError::Stream(StreamError::Line {
            kind: LineErrorKind::TooLong,
            ..
        }))
    ));
    let data = "{\"sessionId\":\"g\",\"projectHash\":\"p\"}\n{\"id\":\"u\",\"type\":\"user\",\"content\":\"valid\"}\n{\"id\":";
    let result = import_session_from(
        Agent::GeminiCli,
        Cursor::new(data),
        &ReadOptions {
            tail: TailMode::AllowIncomplete,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.summary.status, ReadStatus::IncompleteTail);
    assert!(!result.summary.is_supported());
    assert_eq!(texts(&result), vec![(Role::User, "valid")]);
    assert!(
        import_session_from(Agent::GeminiCli, Cursor::new(data), &ReadOptions::default()).is_err()
    );
    let opts = ReadOptions {
        stop_at_byte: Some(100),
        ..Default::default()
    };
    assert!(matches!(
        import_session_from(Agent::Cline, Cursor::new("[]"), &opts),
        Err(ImportError::Stream(StreamError::SnapshotTruncated { .. }))
    ));
}
#[test]
fn document_agents_reject_wrong_streaming_api_and_unimplemented_history() {
    assert!(read_from(Agent::GeminiCli, Cursor::new("{}"), &ReadOptions::default()).is_err());
    assert!(history_entry(Agent::Pi, &json!({}), true).is_err());
    assert!(Roots::from_env_for(Agent::Pi).is_err());
}
#[test]
fn codex_completed_items_select_one_content_source_and_keep_tools() {
    let rows = [
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"same"}]}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"u","content":[{"type":"text","text":"request"}]}}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"a","content":[{"type":"Text","text":"same"}]}}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution","id":"t","command":["pwd"],"aggregated_output":"done","exit_code":0}}}),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"McpToolCall","id":"mcp","tool":"read","arguments":{},"result":{"content":[{"type":"text","text":"result"}],"isError":false}}}}),
    ];
    let result = lines(
        Agent::Codex,
        &rows,
        &ReadOptions {
            codex_content: CodexContentMode::CompletedItems,
            ..Default::default()
        },
    );
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::User, "request"), (Role::Assistant, "same")]
    );
    assert_eq!(tools(&result), vec!["exec_command", "read"]);
    let legacy = lines(Agent::Codex, &rows, &ReadOptions::default());
    assert_eq!(texts(&legacy), vec![(Role::Assistant, "same")]);
}
#[test]
fn codex_auto_uses_native_history_mode_without_duplicate_content() {
    let raw = json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"raw"}]}});
    let completed = json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"a","content":[{"type":"Text","text":"completed"}]}}});
    let result = lines(
        Agent::Codex,
        &[
            json!({"type":"session_meta","payload":{"id":"page","history_mode":"paginated"}}),
            raw.clone(),
            completed.clone(),
            json!({"type":"session_meta","payload":{"id":"old"}}),
            raw.clone(),
            completed.clone(),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::Assistant, "completed"), (Role::Assistant, "raw")]
    );
    let forced = lines(
        Agent::Codex,
        &[
            json!({"type":"session_meta","payload":{"id":"page","history_mode":"paginated"}}),
            raw,
            completed,
        ],
        &ReadOptions {
            codex_content: CodexContentMode::ResponseItems,
            ..Default::default()
        },
    );
    assert_eq!(texts(&forced), vec![(Role::Assistant, "raw")]);
    let unknown = lines(
        Agent::Codex,
        &[json!({"type":"session_meta","payload":{"id":"future","history_mode":"future"}})],
        &ReadOptions::default(),
    );
    assert!(!unknown.summary.is_supported());
}
#[test]
fn codex_completed_dynamic_tools_and_hook_fragments_keep_native_text() {
    let result = lines(
        Agent::Codex,
        &[
            json!({"type":"session_meta","payload":{"id":"s","history_mode":"paginated"}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"DynamicToolCall","id":"tool","tool":"lookup","arguments":{"key":"demo"},"status":"completed","content_items":[{"type":"inputText","text":"found"}],"success":true}}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"HookPrompt","id":"hook","fragments":[{"text":"retry","hookRunId":"h"}]}}}),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"FunctionCallOutput","id":"output","name":"lookup","output":[{"type":"input_text","text":"value"}]}}}),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(texts(&result), vec![(Role::System, "retry")]);
    assert_eq!(tools(&result), vec!["lookup"]);
    let outputs: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::ToolResult(r) => Some(r.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(outputs, vec!["found", "value"]);
}
#[test]
fn codex_completed_file_search_collaboration_and_extension_scope() {
    let items = [
        json!({"type":"FileChange","id":"patch","changes":{"demo.rs":{"type":"update","unified_diff":"+example","move_path":null}},"status":"failed","stdout":"failed output"}),
        json!({"type":"Extension","kind":"web.search","id":"search","query":"native query","action":null,"results":[{"title":"synthetic result"}]}),
        json!({"type":"Extension","kind":"clock.sleep","id":"sleep","durationMs":1000}),
        json!({"type":"CollabAgentToolCall","id":"child","tool":"spawn_agent","status":"completed","prompt":"synthetic prompt","receiver_thread_ids":["child-thread"]}),
        json!({"type":"SubAgentActivity","id":"activity","kind":"completed","agent_thread_id":"child-thread","agent_path":"/child"}),
    ];
    let mut rows =
        vec![json!({"type":"session_meta","payload":{"id":"s","history_mode":"paginated"}})];
    rows.extend(
        items.iter().map(
            |item| json!({"type":"event_msg","payload":{"type":"item_completed","item":item}}),
        ),
    );
    let result = lines(Agent::Codex, &rows, &ReadOptions::default());
    assert!(result.summary.is_supported());
    assert_eq!(
        tools(&result),
        vec!["apply_patch", "web_search", "clock.sleep", "spawn_agent"]
    );
    let results: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::ToolResult(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(results[0].output, Some(items[0].clone()));
    assert_eq!(results[0].text, "failed output");
    assert_eq!(results[0].is_error, Some(true));
    assert_eq!(results[1].output, Some(items[1].clone()));
    assert_eq!(results[2].output, Some(items[2].clone()));
    assert_eq!(results[3].output, Some(items[3].clone()));
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolCall(c) if c.name=="apply_patch"&&c.arguments==ToolArgs::Missing)));
    let unknown = lines(
        Agent::Codex,
        &[
            rows[0].clone(),
            json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"Extension","kind":"future.new","id":"new"}}}),
        ],
        &ReadOptions::default(),
    );
    assert!(!unknown.summary.is_supported());
}
#[test]
fn codex_early_envelopes_work_without_weakening_current_required_fields() {
    let result = lines(
        Agent::Codex,
        &[
            json!({"type":"user_message","content":"request"}),
            json!({"type":"response_item","payload":{"content":[{"type":"text","text":"answer"}]}}),
        ],
        &ReadOptions::default(),
    );
    assert_eq!(
        texts(&result),
        vec![(Role::User, "request"), (Role::Assistant, "answer")]
    );
    assert!(
        import_session_from(
            Agent::Codex,
            Cursor::new(
                json!({"type":"response_item","payload":{"type":"message","content":[]}})
                    .to_string()
            ),
            &ReadOptions::default()
        )
        .is_err()
    );
}
#[test]
fn native_discovery_excludes_indices_and_duplicate_kimi_context() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "wire.jsonl",
        "context.jsonl",
        "session-test.json",
        "logs.json",
        "api_conversation_history.json",
        "ui_messages.json",
        "events.jsonl",
    ] {
        std::fs::write(dir.path().join(name), "{}").unwrap();
    }
    let filter = DiscoverFilter::default();
    assert_eq!(
        discover_directory(Agent::KimiCli, dir.path(), &filter)
            .files
            .len(),
        1
    );
    assert_eq!(
        discover_directory(Agent::GeminiCli, dir.path(), &filter)
            .files
            .len(),
        1
    );
    assert_eq!(
        discover_directory(Agent::Cline, dir.path(), &filter)
            .files
            .len(),
        1
    );
    assert_eq!(
        discover_directory(Agent::CopilotCli, dir.path(), &filter)
            .files
            .len(),
        1
    );
}
#[test]
fn sqlite_opencode_read_only_snapshot_and_native_row_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("opencode.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE session(id TEXT,title TEXT,directory TEXT,parent_id TEXT,time_created INTEGER); CREATE TABLE message(id TEXT,session_id TEXT,time_created INTEGER,data TEXT); CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,time_created INTEGER,data TEXT);").unwrap();
    conn.execute(
        "INSERT INTO session VALUES('s','test','/synthetic',NULL,1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message VALUES('m','s',1,?1)",
        [json!({"role":"user","time":{"created":1700000000123_i64}}).to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part VALUES('p','m','s',1,?1)",
        [json!({"type":"text","text":"request"}).to_string()],
    )
    .unwrap();
    drop(conn);
    let before = std::fs::read(&path).unwrap();
    let opts = ReadOptions::default();
    assert_eq!(
        list_database_sessions(Agent::OpenCode, &path, &opts).unwrap()[0].id,
        "s"
    );
    let result = import_database(Agent::OpenCode, &path, "s", &opts).unwrap();
    assert!(result.summary.is_supported());
    assert_eq!(texts(&result), vec![(Role::User, "request")]);
    let message = result
        .events
        .iter()
        .find(|e| matches!(e.value, Event::Message(_)))
        .unwrap();
    assert_eq!(message.sources.len(), 2);
    assert_eq!(before, std::fs::read(&path).unwrap());
    assert!(import_database(Agent::OpenCode, &path, "missing", &opts).is_err());
    let absent = dir.path().join("missing.db");
    assert!(import_database(Agent::OpenCode, &absent, "s", &opts).is_err());
    assert!(!absent.exists());
    assert!(
        import_database(
            Agent::OpenCode,
            &path,
            "s",
            &ReadOptions {
                max_line_bytes: Some(2),
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn sqlite_cursor_respects_header_order_and_flags_unparsed_tools() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.vscdb");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE cursorDiskKV(key TEXT PRIMARY KEY,value BLOB)")
        .unwrap();
    for (key, value) in [
        (
            "composerData:c",
            json!({"fullConversationHeadersOnly":[{"bubbleId":"u","type":1},{"bubbleId":"a","type":2}]}),
        ),
        (
            "bubbleId:c:u",
            json!({"bubbleId":"u","type":1,"text":"request","createdAt":1700000000123_i64}),
        ),
        (
            "bubbleId:c:a",
            json!({"bubbleId":"a","type":2,"text":"answer","toolFormerData":{"synthetic":true}}),
        ),
    ] {
        c.execute(
            "INSERT INTO cursorDiskKV VALUES(?1,?2)",
            rusqlite::params![key, value.to_string()],
        )
        .unwrap();
    }
    drop(c);
    let result = import_database(Agent::Cursor, &path, "c", &ReadOptions::default()).unwrap();
    assert_eq!(
        texts(&result),
        vec![(Role::User, "request"), (Role::Assistant, "answer")]
    );
    assert!(!result.summary.is_supported());
    assert_eq!(result.summary.unknown_types["cursor:tool-payload"], 1);
}
#[test]
fn sqlite_goose_native_schema_and_optional_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sessions.db");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE sessions(id TEXT,name TEXT,working_dir TEXT,created_at TEXT);CREATE TABLE messages(id INTEGER,message_id TEXT,session_id TEXT,role TEXT,created_timestamp INTEGER,content_json TEXT,metadata_json TEXT);INSERT INTO sessions VALUES('s','test','/synthetic','2026-01-01');").unwrap();
    c.execute(
        "INSERT INTO messages VALUES(1,'m','s','user',1700000000,?1,NULL)",
        [json!([{"type":"text","text":"request"}]).to_string()],
    )
    .unwrap();
    drop(c);
    let result = import_database(Agent::Goose, &path, "s", &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported());
    assert_eq!(texts(&result), vec![(Role::User, "request")]);
}

#[test]
fn iflow_shipped_native_envelopes_and_structured_tool_outputs() {
    let result = lines(
        Agent::IFlow,
        &[
            json!({"uuid":"u","parentUuid":null,"sessionId":"i","type":"user","message":{"role":"user","content":"request"},"cwd":"/synthetic"}),
            json!({"uuid":"a","parentUuid":"u","sessionId":"i","type":"assistant","message":{"id":"m","role":"assistant","model":"test","content":[{"type":"text","text":"answer"},{"type":"tool_use","id":"t","name":"read","input":{"path":"demo"}}],"usage":{"input_tokens":8,"output_tokens":2}}}),
            json!({"uuid":"r","parentUuid":"a","sessionId":"i","type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":{"files":["demo"],"count":1}}]}}),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(tools(&result), vec!["read"]);
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.output==Some(json!({"files":["demo"],"count":1})))));
    assert_eq!(usages(&result)[0].counts.output, Some(2));
}
#[test]
fn cursor_observed_iso_timestamp_and_tool_former_payload() {
    // The public native shape was observed locally; all values here are synthetic.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.vscdb");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE cursorDiskKV(key TEXT PRIMARY KEY,value BLOB)")
        .unwrap();
    for (key, value) in [
        (
            "composerData:c",
            json!({"fullConversationHeadersOnly":[{"bubbleId":"a","type":2}]}),
        ),
        (
            "bubbleId:c:a",
            json!({"bubbleId":"a","type":2,"text":"answer","createdAt":"2026-01-01T00:00:00.123Z","toolFormerData":{"toolCallId":"t","status":"completed","name":"read_file","rawArgs":"{\"path\":\"demo\"}","result":"done"}}),
        ),
    ] {
        c.execute(
            "INSERT INTO cursorDiskKV VALUES(?1,?2)",
            rusqlite::params![key, value.to_string()],
        )
        .unwrap();
    }
    drop(c);
    let result = import_database(Agent::Cursor, &path, "c", &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported());
    assert_eq!(tools(&result), vec!["read_file"]);
    assert_eq!(
        result
            .events
            .iter()
            .find(|e| matches!(e.value, Event::Message(_)))
            .unwrap()
            .at
            .unwrap()
            .timestamp_subsec_millis(),
        123
    );
}

fn grok_update(method: &str, update: Value) -> Value {
    json!({"timestamp":1700000000,"method":method,"params":{"sessionId":"g","_meta":{"eventId":"e","agentTimestampMs":1700000000123_i64},"update":update}})
}
#[test]
fn grok_acp_folds_tools_keeps_native_output_and_one_turn_ledger() {
    let rows = [
        grok_update(
            "session/update",
            json!({"sessionUpdate":"user_message_chunk","content":{"type":"text","text":" 中文\n "}}),
        ),
        grok_update(
            "session/update",
            json!({"sessionUpdate":"tool_call","toolCallId":"c","rawInput":{"path":"old"},"_meta":{"x.ai/tool":{"name":"read_file"}}}),
        ),
        grok_update(
            "session/update",
            json!({"sessionUpdate":"tool_call_update","toolCallId":"c","rawInput":{"path":"new"}}),
        ),
        grok_update(
            "session/update",
            json!({"sessionUpdate":"tool_call_update","toolCallId":"c","status":"completed","rawOutput":{"type":"ReadFile","FileContent":{"content":"fixture\n中文"}}}),
        ),
        grok_update(
            "session/update",
            json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"done"}}),
        ),
        grok_update(
            "_x.ai/session/update",
            json!({"sessionUpdate":"turn_completed","prompt_id":"p","usage":{"inputTokens":12,"outputTokens":3,"cachedReadTokens":5,"reasoningTokens":1,"modelUsage":{"test":{"inputTokens":12,"outputTokens":3}}}}),
        ),
    ];
    let result = lines(Agent::Grok, &rows, &ReadOptions::default());
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::User, " 中文\n "), (Role::Assistant, "done")]
    );
    assert_eq!(tools(&result), vec!["read_file"]);
    let call = result
        .events
        .iter()
        .find(|e| matches!(e.value, Event::ToolCall(_)))
        .unwrap();
    assert_eq!(call.sources.len(), 3);
    assert!(
        matches!(&call.value,Event::ToolCall(c) if c.arguments==ToolArgs::Json(json!({"path":"new"})))
    );
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.text=="fixture\n中文" && r.output==Some(json!({"type":"ReadFile","FileContent":{"content":"fixture\n中文"}})))));
    assert_eq!(usages(&result).len(), 1);
    assert_eq!(usages(&result)[0].counts.input, Some(12));
    assert!(
        usages(&result)[0]
            .counts
            .exclusive(usages(&result)[0].semantics)
            .is_none()
    );
    assert_eq!(
        result.events[0].at.unwrap().timestamp_millis(),
        1700000000123
    );
}
#[test]
fn grok_partial_tools_unknown_extensions_and_projection_are_explicit() {
    let rows = [
        grok_update(
            "session/update",
            json!({"sessionUpdate":"tool_call_update","toolCallId":null,"rawOutput":"excluded"}),
        ),
        grok_update(
            "session/update",
            json!({"sessionUpdate":"tool_call","toolCallId":"c","title":"not a native tool name"}),
        ),
        grok_update(
            "_x.ai/session/update",
            json!({"sessionUpdate":"future_extension"}),
        ),
        grok_update(
            "session/update",
            json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"private reasoning"}}),
        ),
        grok_update(
            "session/update",
            json!({"sessionUpdate":"agent_message_chunk","content":{}}),
        ),
        grok_update(
            "_x.ai/session/update",
            json!({"sessionUpdate":"turn_completed","usage":{"inputTokens":4}}),
        ),
    ];
    let result = lines(
        Agent::Grok,
        &rows,
        &ReadOptions {
            include: EventKinds::USAGE,
            ..Default::default()
        },
    );
    assert_eq!(usages(&result)[0].counts.input, Some(4));
    assert_eq!(usages(&result)[0].counts.output, None);
    assert!(!result.summary.is_supported());
    assert!(
        result
            .summary
            .unknown_types
            .contains_key("grok:future_extension")
    );
    let result = lines(Agent::Grok, &rows[1..2], &ReadOptions::default());
    assert!(tools(&result).is_empty());
    assert!(
        result
            .summary
            .unknown_types
            .contains_key("grok:tool-without-native-name")
    );
}
#[test]
fn grok_duplicate_terminal_update_produces_one_result_and_failed_status() {
    let rows = [
        grok_update(
            "session/update",
            json!({"sessionUpdate":"tool_call","toolCallId":"c","_meta":{"x.ai/tool":{"name":"bash"}}}),
        ),
        grok_update(
            "session/update",
            json!({"sessionUpdate":"tool_call_update","toolCallId":"c","status":"completed","rawOutput":{"output_for_prompt":"old"}}),
        ),
        grok_update(
            "session/update",
            json!({"sessionUpdate":"tool_call_update","toolCallId":"c","status":"failed","rawOutput":{"output_for_prompt":"error"}}),
        ),
    ];
    let result = lines(Agent::Grok, &rows, &ReadOptions::default());
    let outputs: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let Event::ToolResult(r) = &e.value {
                Some(r)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].is_error, Some(true));
    assert_eq!(outputs[0].text, "error");
}

#[test]
fn grok_acp_wrapped_tool_content_preserves_native_array_and_media_diagnostic() {
    let content = json!([{"type":"content","content":{"type":"text","text":"中文结果"}},{"type":"content","content":{"type":"image","data":"synthetic","mimeType":"image/png"}}]);
    let result = lines(
        Agent::Grok,
        &[
            grok_update(
                "session/update",
                json!({"sessionUpdate":"tool_call","toolCallId":"c","_meta":{"x.ai/tool":{"name":"read"}}}),
            ),
            grok_update(
                "session/update",
                json!({"sessionUpdate":"tool_call_update","toolCallId":"c","status":"completed","content":content}),
            ),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert!(result.summary.ignored_types.contains_key("content:image"));
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.text=="中文结果" && r.output==Some(content.clone()))));
}

#[test]
fn cline_cli_native_snapshot_has_tools_metrics_and_message_identity() {
    let result = import(
        Agent::ClineCli,
        json!({"version":1,"sessionId":"cli","messages":[
            {"id":"u","role":"user","ts":1700000000000_i64,"content":[{"type":"text","text":"question"}]},
            {"id":"a","role":"assistant","content":[{"type":"text","text":"reply"},{"type":"tool_use","id":"t","name":"read","input":{"path":"demo"}}],"metrics":{"inputTokens":8,"outputTokens":3,"cacheReadTokens":2}},
            {"id":"r","role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"result"}]}
        ]}),
    );
    assert!(result.summary.is_supported());
    assert_eq!(tools(&result), vec!["read"]);
    assert_eq!(usages(&result)[0].counts.input, Some(8));
    assert_eq!(usages(&result)[0].counts.cache_write, None);
    assert!(
        result
            .events
            .iter()
            .any(|e| e.message_id.as_deref() == Some("a"))
    );
    assert!(
        result
            .events
            .iter()
            .all(|e| e.session_id.as_deref() == Some("cli"))
    );
}
#[test]
fn hermes_native_json_preserves_openai_tool_arguments_and_plain_json_text() {
    let result = import(
        Agent::Hermes,
        json!({"session_id":"h","model":"test","messages":[
            {"role":"user","content":"{\"text\":\"do not reinterpret\"}"},
            {"role":"assistant","content":null,"tool_calls":[{"id":"c","type":"function","function":{"name":"read","arguments":"{\"path\":\"demo\"}"}}]},
            {"role":"tool","content":"done","tool_call_id":"c"}
        ]}),
    );
    assert!(result.summary.is_supported());
    assert_eq!(tools(&result), vec!["read"]);
    assert_eq!(texts(&result)[0].1, "{\"text\":\"do not reinterpret\"}");
    assert!(usages(&result).is_empty());
}
#[test]
fn hermes_sqlite_structured_sentinel_and_budget_are_respected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.db");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE sessions(id TEXT,title TEXT,cwd TEXT,model TEXT,parent_session_id TEXT,started_at REAL);CREATE TABLE messages(id INTEGER,session_id TEXT,role TEXT,content TEXT,tool_call_id TEXT,tool_calls TEXT,timestamp REAL);INSERT INTO sessions VALUES('s','test','/tmp','m',NULL,1700000000);").unwrap();
    c.execute("INSERT INTO messages VALUES(1,'s','user',?1,NULL,NULL,1700000000.25)",["\0json:[{\"type\":\"text\",\"text\":\"中文\"},{\"type\":\"image_url\",\"image_url\":{\"url\":\"example\"}}]"]).unwrap();
    c.execute("INSERT INTO messages VALUES(2,'s','assistant',NULL,NULL,?1,1700000001)",["[{\"id\":\"c\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"{}\"}}]"]).unwrap();
    drop(c);
    let result = import_database(Agent::Hermes, &path, "s", &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported());
    assert_eq!(texts(&result)[0].1, "中文");
    assert_eq!(tools(&result), vec!["read"]);
    assert_eq!(
        result
            .events
            .iter()
            .find(|e| matches!(e.value, Event::Message(_)))
            .unwrap()
            .at
            .unwrap()
            .timestamp_millis(),
        1700000000250
    );
    assert_eq!(
        list_database_sessions(Agent::Hermes, &path, &ReadOptions::default())
            .unwrap()
            .len(),
        1
    );
    assert!(
        import_database(
            Agent::Hermes,
            &path,
            "s",
            &ReadOptions {
                max_line_bytes: Some(8),
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn cline_cli_internal_tool_results_remain_structured_without_false_unknown() {
    let output = json!([{"query":{"path":"demo"},"result":"中文\nresult","success":true},{"query":{},"result":null,"success":false,"error":"failed"}]);
    let result = import(
        Agent::ClineCli,
        json!({"sessionId":"cli","messages":[{"role":"user","content":[{"type":"tool_result","tool_use_id":"c","is_error":true,"content":output}]}]}),
    );
    assert!(result.summary.is_supported());
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.output==Some(output.clone()) && r.text=="中文\nresult" && r.is_error==Some(true))));
}

#[test]
fn workbuddy_native_calls_results_and_millisecond_identity() {
    let output = json!({"type":"text","text":" 中文\n ","extra":[1,2]});
    let result = lines(
        Agent::WorkBuddy,
        &[
            json!({"type":"message","id":"u","sessionId":"s","timestamp":1700000000123_i64,"role":"user","content":[{"type":"input_text","text":"问"},{"type":"image_blob_ref","ref":"image"}]}),
            json!({"type":"function_call","callId":"c","name":"read","arguments":"{\"path\":\"test\"}"}),
            json!({"type":"function_call_result","callId":"c","status":"completed","output":output}),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert_eq!(texts(&result), vec![(Role::User, "问")]);
    assert_eq!(tools(&result), vec!["read"]);
    let m = result
        .events
        .iter()
        .find(|e| matches!(e.value, Event::Message(_)))
        .unwrap();
    assert_eq!(m.at.unwrap().timestamp_millis(), 1700000000123);
    assert_eq!(m.session_id.as_deref(), Some("s"));
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolCall(c) if c.arguments==ToolArgs::RawString("{\"path\":\"test\"}".into()))));
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.output==Some(output.clone())&&r.text==" 中文\n "&&r.is_error==Some(false))));
}
#[test]
fn qoder_old_task_and_project_records_use_native_roles() {
    let result = lines(
        Agent::Qoder,
        &[
            json!({"role":"user","message":{"content":" old\n "}}),
            json!({"type":"runtime-config","sessionId":"s","model":"test","timestamp":1700000000123_i64}),
            json!({"type":"assistant","uuid":"a","message":{"content":[{"type":"text","text":"new"},{"type":"tool_use","id":"c","name":"read","input":{"path":"p"}}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"c","content":"result"}]}}),
        ],
        &ReadOptions::default(),
    );
    assert!(result.summary.is_supported());
    assert!(texts(&result).contains(&(Role::User, " old\n ")));
    assert!(texts(&result).contains(&(Role::Assistant, "new")));
    assert_eq!(tools(&result), vec!["read"]);
}
#[test]
fn grokbot_replica_preserves_vendor_direction_and_unknown_tools() {
    let result = import(
        Agent::GrokBot,
        json!({"schemaVersion":1,"value":{"entries":[
            {"kind":"message","id":"u","role":"user","content":"问","timestampMs":1700000000123_i64},
            {"kind":"send-message","id":"a","message":{"type":"text","content":"答"}},
            {"kind":"message","id":"b","role":"user","fromAgent":"other","content":"转发"},
            {"kind":"tool-call","name":"unverified"}
        ]}}),
    );
    assert_eq!(
        texts(&result),
        vec![
            (Role::User, "问"),
            (Role::Assistant, "答"),
            (Role::Assistant, "转发")
        ]
    );
    assert!(!result.summary.is_supported());
    assert_eq!(result.summary.unknown_types["grokbot:tool-call"], 1);
}
#[test]
fn zed_native_variants_keep_tools_results_and_one_request_ledger() {
    let output = json!({"tool_use_id":"c","tool_name":"read","is_error":false,"content":[{"Text":"中文"},"\n "],"output":{"value":7}});
    let result = import(
        Agent::Zed,
        json!({"messages":[
        {"User":{"id":"u","content":[{"Text":"问"},{"Mention":{"uri":"test","content":" context "}},{"Image":{"source":"test"}}]}},
        {"Agent":{"content":[{"Text":"答"},{"Thinking":{"text":"not prose"}},{"ToolUse":{"id":"c","name":"read","input":{"type":"json","value":{"path":"p"}},"raw_input":"{}","is_input_complete":true}}],"tool_results":{"c":output}}},
        "Resume",{"Compaction":{"Summary":"not new user prose"}}
    ],"request_token_usage":{"u":{"input_tokens":12,"output_tokens":3,"cache_read_input_tokens":4}},"cumulative_token_usage":{"input_tokens":999}}),
    );
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::User, "问\n context "), (Role::Assistant, "答")]
    );
    assert_eq!(tools(&result), vec!["read"]);
    assert_eq!(usages(&result).len(), 1);
    assert_eq!(usages(&result)[0].counts.input, Some(12));
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolCall(c) if c.arguments==ToolArgs::Json(json!({"path":"p"})))));
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.output==Some(output.clone())&&r.text=="中文\n ")));
}
#[test]
fn antigravity_trajectory_keeps_results_deduplicates_calls_and_checks_total() {
    let tool = json!({"id":"c","name":"read","argumentsJson":"{\"path\":\"p\"}"});
    let mut native = json!({"numTotalSteps":4,"trajectory":{"cascadeId":"s","steps":[
        {"type":"CORTEX_STEP_TYPE_USER_INPUT","userInput":{"items":[{"text":"问"}]}},
        {"type":"CORTEX_STEP_TYPE_PLANNER_RESPONSE","plannerResponse":{"response":"答","toolCalls":[tool]},"metadata":{"createdAt":"2026-10-07T00:00:00Z","modelUsage":{"model":"test","inputTokens":"12","outputTokens":"3","responseId":"r"}}},
        {"type":"CORTEX_STEP_TYPE_VIEW_FILE","status":"CORTEX_STEP_STATUS_DONE","metadata":{"toolCall":tool},"viewFile":{"content":"中文","absolutePathUri":"p"}},
        {"type":"CORTEX_STEP_TYPE_CHECKPOINT","checkpoint":{},"metadata":{"modelUsage":{"inputTokens":"12","responseId":"r"}}}
    ]}});
    let result = import(Agent::Antigravity, native.clone());
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::User, "问"), (Role::Assistant, "答")]
    );
    assert_eq!(tools(&result), vec!["read"]);
    assert_eq!(usages(&result).len(), 1);
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.output.as_ref().unwrap()["viewFile"]["content"]=="中文")));
    native["numTotalSteps"] = json!(5);
    assert!(
        import_session_from(
            Agent::Antigravity,
            Cursor::new(native.to_string()),
            &ReadOptions::default()
        )
        .is_err()
    );
}

// Hand-built wire fixtures use field numbers from the vendor definitions, not
// serializer output from the implementation under test.
fn pb_varint(mut n: u64) -> Vec<u8> {
    let mut v = Vec::new();
    while n >= 128 {
        v.push((n as u8) | 128);
        n >>= 7;
    }
    v.push(n as u8);
    v
}
fn pb_bytes(tag: u64, data: &[u8]) -> Vec<u8> {
    let mut v = pb_varint(tag << 3 | 2);
    v.extend(pb_varint(data.len() as u64));
    v.extend(data);
    v
}
fn pb_join(fields: &[(u64, Vec<u8>)]) -> Vec<u8> {
    fields.iter().flat_map(|(n, v)| pb_bytes(*n, v)).collect()
}
#[test]
fn cursor_cli_checkpoint_uses_references_not_blob_order_and_keeps_native_tools() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE blobs(id TEXT PRIMARY KEY,data BLOB);CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT);").unwrap();
    let hex = |v: &[u8]| v.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let meta = json!({"agentId":"s","latestRootBlobId":"00","blobEncryptionKey":"synthetic-secret","lastUsedModel":"test"});
    let encoded = hex(meta.to_string().as_bytes());
    db.execute("INSERT INTO meta VALUES('0',?1)", [encoded])
        .unwrap();
    let read = pb_join(&[
        (1, pb_bytes(1, b"p")),
        (2, pb_bytes(1, &pb_bytes(10, &[6]))),
    ]);
    let tool = pb_join(&[(8, read)]); // no native tool ID: do not invent one
    let root = pb_join(&[(1, vec![5]), (8, vec![1])]);
    let turn = pb_bytes(1, &pb_join(&[(1, vec![2]), (2, vec![3]), (2, vec![4])]));
    let rows = [
        (9, pb_bytes(1, b"unreachable")),
        (6, " 中文\n ".as_bytes().to_vec()),
        (4, pb_bytes(2, &tool)),
        (3, pb_bytes(1, &pb_bytes(1, "答".as_bytes()))),
        (2, pb_bytes(1, "问".as_bytes())),
        (1, turn),
        (0, root),
        (
            5,
            serde_json::to_vec(&json!({"role":"system","content":"context"})).unwrap(),
        ),
    ];
    for (id, data) in rows {
        db.execute(
            "INSERT INTO blobs VALUES(?1,?2)",
            rusqlite::params![hex(&[id]), data],
        )
        .unwrap();
    }
    drop(db);
    let result = import_database(Agent::CursorCli, &path, "s", &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![
            (Role::System, "context"),
            (Role::User, "问"),
            (Role::Assistant, "答")
        ]
    );
    assert_eq!(tools(&result), vec!["readToolCall"]);
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.text==" 中文\n "&&r.output==Some(json!({"success":{"contentBlobId":"Bg=="}})))),"{:?}",result.events.iter().filter(|e|matches!(e.value,Event::ToolResult(_))).collect::<Vec<_>>());
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("synthetic-secret")
    );
    assert!(import_database(Agent::CursorCli, &path, "wrong", &ReadOptions::default()).is_err());
    let db = rusqlite::Connection::open(&path).unwrap();
    let mut root: Vec<u8> = db
        .query_row("SELECT data FROM blobs WHERE id='00'", [], |r| r.get(0))
        .unwrap();
    root.extend(pb_bytes(99, b"future variant"));
    db.execute("UPDATE blobs SET data=?1 WHERE id='00'", [root])
        .unwrap();
    drop(db);
    let future = import_database(Agent::CursorCli, &path, "s", &ReadOptions::default()).unwrap();
    assert!(!future.summary.is_supported());
    assert_eq!(texts(&future), texts(&result));
    assert_eq!(
        future.summary.unknown_types["protobuf:agent.v1.ConversationStateStructure:99"],
        1
    );

    assert!(
        import_database(
            Agent::CursorCli,
            &path,
            "s",
            &ReadOptions {
                max_line_bytes: Some(1),
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn warp_native_tasks_keep_all_message_kinds_and_error_contract() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("warp.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE agent_conversations(id INTEGER,conversation_id TEXT,summary TEXT,last_modified_at INTEGER);CREATE TABLE agent_tasks(id INTEGER,conversation_id TEXT,task BLOB);INSERT INTO agent_conversations VALUES(1,'s','test',0);").unwrap();
    let user = pb_join(&[(1, b"u".to_vec()), (2, pb_bytes(1, " 问\n ".as_bytes()))]);
    let call = pb_join(&[(1, b"c".to_vec()), (2, pb_bytes(1, b"echo test"))]);
    let tool = pb_join(&[(1, b"t".to_vec()), (4, call)]);
    let output = pb_join(&[
        (1, b"r".to_vec()),
        (
            5,
            pb_join(&[(1, b"c".to_vec()), (4, pb_bytes(1, b"result"))]),
        ),
    ]);
    let answer = pb_join(&[(1, b"a".to_vec()), (3, pb_bytes(1, "答".as_bytes()))]);
    let task = pb_join(&[
        (1, b"task".to_vec()),
        (5, user),
        (5, tool),
        (5, output),
        (5, answer),
    ]);
    db.execute("INSERT INTO agent_tasks VALUES(1,'s',?1)", [task])
        .unwrap();
    drop(db);
    let result = import_database(Agent::Warp, &path, "s", &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::User, " 问\n "), (Role::Assistant, "答")]
    );
    assert_eq!(tools(&result), vec!["runShellCommand"]);
    assert!(result.events.iter().any(|e|matches!(&e.value,Event::ToolResult(r) if r.call_id.as_deref()==Some("c")&&r.output.as_ref().unwrap()["server"]["serializedResult"]=="result")));
    assert!(import_database(Agent::Warp, &path, "missing", &ReadOptions::default()).is_err());
    assert!(
        import_database(
            Agent::Warp,
            &path,
            "s",
            &ReadOptions {
                max_line_bytes: Some(1),
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn zed_compressed_database_bounds_decompressed_bytes_and_uses_row_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("threads.db");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE threads(id TEXT,summary TEXT,updated_at INTEGER,parent_id TEXT,data_type TEXT,data BLOB);").unwrap();
    let native = json!({"messages":[{"User":{"id":"u","content":[{"Text":"中文"}]}},{"Agent":{"content":[{"Text":"x".repeat(5000)}],"tool_results":{}}}]});
    let data = zstd::stream::encode_all(native.to_string().as_bytes(), 3).unwrap();
    assert!(data.len() < 256);
    db.execute(
        "INSERT INTO threads VALUES('s','test',0,'parent','zstd',?1)",
        [data],
    )
    .unwrap();
    drop(db);
    let result = import_database(Agent::Zed, &path, "s", &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported());
    assert_eq!(texts(&result)[0], (Role::User, "中文"));
    assert!(result.events.iter().all(|e|e.session_id.as_deref()==Some("s")&&matches!(&e.sources[0],ImportSource::DatabaseRow{table,key} if table=="threads"&&key=="s")));
    assert!(
        result
            .events
            .iter()
            .all(|e| !matches!(&e.value,Event::Message(m) if !m.is_sidechain))
    );
    assert!(
        import_database(
            Agent::Zed,
            &path,
            "s",
            &ReadOptions {
                max_line_bytes: Some(256),
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn zcode_sequence_and_model_timeline_do_not_drop_message_parts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE session(id TEXT,directory TEXT,parent_id TEXT,title TEXT,time_created INTEGER);CREATE TABLE message(id TEXT,session_id TEXT,sequence INTEGER,data TEXT);CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,sequence INTEGER,data TEXT);INSERT INTO session VALUES('s','test',NULL,'test',0);").unwrap();
    db.execute(
        "INSERT INTO message VALUES('z','s',1,?1)",
        [json!({"role":"user"}).to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO message VALUES('a','s',2,?1)",
        [json!({"role":"assistant"}).to_string()],
    )
    .unwrap();
    for (id, m, seq, data) in [
        ("z", "z", 1, json!({"type":"text","text":"问"})),
        ("y", "a", 2, json!({"type":"text","text":" second "})),
        ("x", "a", 1, json!({"type":"text","text":"first"})),
        (
            "t",
            "a",
            0,
            json!({"type":"timeline","timelineType":"model-change","toModel":{"modelID":"test"}}),
        ),
    ] {
        db.execute(
            "INSERT INTO part VALUES(?1,?2,'s',?3,?4)",
            rusqlite::params![id, m, seq, data.to_string()],
        )
        .unwrap();
    }
    drop(db);
    let result = import_database(Agent::ZCode, &path, "s", &ReadOptions::default()).unwrap();
    assert!(result.summary.is_supported());
    assert_eq!(
        texts(&result),
        vec![(Role::User, "问"), (Role::Assistant, "first\n second ")]
    );
    assert_eq!(result.summary.ignored_types["zcode:model-timeline"], 1);
}
#[test]
fn new_native_discovery_excludes_trace_files_and_non_transcript_blobs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("logs")).unwrap();
    std::fs::write(dir.path().join("logs/trace.jsonl"), "bad log text").unwrap();
    std::fs::write(dir.path().join("session.jsonl"), "{}\n").unwrap();
    for agent in [Agent::WorkBuddy, Agent::Qoder] {
        let result = discover_directory(agent, dir.path(), &DiscoverFilter::default());
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path.file_name().unwrap(), "session.jsonl");
    }
    // Hand-written unpadded base32 fixture for the public cache-key convention.
    fn name(s: &str) -> String {
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut out = String::new();
        let mut bits = 0_u32;
        let mut n = 0;
        for b in s.bytes() {
            bits = bits << 8 | u32::from(b);
            n += 8;
            while n >= 5 {
                n -= 5;
                out.push(alphabet[((bits >> n) & 31) as usize] as char);
            }
            bits &= (1 << n) - 1;
        }
        if n > 0 {
            out.push(alphabet[((bits << (5 - n)) & 31) as usize] as char);
        }
        out.to_ascii_lowercase() + ".blob"
    }
    std::fs::write(
        dir.path()
            .join(name("sand.client.slice.test.transcript.replicas.session")),
        "{}",
    )
    .unwrap();
    std::fs::write(dir.path().join(name("sand.client.slice.settings")), "{}").unwrap();
    assert_eq!(
        discover_directory(Agent::GrokBot, dir.path(), &DiscoverFilter::default())
            .files
            .len(),
        1
    );
}

#[test]
fn new_native_malformed_roles_bodies_and_variants_do_not_look_supported() {
    let bad = import_session_from(
        Agent::Qoder,
        Cursor::new("{\"role\":\"user\",\"message\":\"bad\"}\n"),
        &ReadOptions::default(),
    );
    assert!(bad.is_err());
    let unknown = import(
        Agent::GrokBot,
        json!({"schemaVersion":1,"value":{"entries":[{"kind":"message","role":"new-role","content":"unverified"},{"kind":"send-message","message":{"type":"new-text","content":"unverified"}}]}}),
    );
    assert!(!unknown.summary.is_supported());
    assert!(texts(&unknown).is_empty());
    assert_eq!(unknown.summary.unknown_types.len(), 2);
}
