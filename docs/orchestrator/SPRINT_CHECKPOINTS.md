# Sprint Checkpoints

Date: 2026-07-25
Branch: orchestrator/v1-coding-sprint

## Milestone A - Architecture Pivot And Core Protocol

Status: PASSED_AFTER_PIVOT

Tickets:

- T-0012: local commit `d0cc25e`
- T-0013: local commit `53e06a4`
- T-0013A: local commit `35e23dc`
- T-0013B: local commit `bd29103`

Required gate evidence:

| Gate condition | Evidence | Status |
| --- | --- | --- |
| CatDesk-owned loop frozen | T-0013B architecture decision removes OpenClaw from the required v1 path. | passed |
| No required OpenClaw dependency | OpenClaw is deferred optional research and must satisfy the CatDesk provider boundary before reuse. | passed |
| Provider-neutral runtime and patch contracts defined | T-0013B runtime and patch protocol docs define provider, session, event, checkpoint, handoff, patch, and diff contracts. | passed |
| Disclosure and network-efficiency policy defined | T-0013B documents local-only versus remote disclosure and bounded context transport. | passed |
| Execution and event schemas pass tests | `cargo test delegated -- --nocapture`, clippy, and full `cargo test` passed after T-0013B documentation changes. | passed |

Historical hard-stop condition:

The earlier OpenClaw-centered path required a hard stop when CatDesk could not prove it remained the sole execution and policy boundary. T-0013A recorded that hard stop. The approved architecture pivot makes CatDesk the owner of the model/tool loop, so the base sprint continues without OpenClaw as a required runtime.

Attempts:

- Started hardened CatDesk headless MCP in read-only mode on loopback.
- Registered CatDesk in OpenClaw using process-scoped disposable `OPENCLAW_CONFIG_PATH` and `OPENCLAW_STATE_DIR`.
- Validated disposable OpenClaw policy with only four server-qualified CatDesk MCP tools in `tools.allow`.
- Captured `openclaw mcp probe`, `openclaw config get tools`, `openclaw config get agents.list`, `openclaw agent --help`, and plugin metadata.
- Confirmed these commands do not expose the final resolved worker-visible tool definitions before a model request.

Current tests after T-0013B final verification:

- `cargo fmt --check`: passed
- `cargo test delegated -- --nocapture`: passed, 9 tests
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 129 tests

Changed files since `d0cc25e`:

- `docs/orchestrator/EXECUTION_PROTOCOL_V1.md`
- `docs/orchestrator/SPRINT_CREDITS.md`
- `docs/orchestrator/tickets/T-0013.md`
- `src/delegated/contracts.rs`
- `src/delegated/events.rs`
- `src/delegated/mod.rs`
- `src/delegated/state_machine.rs`
- `src/main.rs`
- `tests/fixtures/delegated/execution_contract_v1.json`

Recommendation:

Proceed to T-0014 only after T-0013B verification passes and a local T-0013B commit is created. Do not push, open a PR, merge, release, deploy, or publish.

Nothing was pushed, merged, published, released, deployed, or opened as a pull request.

## Milestone B - Recovery And Context

Status: PASSED

Tickets:

- T-0014: local commit `684e9d2`
- T-0015: local commit `6596392`

Required gate evidence:

| Gate condition | Evidence | Status |
| --- | --- | --- |
| Journal survives restart | T-0014 journal tests reopen the journal and restore tool, patch, run, and event state. | passed |
| Mutation replay protections pass | T-0014 rejects duplicate tool calls, completed replay, `OUTCOME_UNKNOWN` replay, and duplicate run overwrite. | passed |
| Bounded context and compaction tests pass | T-0015 context tests cover bounded excerpts, output dedupe, compaction, disclosure, and redacted inspection. | passed |
| Provider-neutral handoff passes | T-0015 builds provider-neutral handoff packets from compact checkpoints. | passed |

Current T-0014 tests:

- `cargo fmt --check`: passed
- `cargo test delegated::journal -- --nocapture`: passed, 9 tests
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 138 tests

Current T-0015 tests:

- `cargo fmt --check`: passed
- `cargo test delegated::context -- --nocapture`: passed, 7 tests
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 145 tests

## Milestone C - First Integrated Worker

Status: PASSED

Tickets:

- T-0016: local commit `d363555`
- T-0017: local commit `90ab774`
- T-0018: local commit `060eed8`

Required gate evidence:

| Gate condition | Evidence | Status |
| --- | --- | --- |
| Qwen completes a disposable patch-first task | T-0017 includes a deterministic disposable repair cycle and a live Qwen patch-proposal smoke against a bounded excerpt. | passed |
| Patch revision and comparison work | T-0017 compares a revised child patch against its parent. | passed |
| One escalation succeeds | T-0018 coordinator builds escalation packets with evidence. | passed |
| CatDesk remains the only execution boundary | T-0016 providers receive bounded context and CatDesk-owned tool schemas only. | passed |

Current T-0016 tests:

- `cargo fmt --check`: passed
- `cargo test delegated::runtime -- --nocapture`: passed, 6 tests and 1 ignored live smoke
- `cargo test delegated::runtime::tests::ollama_qwen_live_smoke_returns_normalized_response -- --ignored --nocapture`: passed, 1 live Qwen/Ollama smoke
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 151 tests and 1 ignored live smoke

Current T-0017 tests:

- Live Qwen proposal smoke: passed
- `cargo fmt --check`: passed
- `cargo test delegated::patch_engine -- --nocapture`: passed, 6 tests
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 157 tests and 1 ignored live Ollama smoke

Current T-0018 tests:

- `cargo fmt --check`: passed
- `cargo test delegated::coordinator -- --nocapture`: passed, 8 tests
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 165 tests and 1 ignored live Ollama smoke

## Milestone D - Provider And Supervisor Workflow

Status: PASSED_AFTER_T0023A_CLOSURE

Tickets:

- T-0019: local commit `eaf85e5`
- T-0020: local commit `093055f`
- T-0021: local commit `e66ae92`
- T-0022: local commit `4cf4f6e`
- T-0023: local commit `2571424`
- T-0023A: local commit `59d0634`
- T-0023B: local closure commit recorded in Git history and in the external review bundle
- T-0023C: pending local review; not pushed or opened as a pull request

Required gate evidence:

| Gate condition | Evidence | Status |
| --- | --- | --- |
| Simulated provider switch succeeds without duplicate mutation | T-0019 filters completed/failed tool calls out of handoff. | passed |
| Supervisor can poll, inspect patches/diffs, and resume | T-0020 supervisor surface supports event polling, bounded patch/diff inspection, pause, resume, cancel, and final review retrieval. | passed |
| Long-running jobs are bounded and recoverable | T-0021 records durable jobs, bounded logs, rotation, cancellation, lost-process recovery, and restart reopening. | passed |
| Fault-injection and security matrix passes | T-0022 covers malformed tool calls, forbidden operations, stale/conflicting patches, provider switch, disclosure, prompt injection, redaction, staged Git content, and no push/merge final review. | passed |
| Setup and release review are documented | T-0023 documents runtime setup, Ollama/Qwen, adapters, disclosure, patch protocol, restart recovery, long jobs, limitations, and a disposable first-run tutorial. | passed |
| Integrated delegated workflow is composed and tested | T-0023A adds the integrated service, expanded tool dispatcher, MCP transport wiring, setup scripts, live Qwen model-tool-model evidence, and review bundle. | passed |
| Production delegated-run path is wired and safety-closed | T-0023B adds an autonomous Ollama worker loop, retained provider/tool history, durable integrated state, least-privilege supervisor-only MCP startup, explicit MCP artifact errors, and live Qwen production-loop evidence. | passed |
| MCP supervisor starts the real worker path | T-0023C requires full execution contracts through `delegated_run_create`, starts `IntegratedDelegatedService` through MCP `delegated_run_start`, polls real status/events/final-review data, safely rejects malformed run IDs, and proves the live Qwen path begins only through MCP. | passed |
| Remote disclosure policy is enforced | T-0019 blocks remote/browser providers unless disclosure policy allows them. | passed |

Current T-0019 tests:

- `cargo fmt --check`: passed
- `cargo test delegated::provider_router -- --nocapture`: passed, 6 tests
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 171 tests and 1 ignored live Ollama smoke

Current T-0020 tests:

- `cargo fmt --check`: passed
- `cargo test delegated::supervisor -- --nocapture`: passed, 7 tests
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 178 tests and 1 ignored live Ollama smoke

Current T-0021 tests:

- `cargo fmt --check`: passed
- `cargo test delegated::job_manager -- --nocapture`: passed, 7 tests
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 185 tests and 1 ignored live Ollama smoke

Current T-0022 tests:

- `cargo fmt --check`: passed
- `cargo test delegated::fault_injection -- --nocapture`: passed, 4 tests
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 189 tests and 1 ignored live Ollama smoke

Current T-0023 tests:

- `cargo fmt --check`: passed
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 189 tests and 1 ignored live Ollama smoke

Current T-0023A tests:

- `cargo fmt --check`: passed
- `cargo test delegated::integrated -- --nocapture`: passed, 2 tests and 1 ignored live Qwen closure
- `cargo test supervisor_delegated_tools_are_discovered_and_invoked_through_mcp -- --nocapture`: passed, 1 MCP transport test
- `cargo test delegated::integrated::tests::ollama_qwen_live_model_tool_model_closure -- --ignored --nocapture`: passed with `CARGO_TARGET_DIR` set to a temporary path because Windows Application Control blocked the default target test executable
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 192 tests and 2 ignored live/opt-in tests

Current T-0023B tests:

- `cargo fmt --check`: passed
- `cargo test delegated::integrated -- --nocapture`: passed, 4 tests and 2 ignored live tests
- `cargo test supervisor_delegated_tools_are_discovered_and_invoked_through_mcp -- --nocapture`: passed, 1 MCP transport test
- `cargo test delegated::integrated::tests::ollama_qwen_live_production_worker_loop_closure -- --ignored --nocapture`: passed in 67.28s with `CARGO_TARGET_DIR` set to a temporary path; Qwen drove the autonomous production loop through read, patch, failed verification, revised patch, passing verification, diff capture, and completion
- `scripts/start-local-orchestrator.ps1`: passed bounded start/cleanup with `--tool-mode supervisor-only`
- `scripts/start-dev-orchestrator.ps1`: passed bounded start/cleanup with `--tool-mode supervisor-only`
- `cargo clippy --all-targets --all-features -- -D warnings`: passed
- `cargo test`: passed, 194 tests and 3 ignored live/opt-in tests

Current T-0023C tests:

- `cargo test supervisor_ -- --nocapture`: passed, including MCP discovery/invocation, malformed run ID handling, and durable journal rehydration.
- `cargo test delegated::patch_engine -- --nocapture`: passed, 6 tests.
- `cargo test delegated::integrated -- --nocapture`: passed, 4 tests and 2 ignored live tests.
- `cargo test live_qwen_delegated_run_starts_and_completes_through_mcp -- --ignored --nocapture`: passed in 72.89s with `CARGO_TARGET_DIR` set to a temporary path; Qwen was started exclusively through MCP create/start and completed with MCP-polled `COMPLETED_VERIFIED` status, journal events, and final review.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test`: passed, 196 tests and 4 ignored live/opt-in tests.
