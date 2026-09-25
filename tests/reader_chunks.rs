use agent_sessions::*;
use std::io::{BufReader, Cursor};
fn message() -> Vec<u8> {
    b"{\"type\":\"user\",\"message\":{\"content\":\"chunk\"}}\n".to_vec()
}
fn reader(s: &[u8], opts: ReadOptions) -> SessionReader<Cursor<&[u8]>> {
    read_from(Agent::ClaudeCode, Cursor::new(s), &opts).unwrap()
}

#[test]
fn chunk_size_does_not_change_events() {
    let data = message();
    let expected = reader(&data, ReadOptions::default())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for cap in 1..data.len() + 1 {
        let source = BufReader::with_capacity(cap, Cursor::new(&data));
        let actual = read_from(Agent::ClaudeCode, source, &ReadOptions::default())
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(actual, expected);
    }
}
