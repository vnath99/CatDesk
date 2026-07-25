# Supervisor MCP Surface

Status: T-0020 implementation
Date: 2026-07-25

## Scope

T-0020 defines the ChatGPT supervisor-facing delegated-run tool surface and tests its local semantics. The module is transport-neutral so the same behavior can be exposed through CatDesk MCP without changing event, artifact, patch, or review rules.

## Tools

- `delegated_run_create`
- `delegated_run_validate`
- `delegated_run_start`
- `delegated_run_status`
- `delegated_run_list`
- `delegated_run_events`
- `delegated_run_get_checkpoint`
- `delegated_run_get_escalation`
- `delegated_run_get_artifact`
- `delegated_run_get_patch`
- `delegated_run_compare_patches`
- `delegated_run_get_diff`
- `delegated_run_resume`
- `delegated_run_pause`
- `delegated_run_cancel`
- `delegated_run_get_final_review`

## Event Polling

Event polling uses the existing T-0013 cursor behavior:

- events are sorted by `event_sequence`;
- `after_sequence` is exclusive;
- `limit` bounds returned events;
- repeated polling with the same cursor is idempotent.

## Bounded Retrieval

Artifacts and diffs are retrieved through bounded text responses. Large logs remain local and return a truncation marker instead of flooding a provider or supervisor turn.

## Patch Lineage

Patch lookup and comparison reuse T-0017 patch records and comparison logic. This lets the supervisor inspect parent and candidate patches without granting the model mutation authority.

## Escalation Decisions

Escalation decision IDs are one-shot. Reuse returns an error.

## Implementation

Rust module:

- `src/delegated/supervisor.rs`

Export:

- `src/delegated/mod.rs`

## Verification Coverage

Focused tests cover:

- required tool-name list;
- ordered idempotent event polling;
- bounded artifact and diff retrieval;
- patch lineage inspection;
- escalation decision reuse rejection;
- pause, resume, cancel, checkpoint, escalation, and final-review retrieval.
