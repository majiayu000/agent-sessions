use super::*;
use serde_json::json;

#[test]
fn selective_decode_preserves_selected_events_and_native_metadata() {
    let rows = [
        json!({"type":"session_meta","uuid":"record","timestamp":null,"created_at":100,
            "payload":{"id":"session","source":null,"originator":"Codex Desktop","cli_version":42,"cwd":"C:\\work\\项目"}}),
        json!({"type":"turn_context","payload":{"model":"m"}}),
        json!({"type":"response_item","timestamp":"2026-01-01T00:00:00Z","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"not requested"}]}}),
        json!({"type":"event_msg","timestamp":"2026-01-01T00:00:00.000Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100}}}}),
        json!({"type":"token_usage_record","timestamp":"2026-01-01T00:00:00Z","payload":{"response_id":"r","usage":{"input_tokens":100}}}),
        json!({"type":"event_msg","payload":{"type":"token_count","info":null}}),
        json!({"type":"response_item","payload":{"type":"future"}}),
    ];
    for mode in [CodexUsageMode::TokenCount, CodexUsageMode::Response] {
        let opts = ReadOptions {
            accounting: AccountingPolicy::UsageStatistics,
            include: EventKinds::META.union(EventKinds::USAGE),
            codex_usage: mode,
            ..Default::default()
        };
        let mut normal = State::default();
        let mut selective = State::default();
        for value in &rows {
            let reference = parse(
                Agent::Codex,
                value,
                &mut normal,
                mode,
                opts.include,
                opts.accounting,
            )
            .unwrap();
            let fast = match decode(
                Agent::Codex,
                value.to_string().as_bytes(),
                &mut selective,
                &opts,
            ) {
                Ok(p) => p,
                Err(_) => panic!("valid record rejected"),
            };
            assert_eq!(reference.events, fast.events);
            let mut unknown = fast.unknown.clone();
            let mut ignored = fast.ignored.clone();
            if let Some((known, label)) = &fast.ignored_one {
                if *known {
                    ignored.push(label.to_string());
                } else {
                    unknown.push(label.to_string());
                }
            }
            assert_eq!(reference.unknown, unknown);
            assert_eq!(reference.ignored, ignored);
            if !reference.events.is_empty() {
                assert_eq!(reference.at, fast.at);
                assert_eq!(reference.timestamp_text, fast.timestamp_text);
                assert_eq!(reference.record_id, fast.record_id);
                assert_eq!(reference.record_type, fast.record_type);
            }
        }
    }
}
