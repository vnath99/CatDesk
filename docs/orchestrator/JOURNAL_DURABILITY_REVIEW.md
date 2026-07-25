# Journal Durability Review

Status: T-0023A review
Date: 2026-07-25

## Reviewed Properties

| Property | Current State | Risk | Follow-Up |
| --- | --- | --- | --- |
| Atomic snapshot replacement | JSON snapshots are written to a temporary file, flushed with `sync_all`, then renamed. | Acceptable for single-process local use. Cross-platform rename semantics are relied on. | Keep; revisit if journal moves to shared/network filesystems. |
| Partial JSONL recovery | Event JSONL reads currently deserialize all lines and do not skip a torn trailing line. | A crash during event append can make event replay fail closed instead of recovering all complete prior events. | Add tolerant JSONL replay or move event writes to SQLite transactions before multi-process production use. |
| Cross-process/run locking | Coordinator has in-process run locks. Journal writes do not currently hold OS file locks. | Two CatDesk processes can race on the same journal root. | Required before production multi-process use: OS lock file or SQLite transaction lock. |
| Durable ordering between mutation result, event, patch result, and run state | T-0023A integrated tool calls now persist requested/policy/executing/completed tool-call state before appending completion events. Patch and run-state ordering is still composed at service level rather than one transaction. | Crash between records can require supervisor reconciliation. | Prefer SQLite for mutation/event/patch/run-state transaction groups. |
| Schema-version migration | Records include schema versions. There is no migration runner. | Future schema changes can require manual migration. | Add explicit migration plan before changing persisted schema. |
| Flush behavior at mutation boundaries | Snapshot writes call `sync_all`; event appends call `sync_data`. | Reasonable for local single-process use, but no directory fsync after rename. | Add directory fsync where supported or use SQLite. |

## Recommendation

The current JSON/JSONL journal is acceptable for the disposable local v1 spike
and deterministic tests. It is not yet sufficient for production multi-process
mutation safety.

Before enabling delegated runs as a long-lived production workflow, prefer a
SQLite-backed journal with transactions covering:

- tool-call state transition;
- tool result hash and summary;
- event append;
- patch proposal/application record;
- run-state checkpoint update.

The closure branch does not add SQLite because the current ticket scope is to
prove the integrated workflow locally and produce review evidence, not to
replace the storage engine in the same pass.
