# T-0035-R1 first-class DAG and planner-gate review bundle

## Contract schema and compatibility

AutonomousDevelopmentContractV1 now has an optional taskGraph field. It uses
serde default plus skip-when-empty behavior, so legacy single-task contracts
remain readable and serialize/hash exactly as before. Each graph task binds its
task ID, positive priority, unique dependencies, bounded non-empty acceptance
criteria, and optional planner gate into the approved contract hash.

Validation rejects duplicate IDs/dependencies, missing/self dependencies,
unsafe or oversized text, and cycles with deterministic dependency reduction.
The durable queue independently rejects duplicate/missing/cyclic dependencies
as defense in depth.

## Queue seeding and task execution

autonomy_contract_create derives both execution queue and plan metadata from
the approved graph. A legacy empty graph derives the original one-task queue
and original ordered-step acceptance criteria. Graph queue tasks begin READY,
with the existing dependency-satisfaction selection remaining authoritative.

The controller loads plan metadata for the selected task and includes its
bounded task-specific acceptance criteria in every provider instruction. It
only treats a provider continuation as a repair when the selected ready task
is the persisted current task. A different selected task resets repair state
before its turn, while accounting remains keyed by session and task.

## Planner gate and completion safety

A task with an unsatisfied approved planner gate persists as current while
remaining READY, emits planned_architecture_decision_required once, and waits
for ChatGPT without launching a provider. Gate satisfaction is a durable
task-scoped artifact, not inferred from history. Only a reply to that exact
planned escalation can satisfy it; terminal provider escalations cannot.

After verification, final completion is permitted only when every queue task
is COMPLETED_VERIFIED. A remaining dependency-unrunnable task escalates rather
than producing a premature final review.

## Coverage

Focused tests cover legacy empty graph serialization/hash, DAG validation,
A to B/C to D deterministic queue progression, cycle rejection, and durable
task-scoped planner-gate satisfaction. Existing controller, accounting,
single-task, routing, and W13 code paths remain compiled under the full suite.

## Changed files

- src/delegated/autonomous_contract.rs
- src/delegated/autonomy_state.rs
- src/delegated/autonomy_supervisor.rs
- src/delegated/autonomous_controller.rs
- this review bundle

## Local verification

Run cargo fmt -- --check, cargo clippy --all-targets --all-features -- -D
warnings, cargo test, and git diff --check. CatDesk must perform the specified
live acceptance only through autonomy_contract_create, followed by the
supported candidate lifecycle and a normal automatic wake. No daemon reload,
browser action, protected state edit, or live DAG was performed by this worker.

Completed locally: cargo fmt and clippy passed; cargo test reported 500 passed,
18 ignored, and 0 failed; git diff --check passed. CatDesk independently owns
the live DAG acceptance and authoritative diff capture.
