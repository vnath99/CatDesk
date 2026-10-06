# T-0057 Safe Storage Cleanup Review Bundle

## Delivered

- Added `scripts/cleanup-catdesk-storage.ps1`, a two-phase plan/execute cleanup
  command with a digest-bound confirmation token.
- Added `scripts/test-cleanup-catdesk-storage.ps1` deterministic fixtures for
  allowlisting a Cargo-like tree and retaining protected-marker and unknown
  trees.
- Produced the pre-manifest, execution receipt, post-cleanup inventory, and
  operator-readable cleanup report.

## Safety proof

The plan phase canonicalizes each candidate under the workspace and requires
multiple Cargo/build signatures. It rejects reparse points, protected roots and
markers, unknown data, and a potentially live CatDesk runtime. The execute
phase requires the same workspace fingerprint, confirmation token, digest,
candidate structure, and measured byte count before each removal. A locked or
access-denied root is recorded rather than bypassed.

This cleanup never targets `.git`, `target\release`, `.catdesk\autonomy`,
`.catdesk\wake-bridge`, `.catdesk\restart-handoff`, active planning/state files,
projects, logs, or browser/profile state. It makes no daemon, tunnel, browser,
provider, account, credential, or network call.

## Evidence

- Pre-manifest: `docs/orchestrator/T-0057_SAFE_STORAGE_CLEANUP_MANIFEST_PRE.json`
- Execution receipt: `docs/orchestrator/T-0057_SAFE_STORAGE_CLEANUP_MANIFEST_EXECUTION.json`
- Post-audit: `docs/orchestrator/T-0057_STORAGE_INVENTORY_RETENTION_POST.json`
- Summary: `docs/orchestrator/T-0057_SAFE_STORAGE_CLEANUP_REPORT.md`

The execution receipt records 26 `REMOVED` entries totaling 62.84 GiB and no
locked/access failures. The post-audit shows 8.617 GiB remaining and confirms
the protected roots remain present. `target\debug` was retained fail-closed
because process inspection was not permitted.

## Exact disposition

Removed exact roots (all are also recorded with bytes in the execution receipt):

```text
.catdesk\reload-target
.catdesk\t0046b-accept-target
.catdesk\t0047b-final-target
.catdesk\t0047d-final-auth-runtime-a
.catdesk\t0047d-final-auth-runtime-b
.catdesk\t0047d-verified-daemon
.catdesk\t0047e-safe-forward-daemon
.catdesk\t0047-final-target
.catdesk\t0048b-server-owned-reload
.catdesk\t0048c-bootstrap-relay
.catdesk\t0048c-headless-daemon
.catdesk\t0048d-native-reload
.catdesk\t0048e-bootstrap-watchdog
.catdesk\t0048e-exact-pid-reload
.catdesk\t0048-final-proven
.catdesk\t0048g-listener-table
.catdesk\t0048h-windows-teardown
.catdesk\t0048i-windows-release-window
.catdesk\t0048j-noninheritable-listener
.catdesk\t0048-native-reload-bootstrap
.catdesk\t0048-native-reload-proof
target\t0040-r7-final
target\t0051-deploy
target\t0051-r2-release
target\t0053-deploy
target\t0054-final
```

Skipped/retained: `target\debug` (6.858 GiB, process inspection unavailable so
fail-closed), `target\release` (1.010 GiB, canonical release), and all live
`.catdesk` control-plane/browser/recovery/review roots including
`.catdesk\wake-bridge` (0.384 GiB). `.git`, source, scripts, docs, assets, and
unknown data were never candidate roots.

Post-cleanup, `target\release\catdesk.exe` matched its retained SHA-256 sidecar:
`532e05f4f772d12c68990ba313ebe5f9a9854ecb5723703cfbeeddbe99be3689`.

## Verification to run

- `powershell -ExecutionPolicy Bypass -File scripts/test-cleanup-catdesk-storage.ps1`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- `git diff --check`

Observed results: the PowerShell fixture suite, `cargo fmt --check`, clippy,
and `git diff --check` passed. `cargo test` ran 461 tests: 443 passed, 11
ignored, and 7 failed for pre-existing host dependencies (three missing advisor
executable cases and four Windows process-termination access-denied cases).
No Rust source was changed for T-0057 and no production release build was
performed. The dirty worktree was preserved; no branch, commit, push, merge,
tunnel, browser, or runtime mutation was attempted.
