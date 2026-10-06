# T-0046B Host App-Server + Terra/High Review Bundle

Status: `IMPLEMENTED_PENDING_CATDESK_HOST_ACCEPTANCE`.

## Outcome

T-0046B moves the supported Codex app-server lifecycle to CatDesk host-side runtime code. The workspace-write Codex worker has no app-server launch or capability-probe path. The host launches only a direct executable as `codex app-server --stdio`, removes API-key environment names, inherits the current user's normal Codex context by default, and treats `CATDESK_CODEX_CLI_EXECUTABLE` and `CATDESK_CODEX_HOME` as optional validated recovery overrides. No auth contents are read, copied, serialized, or logged.

The implementation requires `gpt-5.6-terra` in autonomous Codex contracts and sends the existing explicit `model_reasoning_effort=high` CLI override. Before a Codex mutating turn, host app-server `thread/list`, `account/rateLimits/read`, and `thread/read` must produce one exact-CWD, non-owned thread whose authoritative metadata is exactly `gpt-5.6-terra` / `high`. Missing or mismatched metadata stops at `WAITING_FOR_CHATGPT`; it does not launch a provider turn.

## Host Boundary And Transport

- `CodexAppServerStdioTransportV1` owns child stdin/stdout, bounded JSON-RPC IDs, initialization, shutdown, notification skipping, malformed-message rejection, and a 64 KiB message limit.
- The transport allows only `initialize`, the documented read methods, and a resume of a previously exact/non-owned thread.
- The host preflight binds only the exact workspace thread matching `Integrate Codex MCP for ChatGPT` unless a prior exact binding supplies its ID. It never creates a replacement thread.
- A fresh host preflight is required at Codex task boundaries. A live worker turn is not disturbed by a second app-server launch.
- App-server responses are bounded before persistence; terminal malformed/oversized JSONL is fail-closed. Existing provider JSONL handling retains its non-terminal oversized-event placeholder behavior.

## Continuity, Telemetry, And Accounting

Preflight persists the canonical thread ID and bounded model, reasoning, token-usage, rate-limit, reset, plan, and metadata fields. A post-turn host refresh API records a second supported snapshot. The accounting ledger computes only comparable rate-limit percentage deltas and keeps credit usage `UNKNOWN_NOT_CAPTURED` unless authoritative comparable data exists; it does not infer credits from tokens or prices.

Existing stale `WORKER_RUNNING` recovery remains durable: a queued task with no owned live handle returns to `READY` while retaining the captured provider thread for safe continuation. No user Codex database is deleted, reset, copied, or recreated.

## Focused Regression Coverage

- Host app-server launch rejects shell shims and invalid operator context paths; its command receives only the opaque optional `CODEX_HOME` path.
- Bounded JSON-RPC parser rejects malformed, non-object, and oversized JSONL records.
- Exact-CWD resolution rejects ambiguity and concurrent ownership.
- Terra/High enforcement rejects both unknown and mismatched authoritative metadata.
- Controller-level gate stops before provider launch if host evidence is absent.
- Existing regressions cover stale queue recovery and oversized Codex worker JSONL placeholders.

## Worker Boundary

No nested `codex exec`, `codex app-server`, account, rate-limit, or live thread probe was launched while preparing this change. Live initialize/account/thread acceptance is intentionally left to the CatDesk host after the verified daemon is loaded.

## Verification Recorded

- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- Focused independent regressions: 59 passed (`codex_app_server`, `autonomy_runtime`, `autonomous_controller`, `autonomy_state`, `autonomy_accounting`, and `codex_cli`).
- `cargo test --no-fail-fast`: 406 passed, 7 failed, 9 ignored. The seven failures match the pre-existing Windows/environment-sensitive baseline: three advisor fixtures cannot find their configured program, the dependent advisor-cancellation assertion fails, and three process-tree cancellation checks receive `ERROR: Access denied`.
- `git diff --check`: PASS. Git emitted only existing CRLF-conversion warnings for the dirty working tree.

CatDesk independently performs daemon reload, app-server/account/thread acceptance, and final verification. This bundle does not claim those live checks completed.
