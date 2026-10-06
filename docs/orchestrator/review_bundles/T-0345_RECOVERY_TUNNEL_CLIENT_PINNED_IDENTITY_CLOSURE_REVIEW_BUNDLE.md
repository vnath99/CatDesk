# T-0345 — Recovery Tunnel-Client Pinned Executable Identity Closure

## Scope

Close the remaining executable-identity seam in `scripts/start-catdesk-stack.ps1` default tunnel-client discovery without changing operator override semantics or CatDesk ownership boundaries.

T-0341 removed caller `PATH` from recovery tunnel-client selection, but the default CatDesk user-local `current` and legacy candidates were still accepted after `Test-Path`/`Resolve-Path` without an explicit leaf-reparse rejection or exact expected-vs-observed full-path identity check. Because the selected client participates in official Secure MCP runtime verification during `plan`/`recover`, that left a durable recovery authority boundary dependent on filesystem redirection semantics.

## Implementation

`Find-TunnelClient` now distinguishes two authority classes:

1. An explicit `-TunnelClientPath` remains an intentional operator override and preserves the established behavior.
2. Default CatDesk-owned user-local candidates (`~/.catdesk/tools/tunnel-client/current/tunnel-client.exe` and the legacy pinned location) must:
   - exist as a leaf;
   - materialize as `System.IO.FileInfo`;
   - reject `FileAttributes.ReparsePoint`;
   - derive both expected and observed identities through `System.IO.Path.GetFullPath`;
   - compare those full paths with `StringComparison.OrdinalIgnoreCase` before execution authority is returned.

No caller `PATH` discovery was reintroduced. No runtime mutation authority was added.

## Regression coverage

The existing `scripts/test-start-catdesk-stack.ps1` T-0341 tunnel-client discovery fixture was extended rather than creating a parallel recovery harness. It now preserves the existing explicit/current/legacy/fail-closed and PATH-shadow checks and adds deterministic source guards requiring:

- reparse-point rejection;
- full-path normalization;
- exact case-insensitive identity comparison;
- file-identity inspection through `Get-Item -LiteralPath ... -Force`.

This keeps the test deterministic on Windows hosts where creating symbolic links/reparse points can require host-specific privileges.

## Verification

Repository verification completed successfully after the implementation:

- sanctioned project verifier: formatting, full Cargo test suite, and build passed;
- `cargo clippy --all-targets --all-features -- -D warnings`: passed;
- `git diff --check`: passed.

A fresh dedicated nested-PowerShell recovery-harness pass is not claimed by this implementation record. The command-policy boundary must not be bypassed merely to manufacture acceptance evidence; approved execution remains part of the independent T-0319 acceptance boundary where required.

## Preserved boundaries

- The already-running official Secure MCP runtime remains externally owned; no duplicate runtime was created, restarted, migrated, or replaced.
- The dirty worktree remains intentionally preserved.
- No browser wake bridge was manually invoked.
- No protected wake-target state was edited.
- No Scheduler mutation, release promotion, or Git publication occurred.
- Codex/Terra-high reset-aware blocking was honored; no repeated provider probe was used to force availability and Qwen was not dispatched for substantive implementation.

## Acceptance status

Implementation and repository verification for T-0345 are complete. This record is not an independent final acceptance of the broader recovery chain.

T-0319 remains open for a genuinely separate reviewer to assess the cumulative recovery-hardening lineage through T-0345, including any acceptance-only Windows recovery-harness execution that policy permits. T-0324 remains separately gated on reviewed build/promotion followed by its bounded live Qwen continuation canary.
