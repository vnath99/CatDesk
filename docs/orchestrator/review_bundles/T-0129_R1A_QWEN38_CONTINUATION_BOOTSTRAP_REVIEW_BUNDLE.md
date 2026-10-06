# T-0129-R1A — Qwen 3.8 continuation bootstrap

## Trigger and reproduced failure

Codex exhausted its provider allowance during T-0134 and CatDesk durably stopped the autonomous session with `provider_terminal_error`. Two bounded local-worker canaries then positively proved that `qwen3.8:27b` is loadable and can issue valid CatDesk `search`/`read` calls. The second canary failed on the next Ollama continuation with HTTP 500 and bounded provider body `no user query found in messages` after a successful tool result.

This is a worker-transport compatibility defect, not evidence that the model is unavailable.

## Narrow bootstrap design

Only the integrated delegated Ollama loop is changed.

- `model_requires_explicit_tool_continuation_user()` gates the compatibility behavior to the exact `qwen3.8` model family (case-insensitive family component before `:`). Older Qwen/Ollama models remain unchanged.
- After a successful CatDesk tool result, `push_qwen38_tool_continuation_after_success()` appends one bounded deterministic `user` continuation for qwen3.8. The message explicitly adds no authority, changes no tools/paths/budgets, and forbids replaying completed tool calls.
- The authoritative execution-contract user query remains in history; the compatibility message does not replace it or carry new task authority.
- The compatibility continuation is persisted through the existing durable `provider_history` state because it is inserted before the existing `persist_durable_state()` boundary.
- When `diff.actual` is the final authoritative diff after verification has already passed, the generic compatibility continuation is suppressed. The pre-existing final-reasoning user instruction remains the sole next user turn.
- No Codex, cloud-provider, browser, Secure MCP tunnel, Scheduler, promotion, Git-publication, or external-project behavior is changed.

## Deterministic coverage added

`src/delegated/integrated.rs` now covers:

1. exact qwen3.8 family gating, including case-insensitive model family matching and negative qwen3.5/qwen3.6/other cases;
2. real `execute_tool_call(read)` ordering for qwen3.8: `tool` result followed by the bounded non-authority `user` continuation with no tool id/name;
3. unchanged qwen3.5 history shape after the same successful read;
4. verified final `diff.actual` on qwen3.8: existing final-reasoning user instruction is retained and no generic compatibility continuation is duplicated.

The tests exercise the actual CatDesk tool-result history boundary rather than only a standalone string helper.

## Verification

CatDesk `verify_project(timeout=120000)` after the final formatting corrections:

- `cargo fmt --check` — PASS
- `cargo test` — PASS
- `cargo build` — PASS

The earlier verifier iterations also had all Rust tests/build passing; their only failures were rustfmt line wrapping in the newly added test assertions. Those formatting-only findings were corrected before the final PASS.

## Attribution and remaining work

Task-attributable product change for this bootstrap is confined to `src/delegated/integrated.rs` plus this review bundle. The repository-wide Git diff contains substantial pre-existing unrelated dirty-workspace changes and is not used as attribution evidence.

This bundle **does not close T-0135**. Remaining T-0135 work includes the first-class bounded Ollama model inventory/probe and a fresh live qwen3.8 delegated canary after this reviewed source is loaded into an isolated CatDesk candidate. Only after that can Qwen be trusted with T-0134 implementation work.

No canonical promotion or live daemon/tunnel/browser/Scheduler mutation occurred while producing this bundle.
