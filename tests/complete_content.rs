use agent_sessions::*;
use serde_json::{Value, json};
use std::io::Cursor;

fn lines(agent: Agent, rows: &[Value], opts: &ReadOptions) -> SessionImport {
    let text = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    import_session_from(agent, Cursor::new(text), opts).unwrap()
}
fn document(agent: Agent, value: Value, opts: &ReadOptions) -> SessionImport {
    import_session_from(agent, Cursor::new(value.to_string()), opts).unwrap()
}
fn messages(r: &SessionImport) -> Vec<&str> {
    r.events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Message(m) if !m.text.is_empty() => Some(m.text.as_str()),
            _ => None,
        })
        .collect()
}
fn has(r: &SessionImport, kind: &str, data: &Value) -> bool {
    r.events.iter().any(|e| matches!(&e.value, Event::Content(c) if c.kind == kind && &c.data == data && !e.sources.is_empty()))
}

#[test]
fn api_and_stream_families_preserve_native_media_signatures_and_projection_identity() {
    let image = json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"fixture-image"}});
    let thinking = json!({"type":"thinking","thinking":"中文思考","signature":"opaque-signature"});
    let content = json!([{"type":"text","text":"正文"}, image, thinking]);
    for agent in [
        Agent::ClaudeCode,
        Agent::CodeBuddy,
        Agent::IFlow,
        Agent::Qoder,
    ] {
        let row = json!({"type":"assistant","uuid":"record","message":{"id":"message","role":"assistant","content":content}});
        let r = lines(agent, std::slice::from_ref(&row), &ReadOptions::default());
        assert!(r.summary.is_supported(), "{agent:?}: {:?}", r.summary);
        assert_eq!(messages(&r), ["正文"], "{agent:?}");
        assert!(has(&r, "image", &image), "{agent:?}");
        assert!(has(&r, "thinking", &thinking), "{agent:?}");
        let only = lines(
            agent,
            &[row],
            &ReadOptions {
                include: EventKinds::CONTENT,
                ..Default::default()
            },
        );
        assert_eq!(only.events.len(), 2, "{agent:?}");
        for e in &only.events {
            assert!(r.events.contains(e));
            let encoded = serde_json::to_value(&e.value).unwrap();
            assert_eq!(serde_json::from_value::<Event>(encoded).unwrap(), e.value);
        }
        let indexes: Vec<_> = r
            .events
            .iter()
            .filter_map(|e| match &e.sources[0] {
                ImportSource::Record(l) => Some(l.event_index),
                _ => None,
            })
            .collect();
        // Snapshot replay sources identify contributing physical records;
        // streaming locations additionally identify each projected event.
        if agent != Agent::CodeBuddy {
            assert!(indexes.windows(2).all(|v| v[0] < v[1]));
        }
    }
    for agent in [
        Agent::Cline,
        Agent::RooCode,
        Agent::ClineCli,
        Agent::Hermes,
        Agent::Continue,
        Agent::Goose,
    ] {
        let m = json!({"role":"assistant","content":content});
        let v = match agent {
            Agent::Cline | Agent::RooCode => json!([m]),
            Agent::ClineCli => json!({"messages":[m]}),
            Agent::Hermes => json!({"session_id":"h","messages":[m]}),
            Agent::Continue => json!({"history":[{"message":m}]}),
            Agent::Goose => json!({"conversation":[m]}),
            _ => unreachable!(),
        };
        let r = document(agent, v, &ReadOptions::default());
        assert!(r.summary.is_supported(), "{agent:?}: {:?}", r.summary);
        assert_eq!(messages(&r), ["正文"]);
        assert!(has(&r, "image", &image), "{agent:?}");
        assert!(has(&r, "thinking", &thinking), "{agent:?}");
    }
}

#[test]
fn native_content_shapes_are_retained_without_turning_them_into_answer_text() {
    let image = json!({"inlineData":{"mimeType":"image/png","data":"fixture"}});
    let thought = json!({"text":"内部推理","thought":true,"thoughtSignature":"signed"});
    let q = lines(
        Agent::QwenCode,
        &[json!({"type":"assistant","message":{"parts":[image,thought,{"text":"答复"}]}})],
        &ReadOptions::default(),
    );
    assert!(q.summary.is_supported());
    assert!(has(&q, "media-or-signature", &image));
    assert!(has(&q, "thinking", &thought));
    assert_eq!(messages(&q), ["答复"]);
    let thoughts =
        json!([{"subject":"推理","description":"原生思考","timestamp":"2026-10-08T00:00:00Z"}]);
    let g = document(
        Agent::GeminiCli,
        json!({"sessionId":"g","messages":[{"type":"gemini","content":[image],"thoughts":thoughts}]}),
        &ReadOptions::default(),
    );
    assert!(g.summary.is_supported());
    assert!(has(&g, "thoughts", &thoughts));
    assert!(has(&g, "media-or-signature", &image));
    let reason = json!({"type":"reasoning","summary":[{"type":"summary_text","text":"想法"}],"encrypted_content":"opaque"});
    let c = lines(
        Agent::Codex,
        &[json!({"type":"response_item","payload":reason})],
        &ReadOptions::default(),
    );
    assert!(c.summary.is_supported());
    assert!(has(&c, "reasoning", &reason));
    assert!(messages(&c).is_empty());
    let wb = json!({"type":"reasoning","id":"r","summary":[{"type":"summary_text","text":"WorkBuddy 思考"}]});
    let w = lines(
        Agent::WorkBuddy,
        std::slice::from_ref(&wb),
        &ReadOptions::default(),
    );
    assert!(w.summary.is_supported());
    assert!(has(&w, "reasoning", &wb));
    let data = json!({"reasoningId":"r","content":"Copilot 思考"});
    let cp = lines(
        Agent::CopilotCli,
        &[json!({"type":"assistant.reasoning","data":data})],
        &ReadOptions::default(),
    );
    assert!(cp.summary.is_supported());
    assert!(has(&cp, "assistant.reasoning", &data));
    let pi = lines(
        Agent::Pi,
        &[
            json!({"type":"message","id":"p","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Pi 思考","thinkingSignature":"sig"}]}}),
        ],
        &ReadOptions::default(),
    );
    assert!(pi.summary.is_supported());
    assert!(
        pi.events
            .iter()
            .any(|e| matches!(&e.value,Event::Content(c) if c.data["thinkingSignature"]=="sig"))
    );
    let oc = document(
        Agent::OpenCode,
        json!({"info":{"id":"o"},"messages":[{"info":{"role":"assistant"},"parts":[{"type":"reasoning","text":"OpenCode 思考","time":{"start":1,"end":2}},{"type":"file","mime":"image/png","url":"data:image/png;base64,fixture"}]}]}),
        &ReadOptions::default(),
    );
    assert!(oc.summary.is_supported());
    assert_eq!(
        oc.events
            .iter()
            .filter(|e| matches!(e.value, Event::Content(_)))
            .count(),
        2
    );
}

fn append(text: &str, id: &str, origin: Option<Value>) -> Value {
    let mut m = json!({"role":"user","id":id,"content":[{"type":"text","text":text}]});
    if let Some(origin) = origin {
        m["origin"] = origin;
    }
    json!({"type":"context.append_message","message":m})
}
fn loop_event(e: Value) -> Value {
    json!({"type":"context.append_loop_event","event":e})
}

#[test]
fn kimi_v2_undo_clear_compaction_and_usage_have_distinct_semantics() {
    let rows = vec![
        append("保留", "u1", None),
        append(
            "注入",
            "i",
            Some(json!({"kind":"injection","ownerPromptId":"u2"})),
        ),
        append("撤回", "u2", None),
        loop_event(
            json!({"type":"content.part","stepUuid":"s","part":{"type":"text","text":"撤回答复"}}),
        ),
        json!({"type":"usage.record","usage":{"inputOther":7,"output":3}}),
        json!({"type":"context.undo","count":1}),
    ];
    let r = lines(Agent::KimiCli, &rows, &ReadOptions::default());
    assert!(r.summary.is_supported(), "{:?}", r.summary);
    assert_eq!(messages(&r), ["保留"]);
    assert_eq!(
        r.events
            .iter()
            .filter(|e| matches!(e.value, Event::Usage(_)))
            .count(),
        1
    );
    let mut rows = rows;
    rows.push(json!({"type":"context.clear"}));
    rows.push(append("新开始", "u3", None));
    assert_eq!(
        messages(&lines(Agent::KimiCli, &rows, &ReadOptions::default())),
        ["新开始"]
    );
    let mut rows = vec![
        append("旧提问", "u1", None),
        append("保留尾部", "u2", None),
        json!({"type":"context.apply_compaction","summary":"压缩摘要","compactedCount":1}),
    ];
    assert_eq!(
        messages(&lines(Agent::KimiCli, &rows, &ReadOptions::default())),
        ["压缩摘要", "保留尾部"]
    );
    rows.push(json!({"type":"context.undo","count":2}));
    assert_eq!(
        messages(&lines(Agent::KimiCli, &rows, &ReadOptions::default())),
        ["压缩摘要", "保留尾部"]
    );
}

#[test]
fn kimi_v2_merges_parts_defers_user_injections_and_never_invents_tool_output() {
    let rows = [
        append("问题", "u", None),
        loop_event(json!({"type":"step.begin","uuid":"s"})),
        loop_event(
            json!({"type":"content.part","stepUuid":"s","part":{"type":"text","text":"第一段"}}),
        ),
        loop_event(
            json!({"type":"content.part","stepUuid":"s","part":{"type":"text","text":"第二段"}}),
        ),
        loop_event(
            json!({"type":"tool.call","stepUuid":"s","toolCallId":"c","name":"read","args":{"path":"测试.txt"}}),
        ),
        append("延迟注入", "i", Some(json!({"kind":"injection"}))),
        loop_event(
            json!({"type":"tool.result","toolCallId":"c","result":{"output":[{"type":"text","text":"结果"}],"isError":false}}),
        ),
        loop_event(json!({"type":"step.end","finishReason":"stop"})),
    ];
    let r = lines(Agent::KimiCli, &rows, &ReadOptions::default());
    assert!(r.summary.is_supported());
    assert_eq!(messages(&r), ["问题", "第一段\n第二段", "延迟注入"]);
    let assistant = r
        .events
        .iter()
        .find(|e| matches!(&e.value,Event::Message(m) if m.role==Role::Assistant))
        .unwrap();
    assert_eq!(assistant.sources.len(), 4);
    assert_eq!(assistant.message_id.as_deref(), Some("s"));
    let r = lines(Agent::KimiCli, &rows[..6], &ReadOptions::default());
    assert!(
        !r.events
            .iter()
            .any(|e| matches!(e.value, Event::ToolResult(_)))
    );
}

#[test]
fn native_desktop_and_acp_content_keeps_attachment_and_control_payloads() {
    let image = json!({"Image":{"url":"file:///fixture.png","mime_type":"image/png"}});
    let think = json!({"Thinking":{"text":"Zed 思考","signature":"sig"}});
    let z = document(
        Agent::Zed,
        json!({"messages":[{"User":{"content":[image]}},{"Agent":{"content":[think]}},"Resume",{"Compaction":{"Summary":"上下文摘要"}}]}),
        &ReadOptions::default(),
    );
    assert!(z.summary.is_supported());
    assert!(has(&z, "Image", &image));
    assert!(has(&z, "Thinking", &think));
    assert!(has(&z, "Resume", &json!("Resume")));
    let attachment =
        json!({"type":"attachment","file_path":"/fixture.txt","file_name":"fixture.txt"});
    let b = document(
        Agent::GrokBot,
        json!({"value":{"entries":[{"kind":"send-message","message":attachment},{"kind":"user-attachment","id":"f","file_path":"/input.png"},{"kind":"tool-call","id":"t","name":"read","status":"completed","summary":"读取完毕"}]}}),
        &ReadOptions::default(),
    );
    assert!(b.summary.is_supported());
    assert!(has(&b, "attachment", &attachment));
    assert!(b.events.iter().any(|e|matches!(&e.value,Event::ToolCall(c) if c.id.as_deref()==Some("t") && c.arguments==ToolArgs::Missing)));
    assert!(
        !b.events
            .iter()
            .any(|e| matches!(e.value, Event::ToolResult(_)))
    );
    let thought =
        json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Grok 思考"}});
    let g = lines(
        Agent::Grok,
        &[json!({"method":"session/update","params":{"sessionId":"g","update":thought}})],
        &ReadOptions::default(),
    );
    assert!(g.summary.is_supported());
    assert!(has(&g, "agent_thought_chunk", &thought));
    let a = document(
        Agent::Antigravity,
        json!({"steps":[{"plannerResponse":{"response":"回复","thinking":"Antigravity 思考"}}]}),
        &ReadOptions::default(),
    );
    assert!(a.summary.is_supported());
    assert!(has(&a, "thinking", &json!("Antigravity 思考")));
}

#[test]
fn content_exclusion_is_reported_and_unknown_future_variants_still_fail_support() {
    let row = json!({"role":"assistant","content":[{"type":"thinking","thinking":"reason"},{"type":"future-media","payload":"unknown"}]});
    let r = document(
        Agent::Cline,
        json!([row]),
        &ReadOptions {
            include: EventKinds::MESSAGE,
            ..Default::default()
        },
    );
    assert!(!r.summary.is_supported());
    assert_eq!(r.summary.ignored_types["content:thinking"], 1);
    assert_eq!(r.summary.unknown_types["content:future-media"], 1);
    let r = lines(
        Agent::KimiCli,
        &[
            json!({"type":"context.append_loop_event","event":{}}),
            json!({"type":"usage.record","usage":{"output":4}}),
        ],
        &ReadOptions {
            include: EventKinds::USAGE,
            ..Default::default()
        },
    );
    assert_eq!(r.events.len(), 1);
}

#[test]
fn pi_snapshot_selects_current_leaf_applies_edits_and_compaction_without_erasing_usage() {
    let mut rows = vec![
        json!({"type":"session","version":3,"id":"s"}),
        json!({"type":"message","id":"u","parentId":null,"message":{"role":"user","content":"原问题"}}),
        json!({"type":"message","id":"old","parentId":"u","message":{"role":"assistant","content":"废弃分支","usage":{"input":10,"output":2}}}),
        json!({"type":"message","id":"new","parentId":"u","message":{"role":"assistant","content":"当前分支","usage":{"input":11,"output":3}}}),
        json!({"type":"context_edit","id":"edit","parentId":"new","targetId":"u","replacement":{"content":"替换问题"}}),
    ];
    let r = lines(Agent::Pi, &rows, &ReadOptions::default());
    assert!(r.summary.is_supported());
    assert_eq!(messages(&r), ["替换问题", "当前分支"]);
    assert_eq!(
        r.events
            .iter()
            .filter(|e| matches!(e.value, Event::Usage(_)))
            .count(),
        2
    );
    assert!(r.events.iter().any(
        |e| matches!(&e.value,Event::Message(m) if m.text=="替换问题") && e.sources.len() == 2
    ));
    rows.push(json!({"type":"compaction","id":"compact","parentId":"edit","summary":"摘要","firstKeptEntryId":"new"}));
    let r = lines(Agent::Pi, &rows, &ReadOptions::default());
    assert_eq!(messages(&r), ["摘要", "当前分支"]);
    rows.push(json!({"type":"context_edit","id":"delete","parentId":"compact","targetId":"new","replacement":null}));
    assert_eq!(
        messages(&lines(Agent::Pi, &rows, &ReadOptions::default())),
        ["摘要"]
    );
    let cyclic = [
        json!({"type":"message","id":"x","parentId":"x","message":{"role":"user","content":"bad"}}),
    ];
    let text = cyclic[0].to_string() + "\n";
    assert!(import_session_from(Agent::Pi, Cursor::new(text), &ReadOptions::default()).is_err());
}

#[test]
fn kimi_modern_compaction_retains_user_prompts_and_enforces_native_unicode_budgets() {
    let rows = [
        append("早期问题", "u", None),
        loop_event(
            json!({"type":"content.part","stepUuid":"s","part":{"type":"text","text":"旧回答"}}),
        ),
        append("最新问题", "u2", None),
        json!({"type":"context.apply_compaction","summary":"压缩摘要","compactedCount":3,"keptUserMessageCount":2}),
    ];
    let r = lines(Agent::KimiCli, &rows, &ReadOptions::default());
    assert!(r.summary.is_supported());
    assert_eq!(messages(&r), ["早期问题", "最新问题", "压缩摘要"]);
    let long = "中🙂".repeat(15_000);
    let r = lines(
        Agent::KimiCli,
        &[
            append(&long, "u", None),
            json!({"type":"context.apply_compaction","summary":"摘要","compactedCount":1,"keptUserMessageCount":1}),
        ],
        &ReadOptions::default(),
    );
    let texts = messages(&r);
    assert_eq!(texts.len(), 3);
    assert_eq!(texts[0].chars().count(), 2000);
    assert_eq!(texts[1].chars().count(), 18_000);
    assert_eq!(texts[2], "摘要");
}

#[test]
fn kimi_subagent_wire_keeps_split_call_assembly_parent_and_native_record_sources() {
    let event = |e| json!({"timestamp":1700000000.0,"message":{"type":"SubagentEvent","payload":{"agent_id":"child","parent_tool_call_id":"parent","event":e}}});
    let rows = [
        event(json!({"type":"ContentPart","payload":{"type":"text","text":"子代理"}})),
        event(
            json!({"type":"ToolCall","payload":{"id":"c","function":{"name":"read","arguments":"{"}}}),
        ),
        event(json!({"type":"ToolCallPart","payload":{"arguments_part":"\"path\":\"fixture\"}"}})),
        event(
            json!({"type":"ToolResult","payload":{"tool_call_id":"c","return_value":{"output":"结果","is_error":false}}}),
        ),
    ];
    let r = lines(Agent::KimiCli, &rows, &ReadOptions::default());
    assert!(r.summary.is_supported(), "{:?}", r.summary);
    assert_eq!(messages(&r), ["子代理"]);
    assert!(r.events.iter().any(|e|matches!(&e.value,Event::Message(m) if m.is_sidechain && m.parent_id.as_deref()==Some("parent"))));
    assert!(r.events.iter().any(|e|matches!(&e.value,Event::ToolCall(c) if c.arguments==ToolArgs::RawString("{\"path\":\"fixture\"}".into())) && e.sources.len()==2));
}

#[test]
fn claude_and_codex_multimodal_tool_outputs_keep_complete_payload_without_duplicate_locations() {
    let output = json!([{"type":"text","text":"工具结果"},{"type":"image","source":{"type":"base64","data":"fixture","media_type":"image/png"}}]);
    for (agent, row) in [
        (
            Agent::ClaudeCode,
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"c","content":output}]}}),
        ),
        (
            Agent::Codex,
            json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"c","output":output}}),
        ),
    ] {
        let r = lines(agent, &[row], &ReadOptions::default());
        assert!(r.summary.is_supported());
        assert!(r.events.iter().any(|e|matches!(&e.value,Event::ToolResult(t) if t.text=="工具结果" && t.output.as_ref()==Some(&output))));
        let keys: Vec<_> = r
            .events
            .iter()
            .map(|e| serde_json::to_string(&e.sources).unwrap())
            .collect();
        let set: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(keys.len(), set.len());
    }
}

#[test]
fn hermes_sqlite_restores_reasoning_native_codex_items_and_checks_combined_row_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("CREATE TABLE sessions(id TEXT,title TEXT,cwd TEXT,model TEXT,parent_session_id TEXT,started_at REAL);CREATE TABLE messages(id INTEGER,session_id TEXT,role TEXT,content TEXT,tool_call_id TEXT,tool_calls TEXT,timestamp REAL,reasoning TEXT,reasoning_content TEXT,reasoning_details TEXT,codex_reasoning_items TEXT,codex_message_items TEXT);INSERT INTO sessions VALUES('s','fixture',NULL,NULL,NULL,0);").unwrap();
    let native = json!([{"type":"reasoning","encrypted_content":"opaque","summary":[]}]);
    c.execute(
        "INSERT INTO messages VALUES(1,'s','assistant','回复',NULL,NULL,5,'思考','',?1,?2,NULL)",
        rusqlite::params![
            json!([{"type":"reasoning.text","text":"推理细节"}]).to_string(),
            native.to_string()
        ],
    )
    .unwrap();
    c.execute(
        "INSERT INTO messages VALUES(2,'s','user','稍后提问',NULL,NULL,1,NULL,NULL,NULL,NULL,NULL)",
        [],
    )
    .unwrap();
    drop(c);
    let r = import_database(Agent::Hermes, &path, "s", &ReadOptions::default()).unwrap();
    assert!(r.summary.is_supported());
    assert_eq!(messages(&r), ["回复", "稍后提问"]);
    assert!(has(&r, "reasoning", &json!("思考")));
    assert!(has(&r, "reasoning_content", &json!("")));
    assert!(has(&r, "codex_reasoning_items", &native));
    assert!(
        import_database(
            Agent::Hermes,
            &path,
            "s",
            &ReadOptions {
                max_line_bytes: Some(30),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        import_database(
            Agent::Hermes,
            &path,
            "s",
            &ReadOptions {
                include: EventKinds::MESSAGE,
                max_line_bytes: Some(30),
                ..Default::default()
            }
        )
        .is_ok()
    );
}

#[test]
fn grok_explicit_chat_snapshot_preserves_raw_tools_and_reasoning_without_double_ledger() {
    let native = json!({"type":"reasoning","id":"r","summary":[],"encrypted_content":"opaque"});
    let rows = [
        json!({"type":"system","content":"系统"}),
        json!({"type":"user","content":[{"type":"text","text":"问题"}]}),
        native.clone(),
        json!({"type":"assistant","content":"调用","model_id":"test","tool_calls":[{"id":"c","type":"function","function":{"name":"read","arguments":"{}"}}]}),
        json!({"type":"tool_result","tool_call_id":"c","content":"结果"}),
    ];
    let r = lines(Agent::Grok, &rows, &ReadOptions::default());
    assert!(r.summary.is_supported());
    assert_eq!(messages(&r), ["系统", "问题", "调用"]);
    assert!(has(
        &r,
        "reasoning",
        &json!({"type":"reasoning","id":"r","summary":[],"encrypted_content":"opaque"})
    ));
    assert!(r.events.iter().any(
        |e| matches!(&e.value,Event::ToolCall(c) if c.arguments==ToolArgs::RawString("{}".into()))
    ));
    assert!(!r.events.iter().any(|e| matches!(e.value, Event::Usage(_))));
}

#[test]
fn codebuddy_native_sdk_flat_records_wrappers_forks_and_clear_keep_one_physical_ledger() {
    let rows = [
        json!({"type":"message","id":"u","role":"user","content":[{"type":"input_text","text":"问题"}],"timestamp":1700000000123_i64}),
        json!({"type":"message","id":"a","parentId":"u","role":"assistant","content":[{"type":"output_text","text":"旧回答"}],"providerData":{"messageId":"m1","usage":{"inputTokens":7,"outputTokens":2}}}),
        json!({"type":"resend-fork-notice","id":"rewind","parentId":"u"}),
        json!({"type":"session-meta","id":"meta","meta":{"multitaskMode":true}}),
        json!({"type":"message","id":"b","parentId":"u","role":"assistant","content":[{"type":"output_text","text":"新回答"}],"providerData":{"messageId":"m2","usage":{"inputTokens":8,"outputTokens":3}}}),
        json!({"type":"function_call","id":"call","parentId":"b","callId":"c","name":"read","arguments":"{\"path\":\"fixture\"}"}),
        json!({"type":"function_call_result","id":"result","parentId":"call","callId":"c","status":"completed","output":[{"type":"input_text","text":"结果"},{"type":"input_image","image":"blob:fixture"}]}),
    ];
    let r = lines(Agent::CodeBuddy, &rows, &ReadOptions::default());
    assert!(r.summary.is_supported(), "{:?}", r.summary);
    assert_eq!(messages(&r), ["问题", "新回答"]);
    assert_eq!(
        r.events
            .iter()
            .filter(|e| matches!(e.value, Event::Usage(_)))
            .count(),
        2
    );
    assert!(r.events.iter().any(|e|matches!(&e.value,Event::ToolResult(t) if t.text=="结果" && t.output.as_ref().unwrap()[1]["image"]=="blob:fixture")));
    let wrapped: Vec<_> = rows
        .iter()
        .map(|r| json!({"type":r["type"],"uuid":r["id"],"payload":r}))
        .collect();
    let w = lines(Agent::CodeBuddy, &wrapped, &ReadOptions::default());
    assert_eq!(messages(&w), messages(&r));
    let pending = lines(Agent::CodeBuddy, &rows[..4], &ReadOptions::default());
    assert_eq!(messages(&pending), ["问题"]);
    let mut clear = rows.to_vec();
    clear.push(json!({"type":"message","id":"clear","role":"system","content":"会话分隔","providerData":{"isSessionSeparator":true}}));
    clear.push(json!({"type":"message","id":"new","role":"user","content":"新的提问"}));
    assert_eq!(
        messages(&lines(Agent::CodeBuddy, &clear, &ReadOptions::default())),
        ["新的提问"]
    );
}

#[test]
fn opencode_multifile_storage_matches_export_and_retains_each_file_source() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for name in ["session/project", "message/s", "part/m1", "part/m2"] {
        std::fs::create_dir_all(root.join(name)).unwrap();
    }
    let info = json!({"id":"s","directory":"/fixture"});
    let m1 = json!({"id":"m1","role":"user","sessionID":"s"});
    let m2 = json!({"id":"m2","role":"assistant","sessionID":"s","tokens":{"input":5,"output":2}});
    let p1 = json!({"id":"p1","messageID":"m1","sessionID":"s","type":"text","text":"问题"});
    let p2 = json!({"id":"p2","messageID":"m2","sessionID":"s","type":"text","text":"答复"});
    let p3 = json!({"id":"p3","messageID":"m2","sessionID":"s","type":"reasoning","text":"思考"});
    for (name, value) in [
        ("session/project/s.json", &info),
        ("message/s/z.json", &m1),
        ("message/s/a.json", &m2),
        ("part/m1/z.json", &p1),
        ("part/m2/z.json", &p2),
        ("part/m2/a.json", &p3),
    ] {
        std::fs::write(root.join(name), value.to_string()).unwrap();
    }
    let path = root.join("session/project/s.json");
    let r = import_session(Agent::OpenCode, &path, &ReadOptions::default()).unwrap();
    let exported = document(
        Agent::OpenCode,
        json!({"info":info,"messages":[{"info":m1,"parts":[p1]},{"info":m2,"parts":[p2,p3]}]}),
        &ReadOptions::default(),
    );
    assert!(r.summary.is_supported());
    assert_eq!(
        r.events.iter().map(|e| &e.value).collect::<Vec<_>>(),
        exported.events.iter().map(|e| &e.value).collect::<Vec<_>>()
    );
    assert!(r.events.iter().all(|e| {
        e.sources
            .iter()
            .all(|s| matches!(s, ImportSource::File { .. }))
    }));
    assert!(r.summary.bytes_read > 0);
    assert_eq!(r.summary.last_complete_byte, 0);
    assert!(
        import_session(
            Agent::OpenCode,
            &path,
            &ReadOptions {
                max_file_bytes: Some(100),
                ..Default::default()
            }
        )
        .is_err()
    );
    std::fs::write(
        root.join("part/m2/a.json"),
        json!({"id":"p3","messageID":"wrong","sessionID":"s","type":"text","text":"must reject"})
            .to_string(),
    )
    .unwrap();
    assert!(import_session(Agent::OpenCode, &path, &ReadOptions::default()).is_err());
}

#[test]
fn pi_manual_shell_and_extension_messages_keep_native_status_and_details() {
    let rows = [
        json!({"type":"session","version":3,"id":"s"}),
        json!({"type":"message","id":"shell","parentId":null,"message":{"role":"bashExecution","command":"cat fixture","output":"输出","exitCode":1,"cancelled":false,"truncated":true,"fullOutputPath":"/fixture/output.txt","timestamp":1700000000000_i64}}),
        json!({"type":"custom_message","id":"extension","parentId":"shell","customType":"fixture","content":"扩展输入","display":false,"details":{"original":"keep"}}),
    ];
    let r = lines(Agent::Pi, &rows, &ReadOptions::default());
    assert!(r.summary.is_supported());
    assert_eq!(messages(&r), ["扩展输入"]);
    assert!(r.events.iter().any(
        |e| matches!(&e.value,Event::ToolResult(t) if t.is_error==Some(true) && t.text=="输出")
    ));
    assert!(r.events.iter().any(|e|matches!(&e.value,Event::Content(c) if c.kind=="custom_message" && c.data["details"]["original"]=="keep")));
}

#[test]
fn codex_native_search_items_keep_call_identity_status_and_compaction_payload() {
    let pending = json!({"type":"web_search_call","id":"w0","status":"in_progress","action":{"type":"search","query":"fixture"}});
    let done = json!({"type":"web_search_call","id":"w1","status":"completed","action":{"type":"open_page","url":"https://example.test"}});
    let search = json!({"type":"tool_search_call","id":"item","call_id":"t","execution":"server","arguments":{"query":"fixture"}});
    let output = json!({"type":"tool_search_output","call_id":"t","status":"completed","execution":"server","tools":[{"name":"fixture","parameters":{"type":"object"}}]});
    let compact = json!({"type":"compaction","encrypted_content":"opaque"});
    let rows: Vec<_> = [&pending, &done, &search, &output, &compact]
        .iter()
        .map(|v| json!({"type":"response_item","payload":v}))
        .collect();
    let r = lines(Agent::Codex, &rows, &ReadOptions::default());
    assert!(r.summary.is_supported(), "{:?}", r.summary);
    let calls: Vec<_> = r
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::ToolCall(c) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[2].id.as_deref(), Some("t"));
    assert_eq!(
        calls[2].arguments,
        ToolArgs::Json(search["arguments"].clone())
    );
    let results: Vec<_> = r
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::ToolResult(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].call_id.as_deref(), Some("w1"));
    assert_eq!(results[0].output, Some(done));
    assert_eq!(results[1].output, Some(output));
    assert!(has(&r, "compaction", &compact));
    let content = lines(
        Agent::Codex,
        &rows,
        &ReadOptions {
            include: EventKinds::CONTENT,
            ..Default::default()
        },
    );
    assert_eq!(content.events.len(), 1);
    assert!(r.events.contains(&content.events[0]));
}

#[test]
fn pi_v1_linear_history_and_index_compaction_keep_current_context_and_billing() {
    let rows = [
        json!({"type":"session","id":"s"}),
        json!({"type":"message","message":{"role":"user","content":"旧问题"}}),
        json!({"type":"message","message":{"role":"assistant","content":"旧答案","usage":{"input":5,"output":2,"totalTokens":7}}}),
        json!({"type":"message","message":{"role":"user","content":"保留的问题"}}),
        json!({"type":"compaction","summary":"摘要","firstKeptEntryIndex":3}),
        json!({"type":"message","message":{"role":"assistant","content":"新答案"}}),
    ];
    let r = lines(Agent::Pi, &rows, &ReadOptions::default());
    assert!(r.summary.is_supported(), "{:?}", r.summary);
    assert_eq!(messages(&r), ["摘要", "保留的问题", "新答案"]);
    assert_eq!(
        r.events
            .iter()
            .filter(|e| matches!(e.value, Event::Usage(_)))
            .count(),
        1
    );
    assert_eq!(
        messages(&lines(Agent::Pi, &rows[..4], &ReadOptions::default())),
        ["旧问题", "旧答案", "保留的问题"]
    );
}

#[test]
fn codebuddy_legacy_duplicate_ids_are_not_upserted_and_ambiguous_forks_fail() {
    let mut rows = vec![
        json!({"type":"message","id":"same","role":"assistant","content":"回复"}),
        json!({"type":"function_call","id":"same","callId":"c","name":"read","arguments":"{}"}),
    ];
    let r = lines(Agent::CodeBuddy, &rows, &ReadOptions::default());
    assert_eq!(messages(&r), ["回复"]);
    assert!(
        r.events
            .iter()
            .any(|e| matches!(e.value, Event::ToolCall(_)))
    );
    rows.push(json!({"type":"resend-fork-notice","parentId":"same"}));
    let text = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert!(
        import_session_from(Agent::CodeBuddy, Cursor::new(text), &ReadOptions::default()).is_err()
    );
}

#[test]
fn kimi_undo_during_pending_tools_discards_deferred_prompts() {
    let mut rows = vec![
        json!({"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"当前问题"}]}}),
        json!({"type":"context.append_loop_event","event":{"type":"step.begin","uuid":"s"}}),
        json!({"type":"context.append_loop_event","event":{"type":"tool.call","stepUuid":"s","toolCallId":"c","name":"read","args":{}}}),
        json!({"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"尚未注入"}]}}),
        json!({"type":"context.undo","count":1}),
    ];
    let r = lines(Agent::KimiCli, &rows, &ReadOptions::default());
    assert!(r.summary.is_supported());
    assert!(messages(&r).is_empty());
    assert!(
        !r.events
            .iter()
            .any(|e| matches!(e.value, Event::ToolCall(_) | Event::ToolResult(_)))
    );
    let mut late = rows.clone();
    late.push(json!({"type":"context.append_loop_event","event":{"type":"tool.result","toolCallId":"c","result":{"output":"撤回后到达"}}}));
    late.push(json!({"type":"context.append_loop_event","event":{"type":"content.part","stepUuid":"s","part":{"type":"text","text":"过期分片"}}}));
    let r = lines(Agent::KimiCli, &late, &ReadOptions::default());
    assert!(r.summary.is_supported());
    assert!(messages(&r).is_empty());
    assert!(
        !r.events
            .iter()
            .any(|e| matches!(e.value, Event::ToolResult(_)))
    );
    rows[4] = json!({"type":"context.apply_compaction","compactedCount":2,"keptUserMessageCount":1,"summary":"压缩摘要"});
    let r = lines(Agent::KimiCli, &rows, &ReadOptions::default());
    assert_eq!(messages(&r), ["当前问题", "压缩摘要"]);
}

#[test]
fn content_filter_keeps_stream_identity_and_text_annotations() {
    let pi = json!({"type":"message","id":"m","message":{"role":"assistant","content":"答复","reasoning_content":"原生思考","tool_calls":[{"id":"c","function":{"name":"read","arguments":"{}"}}]}});
    let mut full = read_from(
        Agent::Pi,
        Cursor::new(pi.to_string()),
        &ReadOptions::default(),
    )
    .unwrap();
    let full: Vec<_> = full.by_ref().map(Result::unwrap).collect();
    let mut content = read_from(
        Agent::Pi,
        Cursor::new(pi.to_string()),
        &ReadOptions {
            include: EventKinds::CONTENT,
            ..Default::default()
        },
    )
    .unwrap();
    let content: Vec<_> = content.by_ref().map(Result::unwrap).collect();
    assert_eq!(content.len(), 1);
    assert!(full.contains(&content[0]));
    let block =
        json!({"type":"text","text":"引用正文","citations":[{"type":"page_location","page":1}]});
    let r = lines(
        Agent::ClaudeCode,
        &[json!({"type":"assistant","message":{"content":[block]}})],
        &ReadOptions::default(),
    );
    assert_eq!(messages(&r), ["引用正文"]);
    assert!(has(&r, "text", &block));
}

#[test]
fn zed_native_tool_input_matches_sdk_raw_json_and_tagged_input_contract() {
    for (input, expected) in [
        (
            json!({"command":"cat fixture.txt","cd":"."}),
            ToolArgs::Json(json!({"command":"cat fixture.txt","cd":"."})),
        ),
        (
            json!({"type":"json","value":{"path":"fixture"}}),
            ToolArgs::Json(json!({"path":"fixture"})),
        ),
        (
            json!({"type":"text","value":"raw input"}),
            ToolArgs::RawString("raw input".into()),
        ),
        (Value::Null, ToolArgs::Json(Value::Null)),
        (
            json!({"type":"json","value":1,"other":2}),
            ToolArgs::Json(json!({"type":"json","value":1,"other":2})),
        ),
    ] {
        let r = document(
            Agent::Zed,
            json!({"messages":[{"Agent":{"content":[{"ToolUse":{"id":"c","name":"terminal","input":input}}]}}]}),
            &ReadOptions::default(),
        );
        assert!(r.summary.is_supported());
        assert!(
            r.events
                .iter()
                .any(|e| matches!(&e.value,Event::ToolCall(c) if c.arguments==expected))
        );
    }
}
