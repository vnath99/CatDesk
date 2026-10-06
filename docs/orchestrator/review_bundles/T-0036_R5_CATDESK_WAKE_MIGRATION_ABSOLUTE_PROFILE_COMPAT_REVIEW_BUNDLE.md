# T-0036-R5 CatDesk Wake Migration Absolute-Profile Compatibility Review Bundle

## Root cause and minimal correction

T-0036-R3's CatDesk-only compatibility migration rejected every absolute `profile_dir` before it evaluated whether the path was safe. The existing dedicated wake profile is absolute but resolves under the exact canonical `.catdesk/wake-bridge` root, so migration failed before the canonical CatDesk project target could be persisted.

The validator now preserves relative in-root support and accepts an absolute profile only when it is an existing safe directory whose canonical path is contained by the exact canonical wake root. The fixed `config.json` and profile directory are read only as bounded metadata; direct symlink/reparse points, non-directories, parent traversal, outside-root paths, malformed JSON, oversized config, and canonicalization ambiguity are rejected. No browser profile contents or credentials are read.

## Security and wake invariants

The migration remains CatDesk-only, one-time, and durable. It never migrates an external project, never overwrites an existing project target, and does not create a runtime global fallback. Project-scoped target selection, W13 single-submit behavior, R7 readiness ownership, exact conversation identity, schema-4/receipt-1 validation, and fail-closed post-boundary handling are unchanged.

## Deterministic coverage

Tests cover the live-compatible absolute in-root profile shape, relative in-root success, outside-root absolute rejection, parent traversal rejection, symlink escape rejection on supported platforms, malformed and oversized configs, preserved already-bound target, CatDesk-only migration, digest consistency, and existing exact receipt regressions.

## Fresh canary procedure

CatDesk should load the reviewed candidate, allow one fresh `COMPLETED_VERIFIED` review record to enter normal automatic project-scoped dispatch, and inspect only the resulting durable canonical target and schema-4 `SENT` receipt. It must not retry or reuse T-0093's review record. This worker did not run a browser, wake bridge, app-server, external repository, tunnel, Scheduler, daemon reload, release, or Git publication action.
