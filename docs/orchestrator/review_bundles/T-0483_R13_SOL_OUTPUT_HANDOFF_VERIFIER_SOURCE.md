# T-0483 R13 — Dormant Sol worker output handoff verifier

Date: 2026-10-15. Status: source-only verifier; no Sol worker or live handoff accepted.

## Source change

`src/delegated/sol_worker_output.rs` defines a bounded output manifest for changed regular files. The trusted host supplies the expected task ID, baseline commit, native CLI turn ID, handoff nonce and exact path allowlist independently of that manifest. The verifier requires the values to match, validates each relative path and size/hash entry, inventories the whole pinned output directory twice, reads each file through a no-follow unique-link handle, checks size and SHA-256, and returns a host-owned map of captured bytes. It rejects unexpected files or empty directories, case aliases, reparse points, hardlinks, unsafe path components and duplicate entries. The output is an export, not an arbitrary worker worktree.

The limits are 256 files, 12 path components, 512 path bytes, a 256 KiB manifest, 16 MiB per file and 64 MiB total. The bounded native directory enumerator refuses an overfull directory before collecting an unbounded inventory. The module is registered in `src/delegated/mod.rs` but has no producer launch or controller call site.

## Local verification

Five focused Windows tests passed: exact nested bytes remain host-owned after a later file change; wrong task binding/hash or extra inventory fails; aliases, hardlinks and unlisted directories fail; malformed/oversized manifest fails; an overfull directory is rejected. The full root all-features Rust suite exited 0 (main binary: 1,145 passed, 26 ignored; integration suites passed), as did strict all-target/all-feature Clippy and rustfmt. The printed AppContainer access-denied child process is expected negative test evidence; the outer suite passed. The exact new commit's independent three-job Windows CI still needs confirmation.

## Required handoff boundary

A separate OS service must start the restricted worker with CatDesk authority outside its write scope, retain the exact output directory object, prove worker identity and termination, and prove the worker has lost write access before invoking this verifier. The host also needs a durable one-use nonce ledger and a trusted staging transaction that consumes the returned bytes, with a reviewed failure path. This module does not supply those proofs or enable Sol launch. It does not show a real Codex-produced deliverable, ChatGPT review, natural `review_ready` event, independent Wake delivery or recurring cycle. Terra/high compatibility and the P0 Wake installation hold remain in effect.
