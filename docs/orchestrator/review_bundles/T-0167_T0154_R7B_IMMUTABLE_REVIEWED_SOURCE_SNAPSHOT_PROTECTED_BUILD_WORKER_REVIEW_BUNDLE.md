# T-0167 R7B Reviewed Source Snapshot

Completion now captures a protected source-input manifest before
`completion.json` is emitted for structured-output contracts. Promotion rejects
reviews whose session lacks this snapshot with `REVIEWED_SOURCE_SNAPSHOT_REQUIRED`.
The manifest inventory covers Cargo manifests and immediate Rust sources using
safe regular-file identity and bounded SHA-256 evidence. No historical session
is migrated from current dirty workspace bytes.
