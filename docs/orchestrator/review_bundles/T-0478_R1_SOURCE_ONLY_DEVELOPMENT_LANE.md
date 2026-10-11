# T-0478 R1 — Source-only development lane, independent of protected release

Date: 2026-10-11 UTC. Current canonical CatDesk chat: https://chatgpt.com/c/6acaae13-4b18-83e9-b6ca-3c5e00cf47a8.

## Status and objective

This is **Phase 1 of T0478**, not yet a separately isolated running daemon. The operator wants to keep development moving without full product-root signed image rotation/promotion on every source iteration. We explicitly separate development build/verification from protected production activation.

## Implementation

New fixed public development facade `scripts/catdesk-dev-lane.ps1` accepts exactly one fixed action: `status`, `build`, or `verify`. It takes no directory, executable, image, installer, account, URL, browser, Wake event, owner, tunnel, or credentials argument.

- `status` verifies the fixed source workspace, manifest, lockfile and entrypoint are regular non-reparse files and that Git's root matches the fixed script-relative root. It reports current source commit, whether Git has changes and optional debug binary SHA-256. It does **not** start or install anything.
- `build` executes only `cargo build --locked --offline --bin catdesk` from that root and measures the resulting development binary. No `cargo run`, daemon launch, product install, profile access, Wake event, MCP tunnel ownership, elevation, or protected receipt write.
- `verify` executes only fixed `cargo fmt --all -- --check`, `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings`, and `cargo test --locked --offline --bin catdesk`. It runs tests, **not** a production daemon.
- Every successful command prints the same bounded JSON fields: `schemaVersion,mode,operation,sourceCommit,workingTreeClean,debugImageSha256,productionInstallationChanged,daemonOrBrowserStarted,isolatedRuntimeReady`. The last three booleans explicitly remain `false`—a successful source build does not mean a production install or isolated dev runtime was activated.
- The CI Windows Rust job parses the script and runs `status` as a read-only source workspace canary, verifying expected schema and **no claimed production authority**. The full Rust/Python/Wake CI jobs remain intact. The script itself is not run in `build` or `verify` mode by CI, to avoid unnecessary duplicate build/test work.

## Operator-only one-line examples (NOT needed for ChatGPT to progress)

```powershell
cd 'C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop'; & '.\scripts\catdesk-dev-lane.ps1' status
```

```powershell
cd 'C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop'; & '.\scripts\catdesk-dev-lane.ps1' build
```

```powershell
cd 'C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop'; & '.\scripts\catdesk-dev-lane.ps1' verify
```

## Strict limitations and next engineering gates

- This is only **source build/verification**; no developer runtime is isolated yet. A future `run` action is prohibited until explicit cross-root safety can be proved via tests and protected production tunnel/process ownership can be distinguished. The existing production app cannot simply be started with an alternate directory without verifying every global profile, socket, port, registry, and background-service side effect.
- Default inherited Cargo toolchain and local workspace may still produce or run development test fixtures. The script therefore does NOT claim a secure sandbox, credential isolation, production-equivalent release, or attended full recovery.
- Subsequent T0478 R2: audit all CatDesk state and network binding sources, add a separate test-only runtime identity with dedicated data path and non-overlapping port, deny official tunnel start/browser dispatch/privileged installation, and add negative tests demonstrating separation. Only then expose `run`/`stop` of an isolated development worker.
- The production recovery milestone remains **partially incomplete**: stable supervisor `startupDefinition=SUPERVISOR_STARTUP_DEFINITION_READ_FAILED`, `startupPolicy=SUPERVISOR_STARTUP_POLICY_UNPROVEN`, `readiness=SUPERVISOR_ROOT_UNAVAILABLE`, `frontDoor3201=SUPERVISOR_3201_NOT_PROBED`. No unsafe supervisor activation or signed epoch rotation is authorized. The old serving CatDesk MCP shell gateway still rejects the existing typed `catdesk.ps1 diagnose` with `INVALID_ARGUMENT`; source review of its typed support had already passed but serving is stale. Do not quietly claim nine-layer recovery complete.
- **Wake restoration checkpoint accepted:** Chat51 registry and independent Wake agree on SHA `7faf9cc96669e240362da0dd4266bec8b2585f0ef3e03b4b56ae4c8f66db1b22` generation32; WakeHost dev84 RUNNING; manual event `manual-wake-mcp-1791691944140` verified exact `EXACT_USER_MESSAGE_APPENDED`, `SENT`, timer `COMPLETE`, queue0; not natural automatic acceptance.
- The independent WakeHost is an active separate service and should stay externally owned while we develop. Old generation11–31 queue history remains forensic. External Secure MCP tunnel is unchanged. Protect existing T0215 signed epoch1 installed, T0366 signed epoch2 pending and seven unrelated untracked files.
