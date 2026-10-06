# T-0035-R2 transactional DAG integrity review bundle

## Root causes and repair

R1 allowed task-scoped planner-gate satisfaction to be written outside the
reply mutation lock. It also permitted a WAITING_FOR_CHATGPT session to proceed
after a gate artifact existed. R2 makes WAITING_FOR_CHATGPT a hard controller
no-provider state. Only the supported reply path can requeue it.

The reply path now owns the session lock from snapshot/version/idempotency
checks through escalation/task-graph validation, durable planner-reply write,
task-scoped gate write, session requeue/idempotency persistence, and event
append. The exact escalation ID is required. For a planned gate, the current
task must match an approved taskGraph gate. Failure after reply or gate remains
WAITING_FOR_CHATGPT because state requeue is last; a same-key retry can
converge without launching a provider.

Manual resume rejects a waiting task whose current approved task has a planner
gate, so it cannot bypass the matching reply path.

## Graph materialization integrity

Non-empty graphs are checked against durable queue and plan metadata at
validate, approve, start, and controller launch boundaries. IDs, priorities,
dependencies, criteria, allowed paths, and verification profile must match the
approved graph exactly. The controller retains ordered-step fallback only for
legacy empty graphs; graph plan loss/mismatch escalates before provider launch.

Creation derives queue and plan in memory first. If contract, plan, or event
persistence fails after draft creation, the unapproved draft session is
removed; such partial state cannot become approval-able.

## Scope

Changed: autonomous_controller.rs, autonomy_supervisor.rs,
autonomy_state.rs, and this bundle. W13, wake behavior, accounting,
provider routing, and live lifecycle behavior remain unchanged.

## Verification

Run cargo fmt -- --check, cargo clippy --all-targets --all-features -- -D
warnings, cargo test, and git diff --check. CatDesk must independently run
the post-review real DAG only through autonomy_contract_create. No live DAG,
daemon reload, browser action, or protected-state edit was performed here.

Completed locally: cargo fmt and clippy passed; cargo test reported 501 passed,
18 ignored, and 0 failed; git diff --check passed. CatDesk owns independent
verification and authoritative diff capture.
