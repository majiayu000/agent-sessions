# Ecosystem technical contract

Reuse the bounded fill_buf/consume framing implementation for typed and raw
readers. Raw records own at most max_line_bytes; oversized lines are drained and
reported. Raw file helper seeks to start_offset then limits to a captured boundary.
Raw summary reports delivered IO framing, never semantic completeness.

HistoryReader wraps RawReader and parses one JSON object per record; independently
tracks JSON/field errors so a complete raw stream cannot hide semantic failures.
Tail mode follows the typed reader: only final JSON EOF without a delimiter may
be tolerated. Timestamp parsing is host-specific with explicit units.

Title reader is an independent index API, not a transcript scan. It filters IDs
before returning values and never logs field contents. Caller adapter maps its
public title types. Keep path/environment discovery in Roots, pricing in ccstats,
and archive hashes/identity/persistence in archive consumers.

Text segments point into the already joined Message.text (UTF-8 byte ranges).
This avoids duplicating whole message bodies while preserving block boundaries.
The first segment may be empty; ccp must preserve its current first-block policy.

Alternatives: forcing raw archives through Event loses evidence; adding full raw
payloads to every Event duplicates memory and obscures completeness. A separate
raw framing API gives archive users exact data without taxing statistics readers.

No database or host hooks change in this library step. Publication is deferred
until all selected consumer adapters and package checks have passed.
