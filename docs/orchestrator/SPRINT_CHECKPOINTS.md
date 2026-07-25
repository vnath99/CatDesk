# Sprint Checkpoints

Date: 2026-07-25
Branch: orchestrator/v1-coding-sprint

## Milestone A - Architecture Pivot And Core Protocol

Status: PASSED_AFTER_PIVOT

Tickets:

- T-0012: local commit `d0cc25e`
- T-0013: local commit `53e06a4`
- T-0013A: local commit `35e23dc`
- T-0013B: local commit to be recorded in Git history after this checkpoint update

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

Status: IN_PROGRESS

Tickets:

- T-0014: local commit to be recorded in Git history after this checkpoint update
- T-0015: local commit to be recorded in Git history after this checkpoint update

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
