# T-0138-R7 Qwen 3.8 Bounded Read / History Compaction Review Bundle

## Scope

This ticket closes the deterministic blocker found by the live T-0138 R6 `qwen3.8:27b` canary. R6 successfully completed multiple sequential CatDesk continuations and did **not** reproduce the historic Ollama HTTP 500 `no user query found in messages`, but it repeatedly requested the same whole-file read until CatDesk failed closed with `serialized provider input exceeded 49152 bytes after compaction`.

## Root cause

The delegated `read` tool schema already advertised `startLine` and `endLine`, but `IntegratedDelegatedService::tool_read` ignored both fields and returned the entire safe workspace file in the tool result/provider history. History compaction retained the execution-contract authority plus the last eight messages, so several repeated large read results could still exceed the existing 48 KiB provider-input ceiling. Read/search replay prevention also operated only on tool-call IDs, allowing the same semantic read-only request to be executed again under a new tool-call ID.

## Implementation

- `read` now honors `startLine`/`endLine`, defaults to a 120-line window, caps a request at 200 lines and 8 KiB of UTF-8 result text, and returns bounded continuation metadata (`startLine`, `endLine`, `nextStartLine`, `totalLines`, `truncated`).
- Recent-read context now records the bounded excerpt rather than the full source file.
- Provider-facing `search` text is bounded to 8 KiB while retaining the existing bounded search-result policy.
- Completed semantic `read`/`search` requests are recognized durably by tool name plus arguments hash. A repeated request is suppressed without filesystem/search re-execution and receives a bounded non-authority result instructing the worker to change its bounded arguments if more context is needed.
- Qwen 3.8 retains the existing deterministic non-authority continuation after a successful/suppressed read-only result.
- The existing provider-input ceiling remains exactly 48 KiB (`48 * 1024`); this ticket does not increase it.
- The read tool description now explicitly tells local workers to advance through large files with `startLine`/`endLine`.

## Deterministic verification

A new regression, `qwen38_bounded_reads_suppress_duplicate_replay_and_compact_under_limit`, creates a ~900-line source file and proves:

1. a requested range produces a bounded result with continuation metadata;
2. the same completed semantic read under a different tool-call ID is suppressed without re-execution;
3. successive distinct bounded ranges remain usable;
4. compaction keeps serialized provider history at or below the existing 48 KiB ceiling;
5. the genuine user task query remains present; and
6. tool-result history remains present after compaction.

During implementation, an initial hard-error form of duplicate suppression exposed a regression in an existing two-failed-repairs/advisor test. The implementation was corrected to suppress the duplicate read-only operation without failing the worker. The targeted integrated test suite then passed.

Final project verification after formatting:

- `cargo fmt --check`: PASSED
- `cargo test`: PASSED
- `cargo build`: PASSED

## Security and authority review

No provider authority was added. The local model still cannot grant itself paths, tools, command profiles, mutation rights, or larger budgets. Mutating tool semantics are unchanged. Semantic suppression applies only to `read` and `search`. No unrestricted shell, Codex/cloud fallback, browser/wake, Scheduler, Secure MCP ownership change, Git publication, reset, or cleanup was used. The externally owned Secure MCP tunnel remained outside this implementation.

## Remaining live acceptance

This deterministic fix is not sufficient by itself to declare Qwen production-ready. The next acceptance step is to build an isolated candidate, load it through the reviewed CatDesk daemon reload path without canonical promotion, require `CONNECTED_VERIFIED`, and run one fresh `qwen3.8:27b` delegated canary. That canary must complete several sequential bounded read/search continuations, avoid semantic replay and the 49,152-byte compaction failure, perform its single approved review-bundle mutation, pass verification, capture authoritative diff, and reach final review. The already-failed R6 run must not be retried or reused.
