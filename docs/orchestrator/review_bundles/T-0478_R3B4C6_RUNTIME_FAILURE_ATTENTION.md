# T0478 R3B4-C6 — Runtime failure attention

Date: 2026-10-14. Source-only reviewed-candidate lifecycle increment; no installed image, WakeHost, browser or tunnel changed.

## Change

After a candidate listener reports ready, four failure paths now attempt a create-only, file-synced `runtime-attention.json`: unexpected listener exit, Ctrl-C signal failure, shutdown failure, and retained source leases. The record is tied to the journal's canonical `serving.json`, including runtime generation, snapshot, intent hash, opened root identity, owner PID and bound loopback port. Fixed failure codes and canonical encoding are checked on read. An attention-write error is included in the returned launch error rather than hidden.

Signal and shutdown errors abort the server task and await it for a bounded two seconds before returning. The first runtime failure record cannot be overwritten by a later writer.

## Verification

- Focused Windows owner-journal crash-reopen test passed: serving and runtime attention records survive handle close/reopen, reject a second writer, and reject a changed runtime generation or serving root identity.
- Strict Clippy passed. Full local `cargo test --all-features --quiet` exited 0 (main binary 1,083 passed, 26 ignored; subsequent integration groups passed). Rustfmt and diff whitespace checks passed.
- Positive witnessed launch remains untestable while exact-commit GitHub Actions has no check runs and workflow dispatch returns HTTP 422, Actions disabled.

## Limits and next action

This is forensic evidence, not a live process or deletion capability. Startup failure before `serving.json`, retirement failure after the snapshot is consumed, clean journal retirement, cross-process exclusion, exact PID/port ownership and successful witnessed HTTP/stop acceptance remain. Next implement those lifecycle states and restore exact three-job CI before deployment.
