# T-0035-R3 controller-time graph consistency review bundle

## Repair

T-0079 validated a non-empty approved `taskGraph` in supervisor validate,
approve, and start operations, but the controller had a separate, weaker
queue-only launch check. A durable plan could therefore be changed between
start and a provider turn without being compared to the approved contract.

R3 adds `validate_exact_task_graph_materialization` in the durable autonomy
state layer. It validates the approved contract, queue, and plan as one exact
materialization: task count/set, IDs, queue priority/dependencies, plan
dependencies/acceptance criteria, contract-wide allowed paths, and verification
profile. Extra, missing, duplicate, malformed, and mismatched records fail
closed.

The supervisor now delegates validate/approve/start graph checking to this
same shared validator. The controller invokes it before stale-worker recovery
and again immediately before selecting or resuming a task. It escalates a
non-empty graph to bounded ChatGPT review before provider launch, repair
accounting, or task-state mutation if the durable materialization is missing
or differs. After validation, task criteria are sourced from the approved
contract spec, never from mutable plan metadata. Empty `taskGraph` retains the
historical ordered-step fallback and serialization behavior.

## Coverage

Focused controller regression coverage simulates approved/started A -> {B,C}
-> D state and proves zero provider turns/handles plus bounded escalation for
mutated plan criteria, paths, verification profile, dependencies, queue
priority/dependencies, and extra/missing queue tasks. The unchanged graph
progresses in dependency order through D. Existing WAITING/reply/manual resume,
task-repair/accounting, rate-limit/restart, Qwen, Terra/High, and legacy tests
remain in the full suite.

## Changed files

- `src/delegated/autonomy_state.rs`
- `src/delegated/autonomy_supervisor.rs`
- `src/delegated/autonomous_controller.rs`
- this review bundle

## Local verification

After formatting, run `cargo fmt -- --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, and
`git diff --check`. No live DAG, browser/daemon/reload/tunnel/release action,
credential access, or Git publication is part of this repair. CatDesk retains
independent verification and authoritative diff capture.

Completed locally: formatter and strict clippy passed; the full test suite
reported 503 passed, 18 ignored, and 0 failed; `git diff --check` passed.
