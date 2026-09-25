# Synthetic fixtures

Every checked-in transcript is synthetic, authored from the documented field
shapes, not copied from private conversations. No real paths, IDs, messages or
credentials are present. Filenames label compatibility profiles, not claims
that a real agent generated these exact transcripts.

Claude fixtures reflect 2.1.x and legacy progress shapes; Codex fixtures reflect
the legacy rollout and observed 0.156.1 response-usage shape. Pinning exact client
versions requires an independently captured and reviewed public fixture.

The golden test compares full events, physical provenance and final diagnostics.
Initial snapshots were reviewed alongside hand-authored contract assertions.
To intentionally refresh: UPDATE_GOLDEN=1 cargo test --test golden. Review every
changed value before accepting; do not refresh to hide a failing regression.
