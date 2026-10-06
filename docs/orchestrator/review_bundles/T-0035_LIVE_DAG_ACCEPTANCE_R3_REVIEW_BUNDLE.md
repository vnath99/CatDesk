# T-0035 live DAG acceptance R3 review bundle

## Acceptance sequence

The fresh R3 acceptance fixture sequence exercised the approved dependency
graph `A -> {B, C} -> D`. A completed before B and C became eligible; D was
reserved for completion only after both B and C satisfied their dependencies.

B reached its planned `WAITING_FOR_CHATGPT` gate before any B provider launch.
The matching persisted planner reply was `Approved`; the accepted R4/R5
continuation path re-armed the existing session automatically, without a
second `autonomy_session_start` or manual resume. The normal CatDesk wake path
owns notification of that decision boundary.

The provider continuity record for A, B, C initial, C repair, and D is the
same canonical project-bound Codex GPT-5.6 Terra/High thread. B is a new DAG
task, not a repair. C alone intentionally produced one verification failure
by changing its R3 marker from `PASS` to `FAIL`; CatDesk then invoked exactly
one same-task C repair, which restored `FAIL` to `PASS`. No C self-repair was
performed on the initial turn.

Per-task accounting remains distinct for A, B, C initial, C repair, and D;
the C repair is attributed to C rather than to B or a new task. CatDesk's
durable accounting and event records remain the authoritative evidence for
turn/thread identity, gate/reply/rearm ordering, dependency progression, and
repair cardinality.

## Fixture cleanup

The temporary T-0035 acceptance fixtures from the original, R2, and R3
canaries were removed at D:

- `tests/t0035_live_dag_canary.rs`
- `docs/orchestrator/t0035_live_dag_marker.txt`
- `tests/t0035_live_dag_canary_r2.rs`
- `docs/orchestrator/t0035_live_dag_marker_r2.txt`
- `tests/t0035_live_dag_canary_r3.rs`
- `docs/orchestrator/t0035_live_dag_marker_r3.txt`

No substantive product source was changed and no Git publication occurred.

## Final ownership

CatDesk owns the final `rust_full` verification, independent final review, and
the normal R7/W13 automatic final wake. Those checks must confirm the durable
acceptance evidence before this live DAG acceptance is considered verified.
