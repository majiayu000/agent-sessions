#![no_main]
use agent_sessions::*;
use libfuzzer_sys::fuzz_target;
use std::io::{BufReader,Cursor};

fuzz_target!(|data: &[u8]| {
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(data) {
        let _ = project_conversation(&v);
        let _ = tolerant_timestamp_epoch(&v);
        let _ = project_codex_function(&v);
        for agent in [Agent::ClaudeCode, Agent::Codex] {
            let projected = project_transcript(agent, &v);
            if let Some(message) = projected.message {
                for range in message.text_segments { assert!(message.text.get(range).is_some()); }
            }
        }
    }
    let raw_opts=RawReadOptions {max_read_bytes:Some(65536),max_line_bytes:Some(8192),..Default::default()};
    let mut raw=read_raw_from(Cursor::new(data),&raw_opts).unwrap();
    for _ in raw.by_ref() {}
    let summary=raw.finish();
    assert!(summary.delivered_through<=summary.bytes_read);
    let mut statistical=read_from(Agent::Codex,Cursor::new(data),&ReadOptions {
        accounting:AccountingPolicy::UsageStatistics,include:EventKinds::USAGE.union(EventKinds::META),
        max_file_bytes:Some(65536),max_line_bytes:Some(8192),..Default::default()
    }).unwrap();
    for _ in statistical.by_ref() {}
    for agent in [Agent::ClaudeCode,Agent::Codex] {
        let history_opts=HistoryOptions {read:raw_opts.clone(),tail:TailMode::AllowIncomplete,..Default::default()};
        let mut history=read_history_from(agent,Cursor::new(data),&history_opts).unwrap();
        for _ in history.by_ref() {}
        let summary=history.finish();
        assert!(summary.last_complete_byte<=summary.bytes_read);
        let opts=ReadOptions {
            max_file_bytes:Some(65536),max_line_bytes:Some(8192),
            tail:TailMode::AllowIncomplete,..Default::default()
        };
        let mut r=read_from(agent,BufReader::with_capacity(7,Cursor::new(data)),&opts).unwrap();
        let mut last=(0,0);
        for result in r.by_ref() {
            if let Ok(e)=result {
                assert!(e.location.byte_start<=e.location.byte_end);
                assert!((e.location.record_index,e.location.event_index)>=last);
                last=(e.location.record_index,e.location.event_index);
            }
        }
        let summary=r.finish();
        assert!(summary.last_complete_byte<=summary.bytes_read);
        assert_ne!(summary.status,ReadStatus::StoppedEarly);
    }
});
