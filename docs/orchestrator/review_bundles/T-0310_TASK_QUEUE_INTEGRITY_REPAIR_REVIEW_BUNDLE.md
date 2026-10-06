# T-0310 Task-Queue Integrity Repair Review Bundle

## Scope and partial-run attribution

T-0310-R2 finalizes a partial, data-only repair left by the original T-0310
session. The pre-existing partial delta had already changed the stale marker
from `297` to `311` and converted the earlier unchecked T-0296 contract into
clearly labelled non-checkbox historical prose. This continuation inspected
that delta rather than reapplying it.

No product source changed. In particular, `src/task_queue.rs` remains the
authority for strict `T-####` parsing and duplicate-ID rejection.

## Before and after semantics

Before repair, the queue contained two parseable checkbox records with stable
ID `T-0296`: the original unchecked contract and the later checked completion
correction. `task_queue::parse_tasks` deliberately rejects that state with:

`Duplicate task ID found in .catdesk/todo.md: T-0296`

The original partial repair retained every word of that first contract but
made it `Historical/superseded contract (not a checkbox task)`. The later
checked T-0296 correction is now the sole parseable T-0296 and is done.

The next strict parser-equivalent readback surfaced the same historical
pattern for T-0298. Its earlier unchecked contract was likewise converted to
labelled non-checkbox prose, retaining the text; the later checked T-0298
completion remains its sole task. This additional normalization was necessary
for the queue to become readable and did not weaken or alter parser policy.

The marker is exactly `<!-- catdesk-next-task-id: 311 -->`. A checked T-0299
record was already present before this repair; T-0300 through T-0309 are
consumed durable project IDs, not missing queue tasks, and none was fabricated
as backfill. With the checked T-0310 record present, the next stable generated
ID is `T-0311`.

## Final readback and verification

The provider environment has no callable CatDesk MCP tool, so it did not use a
live MCP transport. The same fixed `task_queue.rs` parsing grammar was applied
to the workspace file for bounded readback, while the dedicated handler is
covered by the existing focused MCP/task-queue regression. Independent CatDesk
verification must invoke `task_queue_read` on this final workspace state.

Final parser-equivalent readback requirements and result:

- no duplicate parseable checkbox IDs;
- exactly one `T-0296`, checked/done;
- the pre-existing T-0299 record is retained, and no T-0300 through T-0309
  queue entries were fabricated;
- exactly one checked `T-0310` repair record;
- marker `311`; maximum current parseable ID `T-0310`; calculated next ID
  `T-0311`.

Observed result: `total=270; duplicates=0; t0296_count=1; t0296_done=x;
preexisting_t0299=1; fabricated_t0300_to_t0309=0; t0310_count=1;
t0310_done=x; marker=311; max_task=T-0310; next_generated=T-0311`.

Focused verification run in this continuation:

- `cargo test --workspace --all-targets --all-features task_queue` — PASS:
  5 focused tests, including `duplicate_task_ids_are_rejected`,
  `add_uses_monotonic_task_marker_after_manual_deletion`, and the MCP
  task-queue read/add/complete regression.
- `cargo fmt --all -- --check` — recorded after final documentation edits.
- `git diff --check` — recorded after final documentation edits.

## Source decision and prohibited-action audit

There was no source defect: duplicate task IDs intentionally fail closed and
only literal `T-` plus four digits are parsed as task IDs. No auto-merge,
last-writer-wins behavior, or parser relaxation was added.

Only these T-0310 files are attributed:

- `.catdesk/todo.md`
- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- this bundle

The accumulated dirty worktree is preserved. No host action, bootstrap,
signing/provenance/dedicated-producer work, browser/wake/target/tunnel action,
Option A implementation, external-project action, unrestricted shell, Git
publication, reset, clean, or revert occurred.

## Residual boundary and next safe ticket

T-0309 is accepted as `EXISTING_AUTHORITIES_PURPOSE_BOUND_INADEQUATE`. The
operator selected Option A: only a future separately reviewed implementation
may add the fixed `core-host-gate-approval-v1` domain under the existing
acknowledged independent-review authority. Ordinary acknowledgement remains
insufficient by itself.

T-0223 remains `OPERATOR_BOOTSTRAP_REQUIRED`; T-0222/T-0139 literal host
proof, T-0152, and T-0155 remain parked in order. The next safe repository
ticket, after independent review, is the bounded Option-A receipt-domain
implementation design/authority ticket; it must not activate host state or
interpret ordinary review acknowledgement as approval.
