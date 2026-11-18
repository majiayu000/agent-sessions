//! Grok Build and Kimi Code v2 record shapes first seen in a read-only sweep of
//! real local histories (`examples/corpus_sweep.rs`). Fixtures are synthetic:
//! they copy the native field structure, never the original text, IDs or
//! payloads.
use agent_sessions::*;
use serde_json::{Value, json};
use std::io::Cursor;

fn lines(agent: Agent, rows: &[Value]) -> SessionImport {
    let text = rows
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    import_session_from(agent, Cursor::new(text), &ReadOptions::default()).unwrap()
}

fn contents(result: &SessionImport) -> Vec<(&str, &Value)> {
    result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Content(c) => Some((c.kind.as_str(), &c.data)),
            _ => None,
        })
        .collect()
}

fn count(result: &SessionImport, f: impl Fn(&Event) -> bool) -> usize {
    result.events.iter().filter(|e| f(&e.value)).count()
}

fn texts(result: &SessionImport) -> Vec<(Role, &str)> {
    result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::Message(m) => Some((m.role, m.text.as_str())),
            _ => None,
        })
        .collect()
}

fn grok(update: Value) -> Value {
    json!({"timestamp":1700000000,"method":"_x.ai/session/update",
        "params":{"sessionId":"g","_meta":{"eventId":"e","agentTimestampMs":1700000000123_i64},"update":update}})
}

// Grok Build 1.0.7x journals compaction, media normalization, workflow/goal
// progress and scheduled tasks. They are context/control records that can
// carry text (notes, objectives, prompts), so they stay native Content; their
// subagent/goal token counters are progress figures, not a billed ledger.
#[test]
fn grok_context_and_progress_updates_are_native_content() {
    let updates = [
        json!({"sessionUpdate":"auto_compact_started","context_window":100,"percentage":85,"reason":"threshold","tokens_used":85}),
        json!({"sessionUpdate":"auto_compact_completed","elapsed_ms":5,"summary_preview":null,"tokens_after":10,"tokens_before":85}),
        json!({"sessionUpdate":"compaction_checkpoint","checkpoint_file":"c.json","checkpoint_id":"k","created_at":"t","prompt_index_at_compaction":2,"schema_version":1}),
        json!({"sessionUpdate":"image_dropped","notes":["图片被省略"]}),
        json!({"sessionUpdate":"image_compressed","message":"已压缩","images":[{"index":0,"original_bytes":9,"compressed_bytes":3,"original_width":4,"original_height":4,"compressed_width":2,"compressed_height":2}]}),
        json!({"sessionUpdate":"workflow_updated","name":"w","objective":"目标","status":"running","run_id":"r","revision":1,
            "agents":[{"agent_id":"a","label":"标签","phase":"p","state":"done","duration_ms":1,"tokens_used":500}],
            "phases":[{"state":"done","title":"阶段"}],"agents_used":1,"agent_budget":2,"active_agents":0,"agents_remaining":1,
            "agents_reserved":0,"agent_usage_incomplete":false,"elapsed_ms":1,"foreground":true,"last_event":"e","last_event_timestamp":"t"}),
        json!({"sessionUpdate":"goal_updated","goal_id":"g","objective":"目标","phase":"p","status":"active","tokens_used":700,"token_baseline":0,
            "finished_subagent_tokens":200,"completed_deliverables":0,"total_deliverables":1,"total_verify_rounds":0,"total_worker_rounds":1,
            "elapsed_ms":1,"last_event":"e","last_event_timestamp":"t"}),
        json!({"sessionUpdate":"memory_flush_completed","result":"saved"}),
        json!({"sessionUpdate":"scheduled_task_created","task_id":"t","human_schedule":"daily","next_fire_at":"t","prompt":"定时提示"}),
        json!({"sessionUpdate":"scheduled_task_deleted","task_id":"t","reason":"用户删除"}),
    ];
    let mut rows: Vec<_> = updates.iter().cloned().map(grok).collect();
    rows.push(grok(json!({"sessionUpdate":"memory_flush_started"})));
    let result = lines(Agent::Grok, &rows);
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    let got = contents(&result);
    assert_eq!(got.len(), updates.len());
    for ((kind, data), update) in got.iter().zip(&updates) {
        assert_eq!(*kind, update["sessionUpdate"].as_str().unwrap());
        assert_eq!(*data, update);
    }
    assert_eq!(
        result
            .summary
            .ignored_types
            .get("grok:memory_flush_started"),
        Some(&1)
    );
    assert_eq!(count(&result, |e| matches!(e, Event::Message(_))), 0);
    assert_eq!(count(&result, |e| matches!(e, Event::Usage(_))), 0);
}

// Server-side ("backend") search calls carry no `x.ai/tool` metadata; the
// native variant in rawInput is their tool identity, present on every call.
#[test]
fn grok_backend_search_calls_use_native_variant_as_name() {
    let rows = [
        grok(
            json!({"sessionUpdate":"tool_call","toolCallId":"w","kind":"search","status":"in_progress","title":"Web search:",
            "rawInput":{"backend":true,"variant":"WebSearch"},"_meta":{"backend":true}}),
        ),
        grok(
            json!({"sessionUpdate":"tool_call_update","toolCallId":"w","status":"completed","title":"Web search:",
            "rawOutput":{"id":"w","status":"completed","action":{"type":"search","query":"查询","sources":[]}}}),
        ),
        grok(
            json!({"sessionUpdate":"tool_call","toolCallId":"x","kind":"search","status":"in_progress","title":"X search:",
            "rawInput":{"backend":true,"variant":"XSearch"},"_meta":{"backend":true}}),
        ),
        grok(
            json!({"sessionUpdate":"tool_call_update","toolCallId":"x","status":"completed","title":"X search:",
            "rawOutput":{"id":"x","call_id":"x","name":"x_keyword_search","input":"{}"}}),
        ),
    ];
    let result = lines(Agent::Grok, &rows);
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    let calls: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::ToolCall(c) => Some((c.id.as_deref(), c.name.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        vec![(Some("w"), "WebSearch"), (Some("x"), "XSearch")]
    );
    let results: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| match &e.value {
            Event::ToolResult(r) => Some(r.call_id.as_deref()),
            _ => None,
        })
        .collect();
    assert_eq!(results, vec![Some("w"), Some("x")]);
}

// Without the backend marker and variant there is no reliable native name, so
// the call stays unknown instead of borrowing the display title.
#[test]
fn grok_tool_without_native_identity_stays_unknown() {
    for update in [
        json!({"sessionUpdate":"tool_call","toolCallId":"a","kind":"search","title":"Web search:","rawInput":{"variant":"WebSearch"}}),
        json!({"sessionUpdate":"tool_call","toolCallId":"b","kind":"search","title":"Web search:","rawInput":{"backend":true},"_meta":{"backend":true}}),
    ] {
        let result = lines(Agent::Grok, &[grok(update)]);
        assert!(!result.summary.is_supported());
        assert_eq!(
            result
                .summary
                .unknown_types
                .get("grok:tool-without-native-name"),
            Some(&1)
        );
        assert_eq!(count(&result, |e| matches!(e, Event::ToolCall(_))), 0);
    }
}

fn kimi_append(text: &str, id: &str) -> Value {
    json!({"type":"context.append_message","time":1700000000000_i64,
        "message":{"role":"user","id":id,"content":[{"type":"text","text":text}]}})
}
fn kimi_loop(e: Value) -> Value {
    json!({"type":"context.append_loop_event","time":1700000000001_i64,"event":e})
}

// Kimi Code 2.x also writes the agent-loop engine's own history
// (`agent.message.appended`). It mirrors context.* messages and its meta.usage
// mirrors usage.record, so it is native Content: no second Message/ToolCall/
// ToolResult or Usage.
#[test]
fn kimi_v2_engine_history_mirror_is_content_without_double_counting() {
    let usage = json!({"inputOther":7,"output":3,"inputCacheRead":2,"inputCacheCreation":0});
    let mirrors = [
        json!({"type":"agent.message.appended","kind":"event","time":1700000000002_i64,
            "message":{"message":{"role":"user","content":[{"type":"text","text":"问题"}]},
                "meta":{"source":"input","origin":{"kind":"user"},"promptId":"u","tracked":true}}}),
        json!({"type":"agent.message.appended","kind":"event","time":1700000000003_i64,
            "message":{"message":{"role":"assistant","content":[{"type":"text","text":"回答"}],
                "toolCalls":[{"type":"function","id":"c","name":"Read","arguments":"{}"}]},
                "meta":{"source":"llm","usage":usage,"finish":{"finishReason":"tool_calls","rawFinishReason":"tool_calls"},
                    "model":{"provider":"agent-loop","model":"agent-loop"},"messageId":"m"}}}),
        json!({"type":"agent.message.appended","kind":"event","time":1700000000004_i64,
            "message":{"message":{"role":"tool","toolCallId":"c","content":[{"type":"text","text":"结果"}]},"meta":{"source":"tool"}}}),
        json!({"type":"agent.turn.ended","kind":"event","time":1700000000005_i64,"turnId":1,"outcome":"failed","errorMessage":"出错"}),
    ];
    let mut rows = vec![
        json!({"type":"agent.turn.started","kind":"event","time":1700000000000_i64,"turnId":1,"queueItemId":"q"}),
        kimi_append("问题", "u"),
        kimi_loop(json!({"type":"step.begin","uuid":"s"})),
        kimi_loop(
            json!({"type":"content.part","stepUuid":"s","part":{"type":"text","text":"回答"}}),
        ),
        kimi_loop(
            json!({"type":"tool.call","stepUuid":"s","toolCallId":"c","name":"Read","args":{}}),
        ),
        kimi_loop(
            json!({"type":"tool.result","toolCallId":"c","result":{"output":"结果","isError":false}}),
        ),
        kimi_loop(json!({"type":"step.end","stepUuid":"s","finishReason":"tool_calls"})),
        json!({"type":"usage.record","time":1700000000001_i64,"model":"k","usage":usage}),
    ];
    rows.extend(mirrors.iter().cloned());
    let result = lines(Agent::KimiCli, &rows);
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(
        texts(&result),
        vec![(Role::User, "问题"), (Role::Assistant, "回答")]
    );
    assert_eq!(count(&result, |e| matches!(e, Event::ToolCall(_))), 1);
    assert_eq!(count(&result, |e| matches!(e, Event::ToolResult(_))), 1);
    assert_eq!(count(&result, |e| matches!(e, Event::Usage(_))), 1);
    let got = contents(&result);
    assert_eq!(got.len(), mirrors.len());
    for ((kind, data), mirror) in got.iter().zip(&mirrors) {
        assert_eq!(*kind, mirror["type"].as_str().unwrap());
        assert_eq!(*data, mirror);
    }
    let role = result.events.iter().find_map(|e| match &e.value {
        Event::Content(c) if c.kind == "agent.message.appended" => Some(c.role),
        _ => None,
    });
    assert_eq!(role, Some(Some(Role::User)));
    assert_eq!(
        result
            .summary
            .ignored_types
            .get("kimi:v2:agent.turn.started"),
        Some(&1)
    );
}

// The vendor's recovery note calls everything outside context.* bookkeeping;
// records that can hold text (system prompt, steer input, approval action,
// task command/output, plugin reminder, tool descriptions) stay native
// Content, numeric/identity-only records are ignored. Steer input is already
// a context message and is not emitted twice.
#[test]
fn kimi_v2_bookkeeping_records_are_content_or_ignored() {
    let t = 1700000000000_i64;
    let text_bearing = [
        json!({"type":"profile.bind","time":t,"agentId":"main","profileName":"agent","systemPrompt":"系统提示","modelAlias":"m",
            "thinkingEffort":"high","activeToolNames":["Read"],"subagents":["coder"],"agentsMdPaths":[],"disallowedTools":[]}),
        json!({"type":"plugin.session_start","time":t,"agentId":"main","content":null}),
        json!({"type":"changeNotifier.snapshot","time":t,"agentId":"main","agentsMdFiles":{},"agentsMdHash":null,
            "modelPool":null,"skillsHash":"h","subagentNames":["plan"]}),
        json!({"type":"turn.steer","time":t,"input":[{"type":"text","text":"后台完成"}],
            "origin":{"kind":"background_task","notificationId":"n","status":"completed","taskId":"t"}}),
        json!({"type":"turn.step.interrupted","time":t,"agentId":"main","turnId":1,"step":1,"reason":"error","message":"错误"}),
        json!({"type":"permission.record_approval_result","time":t,"turnId":1,"toolCallId":"c","toolName":"Bash",
            "action":"运行命令","result":{"decision":"approved","scope":"session"},"sessionApprovalRule":"规则"}),
        json!({"type":"interaction.request","time":t,"agentId":"main","id":"i","kind":"approval","toolCallId":"c",
            "request":{"action":"打开","toolName":"browser","display":{"kind":"generic","summary":"摘要"}}}),
        json!({"type":"interaction.resolved","time":t,"agentId":"main","id":"i","response":{"decision":"approved"}}),
        json!({"type":"task.started","time":t,"agentId":"main","info":{"taskId":"t","kind":"process","command":"命令",
            "description":"描述","status":"running","detached":false,"pid":1,"startedAt":t,"endedAt":null,"exitCode":null}}),
        json!({"type":"task.terminated","time":t,"agentId":"main","outputTail":"输出","info":{"taskId":"t","kind":"process",
            "command":"命令","status":"timed_out","exitCode":1,"endedAt":t}}),
        json!({"type":"mcp.tools_discovered","time":t,"agentId":"main","serverName":"s","hash":"h","enabledNames":["run"],
            "tools":[{"name":"run","description":"描述","inputSchema":{"type":"object"}}]}),
    ];
    let metadata = [
        json!({"type":"runtime.set_binding","time":t,"agentId":"main","runtimeId":"local","workspaceId":"w"}),
        json!({"type":"token_counting.measured","time":t,"agentId":"main","length":3,"tokens":40}),
        json!({"type":"token_counting.turn_recorded","time":t,"agentId":"main","length":3,"tokens":40,"turnId":1}),
        json!({"type":"prompt.completed","time":t,"agentId":"main","promptId":"p","reason":"completed","finishedAt":"t"}),
        json!({"type":"prompt.aborted","time":t,"agentId":"main","promptId":"p","abortedAt":"t"}),
        json!({"type":"file_history.tracked","time":t,"agentId":"main","turnId":1,"path":"a.html","entry":{"key":null,"version":0}}),
        json!({"type":"file_history.checkpoint","time":t,"agentId":"main","turnId":1,"phase":"end",
            "entries":{"a.html":{"contentHash":"h","key":"k","size":1,"version":1}}}),
    ];
    let mut rows = vec![kimi_append("后台完成", "u")];
    rows.extend(text_bearing.iter().cloned());
    rows.extend(metadata.iter().cloned());
    let result = lines(Agent::KimiCli, &rows);
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    assert_eq!(texts(&result), vec![(Role::User, "后台完成")]);
    assert_eq!(count(&result, |e| matches!(e, Event::Usage(_))), 0);
    let got = contents(&result);
    assert_eq!(got.len(), text_bearing.len());
    for ((kind, data), row) in got.iter().zip(&text_bearing) {
        assert_eq!(*kind, row["type"].as_str().unwrap());
        assert_eq!(*data, row);
    }
    for row in &metadata {
        let key = format!("kimi:v2:{}", row["type"].as_str().unwrap());
        assert_eq!(result.summary.ignored_types.get(&key), Some(&1), "{key}");
    }
}

// A refused call's failed update carries `_meta: {toolNotRun: <reason>}`; it
// must not erase the native name recorded on the original tool_call.
#[test]
fn grok_tool_not_run_update_keeps_native_name() {
    let rows = [
        grok(
            json!({"sessionUpdate":"tool_call","toolCallId":"c","title":"t","rawInput":{"command":"x"},
            "_meta":{"x.ai/tool":{"name":"run_terminal_cmd","kind":"execute","label":"l","namespace":"n","read_only":false,"version":1}}}),
        ),
        grok(
            json!({"sessionUpdate":"tool_call_update","toolCallId":"c","status":"failed","_meta":{"toolNotRun":"denied"},
            "content":[{"type":"content","content":{"type":"text","text":"未运行"}}]}),
        ),
    ];
    let result = lines(Agent::Grok, &rows);
    assert!(result.summary.is_supported(), "{:?}", result.summary);
    let call = result.events.iter().find_map(|e| match &e.value {
        Event::ToolCall(c) => Some(c.name.as_str()),
        _ => None,
    });
    assert_eq!(call, Some("run_terminal_cmd"));
    let outcome = result.events.iter().find_map(|e| match &e.value {
        Event::ToolResult(r) => Some((r.call_id.as_deref(), r.is_error)),
        _ => None,
    });
    assert_eq!(outcome, Some((Some("c"), Some(true))));
}
