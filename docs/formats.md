# Format notes

These are implementation observations, not provider guarantees. Initial source
baselines: ccstats 2e2a766, remem 7f4e144f, refine 88ceb45. Local structure-only
inspection confirmed Codex 0.156.1 token_usage_record payload. Tests are synthetic.

## Claude Code

`user` / `assistant`: message.content is text or blocks. Text blocks are joined
with newlines; tool_use/server_tool_use and tool_result get separate events.
`message.id`, `sessionId`, `uuid`, `parentUuid`, isMeta/is_meta, isSidechain, cwd,
gitBranch, version, message.model and message.usage are retained as applicable.
Legacy progress tools are at data.message.message.content.

usage contains input_tokens, output_tokens, cache_creation_input_tokens,
cache_read_input_tokens and optional cache_creation.ephemeral_1h_input_tokens.
inference_geo == "not_available" maps heuristically to Native; "" to Proxy;
other values to Unknown. Raw inference_geo is retained; this is not authoritative
provider/billing evidence. No pricing or model name normalization is performed.

## Codex

session_meta and turn_context provide provenance/model updates. Structured source
and thread_source subagent evidence outrank originator fallback. Source exec stays
Exec even with originator Codex Desktop; vscode is Ide. This describes execution
provenance, not a judgment about whether a human initiated a session.

response_item.message provides role/content. Event-msg mirrors of user/assistant
messages are counted as ignored to avoid duplicate conversations. Function and
custom-tool calls retain raw arguments; result records retain call_id/output.
Reasoning and selected system events are intentionally ignored and counted.

event_msg.token_count.info provides total_token_usage and optional last_token_usage;
info:null can be a rate-limit-only update. Cache read aliases are accepted. Missing
counts remain unknown. token_usage_record.payload.usage is a per-response sample;
payload.response_id identifies it. Turn/thread totals in the same record are not
additional billable samples. Choose one usage ledger per read.

Unknown top-level types, response/event subtypes, roles and content block types
are counted. Invalid known fields produce line errors. Add fixtures before adding
a type to the known-ignored set; ignoring is an explicit compatibility decision.

Observed tool_search_output records carry a tools catalog rather than an output
string. They and Claude tool_reference blocks are known-ignored projections,
with explicit counters and synthetic fixtures. They are not ordinary tool results.
