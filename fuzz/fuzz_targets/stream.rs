#![no_main]
use agent_sessions::*;
use libfuzzer_sys::fuzz_target;
use std::io::{BufReader,Cursor};

fuzz_target!(|data: &[u8]| {
    for agent in [Agent::ClaudeCode,Agent::Codex] {
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
