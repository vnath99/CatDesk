# Durable Journal And Idempotency

Status: T-0014 implementation
Date: 2026-07-25

## Scope

T-0014 adds a small file-backed delegated-run journal. It is intentionally dependency-free and stores durable state as JSON snapshots plus append-only JSONL events under a caller-supplied root directory.

No database file is required or committed to Git.

## Persistence Layout

Each run is stored in a conservative hashed directory derived from `RunId`:

```text
<journal-root>/
  <safe-run-id>-<hash>/
    contract.json
    state.json
    events.jsonl
    tool_calls.json
    patch_proposals.json
    patch_applications.json
```

The caller owns the journal root. Tests use temporary directories outside the repository. A future coordinator ticket can choose the production root and add lifecycle cleanup.

## Durable Records

Current T-0014 records:

- execution contract snapshot;
- run state snapshot;
- ordered event cursor;
- tool-call lifecycle records;
- patch proposals;
- patch application results;
- patch and result hashes.

The file format leaves room for T-0015 through T-0021 to add provider requests, checkpoints, verification artifacts, escalations, supervisor decisions, long-running jobs, and provider transitions without changing the core replay rules.

## Tool-Call Lifecycle

```text
REQUESTED
-> POLICY_ALLOWED
-> APPROVAL_REQUIRED
-> APPROVED
-> EXECUTING
-> COMPLETED | FAILED | OUTCOME_UNKNOWN
```

Allowed transitions are enforced before state is written. Duplicate `tool_call_id` values fail. Terminal tool calls do not transition back to execution.

## Replay Safety

Completed tool calls return `CompletedToolCallWillNotReplay` if execution is attempted again after restart. `OUTCOME_UNKNOWN` tool calls return `OutcomeUnknownRequiresSupervisor`; CatDesk must not replay the mutation automatically.

Run creation is also guarded. Creating the same run twice fails instead of overwriting existing tool, patch, or event records.

## Patch Lineage

Patch proposals use stable `patch_id` values and optional `parent_patch_id` values. A child patch must reference an existing proposal. Patch applications must reference a known proposal and persist:

- status;
- tool-call ID;
- actual changed paths;
- before and after hashes;
- result hash;
- diff artifact ID.

T-0017 will build the full patch/diff engine on this durable foundation.

## Restart Restore

`restore_active_runs` scans journal state snapshots and returns only non-terminal active runs. Terminal states are not restored for execution.

`poll_events` reloads append-only JSONL events and applies the T-0013 cursor semantics after restart.

## Implementation

Rust module:

- `src/delegated/journal.rs`

Protocol additions:

- `PatchId` in `src/delegated/contracts.rs`
- `journal` module exported from `src/delegated/mod.rs`

## Verification Coverage

Focused tests cover:

- duplicate run creation does not overwrite a journal;
- duplicate tool-call IDs fail;
- completed mutations are not replayed after restart;
- `OUTCOME_UNKNOWN` requires supervisor handling after restart;
- patch and result hashes survive restart;
- patch applications require known proposals;
- child patches require existing parents;
- active runs restore after restart;
- event cursors survive restart.
