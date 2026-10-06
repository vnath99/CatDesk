# T-0317 — Recovery Wedged-Daemon Convergence Review Bundle

## Purpose

T-0317 is a bounded reliability repair for the operator-visible failure observed on 2026-09-03. A canonical `target\release\catdesk.exe --catdesk-daemon` process remained alive while no loopback listener existed on the configured local MCP port. `catdesk.ps1 recover` therefore returned a pending/action-required state and `scripts/start-catdesk-stack.ps1 -Mode recover -Execute` exhausted its readiness window. Recovery converged only after the operator manually killed the attributable CatDesk process and relaunched the canonical daemon.

The operator explicitly elevated ease of recovery to the highest CatDesk priority. The acceptance target is that this exact stale-daemon/no-listener shape must converge through one supported recovery invocation without an operator command waterfall.

## Root cause

The lifecycle engine used listener ownership as the primary daemon-existence signal. If no listener existed, `$existing` was null and the bootstrap proceeded as though no CatDesk daemon process existed. A still-alive canonical daemon could therefore survive outside the listener model. Launching another daemon did not guarantee convergence because the stale process could still retain runtime/process ownership or otherwise interfere with startup.

The missing state was: **canonical daemon process exists + exact daemon command mode + canonical executable/hash + no listener**.

## Repair

`scripts/start-catdesk-stack.ps1` now adds two narrow helpers:

- `Get-CatDeskDaemonProcessCandidates`
  - Enumerates only a bounded set of `catdesk.exe` process rows.
  - Requires the fixed `--catdesk-daemon` command-line token; GUI/non-daemon CatDesk processes are not daemon candidates.
  - Requires a real process id and executable path.
  - Resolves and SHA-256 hashes the candidate executable.
  - Marks a candidate canonical only when the executable path and SHA-256 exactly match the already-validated canonical CatDesk identity.
  - Process name alone is never stop authority.

- `Stop-StaleCanonicalCatDeskDaemonForRecovery`
  - Rechecks loopback listener ownership before process inspection and again immediately before mutation.
  - Does nothing when the listener becomes healthy.
  - Does nothing when no daemon candidate exists.
  - Refuses a noncanonical daemon candidate.
  - Refuses multiple canonical daemon candidates as ambiguous.
  - Stops only the single exact canonical daemon candidate when no listener exists.
  - Waits boundedly for process exit and fails closed if the process does not terminate.

`Invoke-CanonicalStackBootstrap` invokes this convergence step only when no listener is present, then re-reads listener state and continues through the existing supported canonical daemon launch/readiness path.

## Security and ownership invariants

The repair does not broaden authority:

- no process is selected by image name alone;
- a foreign executable path/hash is never killed;
- multiple candidates are never guessed between;
- GUI/non-daemon CatDesk processes are not classified as daemons;
- listener readiness wins over stale-process evidence;
- the canonical identity remains the existing path + SHA-256 authority;
- no arbitrary process id/path is caller supplied;
- no Secure MCP/tunnel stop, reconfiguration, or ownership change is introduced;
- no browser, wake target, ChatGPT target, Scheduler, ProgramData trust state, reviewed-image, signing/provenance, external-project, or Git publication authority is added.

## Deterministic regression matrix

The updated `scripts/test-start-catdesk-stack.ps1` covers:

1. exact canonical daemon process + no listener -> stop that pid and launch exactly one canonical `--catdesk-daemon` replacement;
2. healthy canonical listener -> repeated recovery performs no mutation;
3. canonical GUI/non-daemon process -> excluded from daemon candidate set and does not block daemon launch;
4. same-name daemon at a foreign executable path -> noncanonical classification and fail-closed refusal with no mutation;
5. multiple canonical daemon candidates -> fail-closed refusal with no mutation;
6. direct candidate classification validates exact command token, path, and hash boundaries;
7. existing bootstrap readiness, external-runtime, wake-runtime, foreground/GUI, timeout, and failure regressions remain intact.

## Verification

The following checks passed after the repair:

- `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\test-start-catdesk-stack.ps1`
  - result: `start-catdesk-stack regression checks passed`
- `cargo test --test recovery_powershell -- --nocapture`
  - result: 20 passed, 0 failed
- `cargo fmt --check`
  - result: passed
- `cargo clippy --all-targets --all-features -- -D warnings`
  - result: passed
- `cargo test --all-targets --all-features`
  - result: passed

The current dirty repository baseline reports the two lifecycle scripts as untracked, so ordinary Git diff output is not an authoritative historical baseline for those files. No commit, push, merge, reset, clean, or publication occurred.

## Live-runtime boundary

This bundle is **repository/source-test acceptance only**. The current production CatDesk daemon was not intentionally killed by this ticket because no independent watchdog/autostart recovery mechanism had yet been proven armed. Losing the local 3200 MCP before such an independent recovery owner exists could strand ChatGPT control even if the official external tunnel process remains alive.

A subsequent live acceptance must therefore proceed in this order:

1. make an independent watchdog/autostart owner available and verify it is running;
2. verify the official Secure MCP runtime is independently healthy and configured to survive CatDesk exit;
3. intentionally terminate one exact canonical local CatDesk daemon under a bounded test;
4. prove the independent recovery owner restores the daemon/listener without operator commands;
5. prove the connector returns to `CONNECTED_VERIFIED`;
6. repeat recovery to prove idempotence;
7. only then test a controlled stale/wedged-daemon shape if it can be produced without threatening the independent tunnel/recovery owner.

The live `catdesk.ps1 autostart status` currently reports `AUTOSTART_DISABLED`, and `catdesk.ps1 autostart enable` returns `AUTOSTART_UNAVAILABLE`. That is now a concrete recovery blocker and should be handled as the next bounded recovery ticket before ordinary feature work.

## Review decision

**Repository/source-test boundary: ACCEPTED.**

The exact 2026-09-03 stale canonical daemon/no-listener defect now has a narrow, fail-closed deterministic repair and green verification. **Live recovery acceptance remains open** until an independent watchdog can be armed and the kill/recover drill can be completed without requiring the operator to restore CatDesk manually.

## Next bounded action

T-0318 should repair/diagnose the persistent autostart/watchdog activation path, ensure an owned watchdog can be started immediately in the current user session, and then perform the staged live daemon-loss recovery drill while preserving the official Secure MCP runtime and exact current control-chat target.
