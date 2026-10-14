# T-0483 R5 — Reviewed source path for canonical Sol thread rotation

Date: 2026-10-14. Status: source only; no registered thread was rotated.

## Change

CatDesk now has separate `thread-rotation preflight` and `thread-rotation confirm` controls for a registered project in the supervisor's exact workspace. Rotation is limited to an existing idle Terra/high canonical thread and a distinct idle Sol/medium candidate. The app-server reads exact workspace, thread, ownership, model and effort metadata. The operator receives opaque candidate handles and fingerprints; confirmation re-reads the selected thread. A 120-second preflight stores the expected previous thread. The registry write requires a matching old binding, workspace, Git identity, an unbound new thread, and exact Sol/medium evidence.

The Sol thread must already be visible in the app-server's bounded recent history. Rotation discovery rejects a listed concurrently owned thread in the workspace, including one using another model. Preflight does not create an unreviewed parallel thread. If no Sol candidate is present, it fails without changing the registry.

All local autonomy sessions must be terminal and inactive. A strict session scan rejects damaged state instead of skipping it, while ignoring only the known `diagnostics` and `qwen_tools` auxiliary directories. Session approval and registry rotation share an exclusive OS file lock, released when its handle closes even after a process exit. Under that lock, confirmation repeats session, Git origin/root, and old/new app-server checks immediately before the compare-and-swap. Ordinary binding/adoption cannot replace an existing thread or confirm a rotation preflight. The old thread remains in Codex history; no thread content or project target changes.

## Verification

- Focused project-registry tests cover stale old binding, sibling collision, workspace/profile drift, expiry, opaque handles, adoption/rotation separation, quiescence guard failure, exact replay and preservation of the sibling binding.
- App-server tests cover exact Sol/medium selection from mixed idle Terra and Sol threads and rejection of another owned workspace thread. Supervisor tests cover unfinished and corrupt session rejection. Operator parser tests cover the fixed rotation action.
- Local `cargo test --all-features --quiet` exited 0: main binary 1,065 passed, 26 ignored; integration binaries passed. A child AppContainer fixture printed an access-denied failure as expected while its parent test passed. Strict all-target Clippy and formatting checks passed after the final edits.

## Release and acceptance limits

The currently installed CatDesk daemon does not expose this source path. No live Codex task, source-bound review record, or Wake event is claimed. The CatDesk project remains bound to its Terra thread. Exact-commit independent Windows CI, reviewed serving deployment, and fresh current-state authority checks are required before using the rotation control. The separate WakeHost dev.85 store-authority discrepancy and protected install gate still block P0 live acceptance. Do not replay the historical ambiguous Wake event.

Read-only local state inspection found six active CatDesk autonomy sessions (three `WAITING_FOR_CHATGPT`, one `PAUSED`, one `QUEUED`, one `DRAFT`). The rotation gate correctly refuses these unfinished sessions. They require supported session-level review and resolution before live rotation; no state was changed. The connected Wake status still reported dev.84/gen34 and the ambiguous old `SUBMITTING` event, while direct installed dev.84 CLI read the old dev.59/gen1 store. The connected status also reported `STATE_REPLACE_FAILED`; no installed Wake mutation or canary followed.
