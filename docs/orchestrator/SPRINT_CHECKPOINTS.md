# Sprint Checkpoints

Date: 2026-07-25
Branch: orchestrator/v1-coding-sprint

## Milestone A - Runtime Proof And Core Protocol

Status: HARD_STOP

Tickets:

- T-0012: local commit `d0cc25e`
- T-0013: local commit `53e06a4`

Required gate evidence:

| Gate condition | Evidence | Status |
| --- | --- | --- |
| Runtime ownership proven | T-0012 docs show CatDesk headless MCP can run read-only on loopback, and OpenClaw can discover four filtered CatDesk tools through disposable config. | partial |
| One full local model/tool/model turn demonstrated or hard stop recorded | T-0012 synthetic Qwen/Ollama tool-call probe succeeded, but not through OpenClaw worker plus CatDesk MCP. | partial |
| Effective worker-visible tool policy verified | T-0012 closure audit found OpenClaw CLI exposes MCP discovery, config policy, and plugin metadata, but not final worker-visible tool definitions before a model turn. | failed |
| Event access proven | Not proven through OpenClaw worker runtime; T-0013 defines CatDesk-side event protocol only. | failed |
| Session persistence tested | Not proven through OpenClaw worker runtime; no model worker was run. | failed |
| Execution and event schemas pass tests | `cargo test delegated -- --nocapture`, clippy, and full `cargo test` passed for T-0013. | passed |

Hard-stop condition:

The sprint roadmap requires a hard stop when CatDesk cannot prove it remains the sole execution and policy boundary, or when OpenClaw cannot expose sufficient session/event control. Current evidence does not prove the effective worker-visible OpenClaw tool list, structured worker event access, or restart/resume control before a model worker run.

Attempts:

- Started hardened CatDesk headless MCP in read-only mode on loopback.
- Registered CatDesk in OpenClaw using process-scoped disposable `OPENCLAW_CONFIG_PATH` and `OPENCLAW_STATE_DIR`.
- Validated disposable OpenClaw policy with only four server-qualified CatDesk MCP tools in `tools.allow`.
- Captured `openclaw mcp probe`, `openclaw config get tools`, `openclaw config get agents.list`, `openclaw agent --help`, and plugin metadata.
- Confirmed these commands do not expose the final resolved worker-visible tool definitions before a model request.

Current tests:

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

Pause before T-0014. Choose one of these options:

1. Approve a targeted OpenClaw runtime-inspection spike that may run a disposable local model worker with Qwen/Ollama, still with no credentials, no provider setup, no Gateway service install, and no persistent OpenClaw onboarding.
2. Change the architecture to avoid OpenClaw for v1 worker execution unless its worker-visible tool policy and event cursor can be inspected.
3. Accept OpenClaw as an unproven orchestration layer and continue protocol/storage tickets only, explicitly deferring integrated worker execution. This changes the Milestone A gate and should be treated as architecture approval.

Nothing was pushed, merged, published, released, deployed, or opened as a pull request.
