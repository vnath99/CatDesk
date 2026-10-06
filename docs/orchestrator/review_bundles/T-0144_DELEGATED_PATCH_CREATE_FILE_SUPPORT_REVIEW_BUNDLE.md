# T-0144 Delegated Patch Create-File Support — Independent Review Bundle

## Defect reproduced

The T-0138 Qwen 3.8 R5 live canary reached `patch.preview` after multiple successful `qwen3.8:27b` tool continuations, but both attempts to preview a new review-bundle file failed with Windows `os error 2`. The delegated patch path assumed every target already existed: `IntegratedDelegatedService::patch_from_args` and `PatchEngine` read every target before preview. This blocked an otherwise healthy Qwen continuation from creating its single permitted review artifact.

## Bounded fix

The delegated patch engine now supports one explicit create-only operation encoded as a single operation whose `old` value is the empty string. Create-only authority is deliberately narrow:

- exactly one patch operation;
- target must be an approved workspace-relative path;
- target must be positively absent during preview and again during apply;
- expected preimage hashes must be empty for create-only proposals;
- an existing file, symlink, dangling symlink, Windows reparse point, directory, or other non-regular target is rejected;
- create apply uses create-if-absent semantics and cannot overwrite a target that appears after preview;
- existing replacement-patch semantics and preimage hashes remain unchanged;
- present-file base snapshot encoding remains compatible with existing persisted patch proposals;
- the tool schema explicitly tells local workers that empty `old` is create-only and never overwrites.

`IntegratedDelegatedService::patch_from_args` now obtains base/preimage evidence through the same `capture_patch_preimage` helper used by the engine, so absent approved targets can be previewed without weakening stale-base checks.

## Deterministic verification

Added/updated regression coverage proves:

1. create-only preview succeeds for a positively absent approved target;
2. apply creates the file and authoritative diff records it as an untracked/new file;
3. a file created between preview and apply causes fail-closed stale-base rejection and is not overwritten;
4. dangling symlink targets are rejected where the platform test seam is available;
5. a create-only preview persists through delegated-service restart, recovers with its absent-file preimage authority intact, and applies exactly once;
6. the existing replacement-patch suite remains passing.

Host verification after the restart test:

- `cargo fmt --check`: PASS
- `cargo test`: PASS
- `cargo build`: PASS

## Attribution / dirty-workspace note

The repository already contains extensive pre-existing dirty changes from the broader CatDesk program. T-0144-attributable production changes are restricted to:

- `src/delegated/patch_engine.rs`: create-only preimage/apply/path-safety support plus create tests;
- `src/delegated/integrated.rs`: shared preimage capture in `patch_from_args` plus create-preview restart regression;
- `src/delegated/runtime.rs`: explicit create-only tool-schema description;
- this review bundle.

The pre-existing Qwen 3.8 continuation-user-message implementation visible in `src/delegated/integrated.rs` predates T-0144 and is not claimed as part of this ticket.

## Security / runtime invariants

No Secure MCP tunnel mutation, browser/wake action, Scheduler change, external-project mutation, canonical release promotion, Git publication, workspace reset, or cleanup was performed. Existing external Secure MCP ownership remains unchanged.

## Review disposition

**ACCEPTED FOR ISOLATED LIVE CANARY.** The code compiles, full Rust verification passes, and deterministic tests cover the R5 failure shape plus preview/apply race and restart persistence. The next allowed action is an isolated candidate build/reload followed by one fresh `qwen3.8:27b` T-0138 canary. Canonical promotion remains forbidden until that canary passes.
