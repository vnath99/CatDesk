# T0478 R3B4-B3B2B2D — Retain ownership after partial candidate staging

Date: 2026-10-14. Scope: dormant witnessed-candidate source path; no production worker or MCP route.

## Change

`stage_witnessed_candidate_snapshot` previously returned `Result<StagedCandidateSnapshot, String>`. Once it created a disposable root, any ACL, file-create, readback or retained-lease error dropped the root handle and returned only text. A partial source file could remain with no in-process owner capability.

It now distinguishes `BeforeOwnership`, `CreationIndeterminate`, and `Owned` failure. The `Owned` variant carries the exact pinned `ProtectedDirectoryGuard`, witnessed manifest and bounded reason. The shared post-create stage helper returns that owned failure for partial writes and lease failures. A native create error is conservatively `CreationIndeterminate`: it carries the guard and generated name for operator diagnosis, because the filesystem might have materialized the child before a later verification failed. Neither its name nor the guard alone authorizes deletion. There is no cleanup by generated-name scan, caller pathname or recursion.

The positive owned-stage test now uses the same helper as the witnessed source path. A collision on the second reviewed leaf forces a partial first-file write and returns the still-stable root/manifest; the preexisting second leaf remains byte-for-byte unchanged. A separate create-collision test confirms the indeterminate variant preserves the pinned guard and foreign bytes without claiming deletion authority. An owner lifecycle can use the returned capability to persist an attention outcome. No caller is connected yet.

## Verification

- Focused `isolated_fixture_snapshot` suite: 11 passed, including partial-write, create-collision, and prior exact-handle retirement tests.
- Root `cargo test --all-features --quiet`: exit 0 on the final revision. Main binary 1,076 passed / 0 failed / 26 ignored; integration suites passed. The AppContainer child access-denied output is expected within its passing parent test.
- Strict root all-target/all-features Clippy, formatting and `git diff --check`: pass.

## Remaining gate

The returned handle protects ordinary error handling only while the process lives. A crash can still leave a generated root without durable trusted owner metadata or a recorded attention state. Before enabling retirement, the producer must create a write-ahead ownership record bound to the exact root volume/file ID, snapshot ID, witnessed manifest and generation, and persist bounded operator attention on uncertain failure. A resumed process may inspect that evidence but must never auto-delete by name. T0478-C, T0478-D, T0479 and independent Windows CI remain outstanding.
