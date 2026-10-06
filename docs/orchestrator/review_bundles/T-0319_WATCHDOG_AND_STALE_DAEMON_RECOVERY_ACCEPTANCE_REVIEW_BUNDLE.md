# T-0319 — Watchdog and stale-daemon recovery acceptance review bundle

## Review request

Review the implementation and evidence for T-0319. This bundle is not an
independent review verdict and does not mark the ticket accepted.

## Scope and authority boundaries

| Surface | Behavior under review | Explicit non-authority |
| --- | --- | --- |
| `catdesk.ps1` | Resolves the PowerShell executable path before comparing project-scoped autostart persistence. | Does not change connector, credential, or tunnel ownership. |
| `scripts/catdesk-autostart-supervisor.ps1` | Invokes only public `status` and `recover`; bounded runtime-pending handling returns to normal polling. | Does not compile, promote a release, or signal the official runtime. |
| `scripts/start-catdesk-stack.ps1` | Scans a bounded complete set of CatDesk process rows before exact daemon/path/hash classification. | Stops only one proven canonical daemon with no listener; foreign/multiple/over-bound states fail closed. |
| `scripts/test-stale-canonical-daemon-recovery.ps1` | Runs a temporary loopback-only stale-process fixture. | Never reads or controls the active daemon, connector, runtime, or tunnel. |

## Functional evidence

1. A false `AUTOSTART_CONFLICT` was reproduced: a valid absolute Windows
   PowerShell Run-key entry was compared against an unresolved `powershell.exe`
   fallback. The façade now requires a resolved executable path.
2. Public `autostart status` and `autostart enable` returned
   `AUTOSTART_ENABLED` after the repair.
3. Starting from public `READY`, two public local-only `catdesk.ps1 stop`
   calls were recovered by the watchdog without a manual `recover`. Both
   returned to `READY`; the second observation had exactly one listener owner
   and one project-scoped supervisor. The public post-recovery status verified
   the local daemon and external runtime.
4. The disposable stale-process fixture builds a separate tiny test executable,
   copies it into a temporary canonical `target/release/catdesk.exe`, and starts
   it once with `--catdesk-daemon` but without a listener. Production process
   discovery identifies that real path/hash process. Production stale recovery
   stops it, then a clean temporary replacement binds one verified loopback
   listener.
5. The fixture revealed that the prior first-eight-row scan could miss the
   canonical stale process when more CatDesk processes existed. Discovery now
   observes up to 64 rows and refuses the one-extra-row overload as ambiguous.
   The bootstrap fixture has a direct 65-row regression for that refusal.

## Verification run

```text
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-stale-canonical-daemon-recovery.ps1
stale canonical daemon recovery integration test passed

powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1
canonical bootstrap behavioral tests passed

powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1
consumer lifecycle fixture tests passed

powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-autostart-supervisor.ps1
autostart supervisor fixture tests passed

powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-promote-reviewed-catdesk-build.ps1
reviewed build promotion fixture tests passed

cargo test --test recovery_powershell -- --nocapture
2 passed

cargo fmt --check
pass

cargo clippy --all-targets --all-features -- -D warnings
pass
```

`git diff --check` also passed; it emitted only existing line-ending warnings.

## Required reviewer checks

- Confirm the 64-row bound cannot silently skip an exact candidate and that
  the 65th row correctly fails closed.
- Confirm the temporary fixture cleanup can only signal processes whose path
  equals its generated canonical test path.
- Confirm live evidence used only public CatDesk lifecycle commands and did not
  operate the external official runtime.
- Confirm no result above is overstated as an independent review conclusion.

## 2026-09-07 reviewer addendum — T-0325 superseding mutation boundary

T-0323's independent sweep found that the stale-daemon path described above
still ended in PID-only `Stop-Process`/PID polling after candidate validation.
A selected process could exit and its PID could theoretically be reused before
the destructive call, so PID alone was not sufficient final mutation authority.

T-0325 supersedes that source boundary. Candidate identity now includes process
creation time; production recovery freshly revalidates PID + creation time +
`--catdesk-daemon` mode + canonical path/SHA-256, acquires the exact process
handle, rechecks listener absence after pinning, and performs `Kill()` plus
`WaitForExit()` through that same process object. Deterministic changed-instance
and disappeared-instance cases pass, as does the real disposable stale-daemon
fixture and full project verification.

An independent final T-0319 review must therefore evaluate the current T-0325
exact-process-instance implementation rather than approving the historical
PID-only termination path in isolation. T-0319 remains open; T-0325 hardens its
source authority but does not itself manufacture watchdog-only live acceptance.
T-0326 separately tracks sibling PID-only stop sites in other recovery branches,
and T-0327 closes the post-CIM/pre-handle same-path process-reacquisition gap.

## 2026-09-07 reviewer addendum — T-0328 detached restart handoff

A subsequent bounded recovery audit found another independent PID-reuse seam in
`scripts/restart_catdesk_daemon.ps1` / `scripts/restart_catdesk_daemon_worker.ps1`.
The launcher selected `OldPid`, but the detached worker delayed, reacquired that
numeric PID, and previously used PID-only `Stop-Process` / `Wait-Process` calls.
The worker also reacquired the newly launched child PID during readiness checks.
Neither seam was covered by the earlier `start-catdesk-stack.ps1` process-instance
closures.

T-0328 carries exact start-time/path identity across the detached handoff. The
worker acquires the reacquired old process handle, requires that exact object to
re-prove the transferred identity before listener inspection or mutation, and
then performs `Kill()` / `WaitForExit()` through that same object. The newly
launched replacement is handle-pinned immediately and readiness observes that
same object rather than reacquiring by PID. A deterministic same-path,
different-instance regression plus the normal recovery PowerShell harness and
full project verification are green.

An independent final T-0319 review must therefore evaluate the complete current
T-0325/T-0326/T-0327/T-0328 recovery-hardening chain together with the existing
watchdog/stale-daemon live evidence. T-0319 remains open; this implementer does
not self-approve that independent gate.

## 2026-09-07 reviewer addendum — T-0329/T-0330 launch and promotion identity boundaries

T-0329 closes the direct missing-daemon launch/readiness race: when recovery
launches the canonical daemon itself, readiness is bound to the exact
handle-pinned `Start-Process -PassThru` child rather than accepting a raced-in
same-canonical listener merely because path/hash match.

T-0330 closes the remaining earlier promotion -> restart seam. Promotion used to
validate a listener by PID/path/hash but carry only the PID into
`restart_catdesk_daemon.ps1`; the restart launcher could therefore reacquire a
reused PID and establish a new identity unrelated to the listener promotion had
reviewed. `Get-PromotionListener` now pins the listener process and returns exact
start time, resolved path, and verified SHA-256 with the PID. Promotion transfers
all three identity fields to the restart launcher, which requires the reacquired
pinned process to re-prove that exact identity before recording restart intent or
launching the detached worker. The existing T-0328 launcher -> worker identity
contract remains unchanged.

Deterministic coverage rejects a same-path/same-hash process with a different
creation tick and rejects hash drift. `cargo test --test recovery_powershell --
--nocapture` passes 2/2 after widening a test-only future-clock fixture from +31
to +60 seconds so it deterministically exceeds the unchanged 30-second
production tolerance. Full fmt/test/build, strict Clippy, and `git diff --check`
are green.

The independent final T-0319 review must therefore evaluate the cumulative
T-0325/T-0326/T-0327/T-0328/T-0329/T-0330 recovery authority chain. T-0330 is
implementation/test complete but does not self-manufacture the separate
independent acceptance verdict.

## 2026-09-07 reviewer addendum — T-0331 Secure MCP migration timeout boundedness

T-0331 found a separate one-command boundedness gap on the supported legacy
`external_foreground` -> official-runtime migration path. `scripts/setup-secure-mcp.ps1`
already imposed a command timeout on its one-shot tunnel-client child, but after
`process.Kill()` it called an unconditional `process.WaitForExit()`. A child that
failed to terminate could therefore hang recovery indefinitely after the nominal
timeout.

The setup helper now mirrors the accepted bootstrap behavior: post-kill waiting is
bounded to 2000 ms and a surviving child produces an explicit `TerminationFailed`
result plus a distinct fail-closed error. This does not add authority over the
externally owned serving Secure MCP runtime; it applies only to the one-shot child
already launched by the bounded setup invocation.

A source/AST-only regression in `scripts/test-secure-mcp-route-validation.ps1`
asserts the bounded wait, termination-failed signal/classification, and absence of
a naked `process.WaitForExit();`. Direct `.ps1` execution remained correctly
restricted, so the regression was added to the existing Windows
`tests/recovery_powershell.rs` harness and executed through the approved Cargo test
profile. `cargo test --test recovery_powershell -- --nocapture` passes 2/2,
including the Secure MCP route-validation fixture. Full Rust fmt/test/build, strict
Clippy, and `git diff --check` are green.

Accordingly T-0331 is implementation/test complete. The independent final T-0319
review must include the cumulative T-0325 through T-0331 chain and still may not be
self-approved from this implementation lineage.

## 2026-09-07 reviewer addendum — T-0332 Codex prerequisite probe boundedness

T-0332 found another live one-command boundedness defect in the supported
`plan`/`recover` entry path. `Test-CodexOnDemandPrerequisites` synchronously ran
`codex --version` and `codex login status` with no timeout before recovery
orchestration. Because Codex is only an on-demand worker prerequisite, a wedged CLI
could nevertheless block daemon/runtime convergence indefinitely.

The current implementation routes both probes through the existing bounded native
process helper with a 5-second per-probe ceiling and 64 KiB capture. The bounded
helper was also tightened so that, after timeout and selected-wrapper termination,
it does not perform an unbounded output-task drain that could remain held open by an
inherited child pipe. Deterministic Windows coverage uses an indefinitely looping
fake Codex wrapper, lowers only the test timeout to 250 ms, proves bounded failure in
under 3 seconds, and statically rejects reintroduction of direct `& $codex.Source`
prerequisite probes. The approved recovery PowerShell harness passes 2/2; full
fmt/test/build, strict Clippy, and `git diff --check` are green. No daemon, external
Secure MCP runtime, protected wake target, release bytes, or promotion authority is
mutated by T-0332.

Accordingly T-0332 is implementation/test complete. The independent final T-0319
review must now include the cumulative T-0325 through T-0332 recovery
authority/boundedness chain and still may not be self-approved from this
implementation lineage. See
`docs/orchestrator/review_bundles/T-0332_CODEX_PREREQUISITE_PROBE_BOUNDEDNESS_REVIEW_BUNDLE.md`.

## 2026-09-07 reviewer addendum — T-0333 recovery helper subprocess boundedness

T-0333 found additional live one-command hang boundaries after the T-0332 audit.
Wake-runtime probing, wake-runtime repair, and ordered official-runtime migration
still crossed synchronous child-process boundaries without parent-side deadlines.
The wake repair helper also ran Python/venv/pip/test children without internal
bounds, and the Secure MCP setup bounded-process helper could still block draining
redirected pipes after a timeout if a descendant retained inherited handles.

The correction reuses the existing bounded native-process primitive for the three
bootstrap helper boundaries, adds finite native-child budgets inside wake-runtime
repair before its larger parent deadline can expire, and makes the Secure MCP
helper return immediately after timeout termination rather than waiting forever on
pipe drain. This does not grant CatDesk ownership over the externally served Secure
MCP runtime and does not broaden wake, Scheduler, promotion, or daemon authority.

The approved recovery PowerShell harness passes 2/2. Full `cargo fmt --check`,
`cargo test` (exit 0; main suite 876 passed / 21 ignored), `cargo build`, strict
Clippy, and `git diff --check` pass. The current token profile still emits the
existing AppContainer helper `Access is denied` fixture diagnostics without
failing the top-level suite.

Accordingly T-0333 is implementation/test complete. The independent final T-0319
review must now evaluate the cumulative T-0325 through T-0333 recovery
authority/boundedness chain and still may not be self-approved from this
implementation lineage. See
`docs/orchestrator/review_bundles/T-0333_RECOVERY_HELPER_SUBPROCESS_BOUNDEDNESS_REVIEW_BUNDLE.md`.

## 2026-09-07 reviewer addendum — T-0334 public stop exact process-instance authority

The continued T-0319 audit found one residual destructive PID-only boundary in the
public lifecycle facade. `catdesk.ps1 stop` validated a canonical loopback listener,
but then discarded its creation/path/hash identity and invoked `Stop-Process -Id`
using only the selected PID. Exit plus PID reuse between validation and mutation
could therefore redirect stop authority to a different process instance.

T-0334 removes that boundary by delegating the production public-stop mutation to
the already-reviewed `Stop-CatDeskListenerProcessForRecovery` primitive. The helper
pins the selected process instance, re-proves loopback listener continuity by PID +
creation identity + path + SHA-256 immediately before mutation, kills only through
the pinned process object, and bounds exit waiting to 30 seconds. Exact-instance
replacement/ambiguity fails closed; an already-exited exact candidate requires no
PID-directed mutation.

The focused Windows recovery/lifecycle harness passes 2/2. Full fmt/test/build and
strict Clippy pass. No external Secure MCP runtime ownership, wake target, Scheduler
state, release promotion, Git publication, or worktree cleanup is introduced by this
ticket.

Accordingly T-0334 is implementation/test complete. The independent final T-0319
review must now evaluate the cumulative T-0325 through T-0334 recovery
authority/boundedness chain and still may not be self-approved from this
implementation lineage. See
`docs/orchestrator/review_bundles/T-0334_PUBLIC_STOP_EXACT_PROCESS_INSTANCE_AUTHORITY_REVIEW_BUNDLE.md`.

## T-0335 addendum — trusted restart interpreter authority

The continued audit found that `restart_catdesk_daemon.ps1` still launched its
detached worker through the bare executable name `powershell.exe`. That left a
PATH/current-directory executable-search authority seam at the restart handoff
even though the CatDesk old-process identity crossing the same boundary had
already been hardened. T-0335 now derives the interpreter from
`[Environment]::SystemDirectory\WindowsPowerShell\v1.0\powershell.exe`, rejects
missing/non-file/reparse/identity-drift candidates, and passes only that absolute
path to `Start-Process`. The Windows recovery fixture prepends a fake
`powershell.exe` to PATH and proves it cannot capture selection; source checks
also reject a basename-launch regression.

The approved recovery PowerShell harness passes 2/2, and fmt/build/main-binary
suite/strict-Clippy gates pass. Monolithic `cargo test` result retrieval hit
repeated transient connector HTTP 504s in this run, so that transport timeout is
not represented as either a repository failure or a false full-suite success.
No live daemon/tunnel/browser/protected-wake/promotion/Scheduler/Git mutation
occurred.

Accordingly T-0335 is implementation/test complete. The independent final T-0319
review must now evaluate the cumulative T-0325 through T-0335 recovery
authority/boundedness chain and existing watchdog/stale-daemon evidence, and it
still may not be self-approved from this implementation lineage. See
`docs/orchestrator/review_bundles/T-0335_RESTART_LAUNCHER_TRUSTED_POWERSHELL_AUTHORITY_REVIEW_BUNDLE.md`.

## T-0336 addendum — bounded successful-exit output drain

The continued one-command audit found that the bounded native-process runners still
had one success-path escape: after the selected wrapper process exited within its
deadline, both `start-catdesk-stack.ps1` and `setup-secure-mcp.ps1` called an
unbounded `Task.WaitAll(stdout, stderr)`. A descendant that inherited the wrapper's
redirected stdout/stderr handles could therefore keep those stream tasks open after
the selected parent exited normally and hang recovery indefinitely without ever
triggering the process timeout.

T-0336 gives the successful-parent stream drain its own fixed 2-second bound and
returns explicit `OutputDrainTimedOut` evidence. Callers fail closed before using
partial output or a nominal parent exit code as success. A deterministic Windows
fixture creates the exact wrapper-exits/descendant-retains-pipes shape and proves
the bounded helper returns through the output-drain classification rather than
waiting for the descendant. The Secure MCP route-validation fixture also rejects
reintroduction of the old unbounded `Task.WaitAll(stdout, stderr)` form.

The approved recovery PowerShell harness passes 2/2. `cargo fmt --check`, full
`cargo test` (main suite 876 passed / 0 failed / 21 ignored), `cargo build`, and
strict all-target/all-feature Clippy pass. No live daemon/tunnel/browser/protected
wake/promotion/Scheduler/Git mutation occurred and external Secure MCP ownership
is unchanged.

Accordingly T-0336 is implementation/test complete. The independent final T-0319
review must now evaluate the cumulative T-0325 through T-0336 recovery
authority/boundedness chain and existing watchdog/stale-daemon evidence, and it
still may not be self-approved from this implementation lineage. See
`docs/orchestrator/review_bundles/T-0336_BOUNDED_NATIVE_PROCESS_OUTPUT_DRAIN_REVIEW_BUNDLE.md`.

## T-0337 addendum — trusted PowerShell authority for mutating recovery helpers

The continued one-command audit found that wake-runtime repair and ordered legacy
`external_foreground` -> official Secure MCP migration still selected their
PowerShell interpreter with caller-derived `$PSHOME`. T-0337 now resolves only
`[Environment]::SystemDirectory\WindowsPowerShell\v1.0\powershell.exe`, requires
that exact target to be a real non-reparse file whose resolved full path remains
the fixed expected OS path, and passes the absolute identity into the existing
bounded recovery-helper runner. This removes caller PATH/`$PSHOME` interpreter
selection from these two mutating recovery boundaries without adding any external
Secure MCP ownership or restart authority.

The deterministic Windows fixture prepends a fake `powershell.exe` to PATH and
proves it cannot capture either helper; source-scoped assertions reject `$PSHOME`
or `Get-Command` interpreter selection in the two mutation functions. The focused
`cargo test --test recovery_powershell -- --nocapture` harness passes 2/2, fmt,
all-target/all-feature build, and strict all-target/all-feature Clippy pass. Two
monolithic `cargo test --all-targets --all-features` attempts ended at the CatDesk
connector with HTTP 504 before a repository test result was returned, so those
calls are recorded as inconclusive transport evidence rather than a false full-suite
pass or repository failure. No live helper invocation, daemon/tunnel replacement,
browser wake, protected wake edit, Scheduler mutation, release promotion, Git
publication, or worktree cleanup occurred.

Accordingly T-0337 is implementation/test complete. The independent final T-0319
review must now evaluate the cumulative T-0325 through T-0337 recovery
authority/boundedness chain and existing watchdog/stale-daemon evidence, and it
still may not be self-approved from this implementation lineage. See
`docs/orchestrator/review_bundles/T-0337_RECOVERY_HELPER_TRUSTED_POWERSHELL_AUTHORITY_REVIEW_BUNDLE.md`.

## T-0338 addendum — trusted interpreter authority for Codex prerequisite wrappers

T-0332 bounded the Codex prerequisite probes, but its wrapper compatibility path still selected `.cmd`/`.bat` execution through caller-controlled `$env:ComSpec` and `.ps1` execution through caller-derived `$PSHOME`. That left an interpreter-selection authority gap on the supported `plan`/`recover` path even after T-0335/T-0337 had fixed the equivalent recovery-helper boundaries.

T-0338 now resolves command-script wrappers only through `[Environment]::SystemDirectory\cmd.exe` and PowerShell wrappers through the existing fixed OS Windows PowerShell resolver. Both trusted identities require a regular non-reparse file and exact resolved full-path equality with the OS-derived candidate. Direct executable Codex discovery remains direct, while T-0332 process timeout, bounded capture, bounded post-exit drain, and fail-closed semantics remain unchanged.

The deterministic Windows recovery fixture injects a fake `ComSpec` and PATH entry while exercising a real `.cmd` Codex shim, proving caller environment state cannot capture interpreter selection. Source-scoped assertions also reject `$env:ComSpec`, `$PSHOME`, or `Get-Command` interpreter selection in `Invoke-BoundedCodexProbe` and require both fixed trusted resolvers. The focused Windows recovery harness passes 2/2; fmt, full all-target/all-feature cargo test, build, and strict Clippy pass. No live daemon/tunnel mutation, browser wake, protected wake-target edit, Scheduler mutation, release promotion, Git publication, or worktree cleanup occurred.

Accordingly T-0338 is implementation/test complete. The independent final T-0319 review must now evaluate the cumulative T-0325 through T-0338 recovery authority/boundedness chain and existing watchdog/stale-daemon evidence, and it still may not be self-approved from this implementation lineage. See `docs/orchestrator/review_bundles/T-0338_CODEX_PREREQUISITE_TRUSTED_WRAPPER_INTERPRETER_AUTHORITY_REVIEW_BUNDLE.md`.

## T-0339 addendum — final recovery convergence deadline phase boundary

The continued one-command audit found that `Invoke-CanonicalStackBootstrap` created its final 5–180 second `readinessDeadline` before bounded wake-runtime repair and ordered legacy `external_foreground` -> official Secure MCP migration. Those prerequisite phases already have their own finite deadlines, and wake repair may legitimately consume up to 25 minutes. A long but successful prerequisite could therefore leave the same `recover` invocation with an expired final readiness budget and force the operator to run recovery a second time.

T-0339 moves final convergence deadline creation until after wake repair and any ordered runtime migration complete. Their existing independent timeouts remain unchanged; daemon launch/listener/runtime convergence now receives the full requested readiness window. This changes only timeout phase accounting and does not broaden ownership or mutation authority over the externally owned Secure MCP runtime.

The deterministic recovery fixture delays both wake repair and migration by 1.2 seconds while using a 5-second final readiness timeout, invokes `Invoke-CanonicalStackBootstrap` once, requires at least 4.5 seconds of readiness budget after prerequisites finish, and verifies the existing `wake-repair,stop:130,migrate,start:--catdesk-daemon` order. The approved Windows recovery harness passes 2/2; `cargo fmt --all -- --check`, full `cargo test --all-targets --all-features`, `cargo build --all-targets --all-features`, strict all-target/all-feature Clippy, and `git diff --check` pass.

Accordingly T-0339 is implementation/test complete. The independent final T-0319 review must now evaluate the cumulative T-0325 through T-0339 recovery authority/boundedness chain and existing watchdog/stale-daemon evidence, and it still may not be self-approved from this implementation lineage. See `docs/orchestrator/review_bundles/T-0339_RECOVERY_CONVERGENCE_DEADLINE_PHASE_BOUNDARY_REVIEW_BUNDLE.md`.

## T-0340 addendum — bounded wake Python launcher discovery

The continued one-command audit found that `scripts/repair_wake_bridge_environment.ps1` still invoked `py.exe` directly while `Resolve-Python3` discovered the selected interpreter. That child bypassed the repair helper's bounded native-process primitive, so a wedged Python launcher could hold recovery until the much larger outer wake-repair deadline rather than failing on a local discovery deadline.

T-0340 routes launcher discovery through `Invoke-NativeQuiet` with an explicit 5-second timeout. The discovery child writes `sys.executable` to a temporary file rather than redirected stdout, avoiding a separate inherited-pipe/output-drain boundary; the resulting path then passes through the existing bounded `Test-Python3` validation before selection. No external Secure MCP ownership, daemon/release promotion authority, protected wake-target mutation, Scheduler mutation, or Git publication is introduced.

The approved Windows recovery harness now parses `Resolve-Python3`, rejects direct `& $py.Source` execution, requires `Invoke-NativeQuiet -Executable $py.Source`, and requires the 5000 ms deadline. The harness passes 2/2; `cargo fmt --all -- --check`, full `cargo test --all-targets --all-features` (main suite 876 passed / 0 failed / 21 ignored), `cargo build --all-targets --all-features`, strict Clippy, and `git diff --check` pass.

Accordingly T-0340 is implementation/test complete. The independent final T-0319 review must now evaluate the cumulative T-0325 through T-0340 recovery authority/boundedness chain and existing watchdog/stale-daemon evidence, and it still may not be self-approved from this implementation lineage. See `docs/orchestrator/review_bundles/T-0340_WAKE_PYTHON_LAUNCHER_DISCOVERY_BOUNDEDNESS_REVIEW_BUNDLE.md`.

## T-0341 addendum — recovery tunnel-client discovery authority

The continued one-command audit found that `Find-TunnelClient` in `scripts/start-catdesk-stack.ps1` still consulted `Get-Command tunnel-client` before CatDesk's durable user-local installation paths. A caller-controlled PATH entry could therefore influence the executable used for official-runtime status verification during `plan`/`recover` even though the runtime itself remains externally owned.

T-0341 removes caller PATH from recovery executable selection. Recovery now accepts only an explicit operator-supplied `-TunnelClientPath` or CatDesk's pinned `%USERPROFILE%\.catdesk\tools\tunnel-client\current\tunnel-client.exe` / legacy user-local installation identity. The migration-only setup helper remains unchanged because its `migrate` mode does not discover or invoke tunnel-client. No external runtime stop/restart/migration authority is added.

The deterministic recovery fixture prepends a fake `tunnel-client.exe` directory to PATH, proves an explicit operator path still wins, proves a missing explicit path cannot fall through to PATH, statically rejects `Get-Command` inside recovery `Find-TunnelClient`, and preserves current/legacy pinned fallback plus fail-closed missing-client coverage. The focused recovery harness passes 2/2; fmt, full all-target/all-feature tests, build, and strict Clippy pass.

Accordingly T-0341 is implementation/test complete. The independent final T-0319 review must now evaluate the cumulative T-0325 through T-0341 recovery authority/boundedness chain and existing watchdog/stale-daemon evidence, and it still may not be self-approved from this implementation lineage. See `docs/orchestrator/review_bundles/T-0341_RECOVERY_TUNNEL_CLIENT_DISCOVERY_AUTHORITY_REVIEW_BUNDLE.md`.

## T-0342 addendum — interrupted promotion recovery helper boundedness

The continued one-command audit found that interrupted reviewed-promotion pair recovery still invoked `scripts/promote-reviewed-catdesk-build.ps1` directly in the current PowerShell process. Because that helper is mutation-capable and reachable from public `recover`, a wedged helper could block recovery indefinitely despite the bounded helper infrastructure added elsewhere in the chain.

T-0342 preserves the existing provenance rule that interrupted transaction state is not rollback authority by itself, but executes the helper through fixed OS Windows PowerShell and `Invoke-BoundedRecoveryHelper` with a 30-second parent deadline. Timeout, termination failure, output-drain timeout, overflow, nonzero exit, oversized result, or missing `RECOVERED_PRIOR_CANONICAL` proof all fail closed; fallback remains limited to independently validated persistent LKG authority.

The deterministic Windows recovery fixture statically rejects direct `& $promotion` execution and requires the trusted PowerShell resolver, bounded helper, and explicit deadline. It also runs an indefinitely looping synthetic promotion helper with a fixture-only 250 ms deadline and proves control returns through bounded timeout in under 3 seconds. `cargo test --test recovery_powershell -- --nocapture` passes 2/2; fmt, the main CatDesk binary test target, all-target/all-feature build, strict Clippy, and `git diff --check` pass. A monolithic all-target test request ended at the connector with HTTP 504 before returning a result, so no fresh monolithic-suite claim is made from that invocation.

Accordingly T-0342 is implementation/fixture complete. The independent final T-0319 review must now evaluate the cumulative T-0325 through T-0342 recovery authority/boundedness chain and existing watchdog/stale-daemon evidence, and it still may not be self-approved from this implementation lineage. See `docs/orchestrator/review_bundles/T-0342_INTERRUPTED_PROMOTION_RECOVERY_HELPER_BOUNDEDNESS_REVIEW_BUNDLE.md`.

## T-0343 addendum — Codex prerequisite executable discovery authority

The continued one-command audit found that T-0338 had fixed the interpreter used after a Codex wrapper was selected, but `Test-CodexOnDemandPrerequisites` still selected `codex` with `Get-Command "codex"`. Caller-controlled PATH therefore remained executable-selection authority at the public `plan`/`recover` prerequisite boundary: a shadow Codex wrapper could be chosen and could falsely satisfy the bounded version/login probes.

T-0343 removes that search authority while preserving the established explicit recovery override. `CATDESK_CODEX_CLI_EXECUTABLE` retains precedence when explicitly inherited; otherwise recovery considers only deterministic current-user `%APPDATA%\npm\codex.exe`, `codex.cmd`, or `codex.ps1` candidates. Selection requires a real file, rejects reparse points, and verifies exact resolved full-path identity. Existing T-0332 bounded probe semantics and T-0338 fixed-OS wrapper interpreter authority remain unchanged.

The deterministic Windows fixture prepends a fake `codex.cmd` to caller PATH while providing a distinct trusted synthetic current-user npm root, removes the explicit override, and proves the trusted candidate is selected rather than the PATH shadow. Source guards reject reintroduction of `Get-Command codex`, `$env:PATH`, or npm root/prefix/config discovery. `cargo test --test recovery_powershell -- --nocapture` passes 2/2; fmt, full all-target/all-feature tests (main suite 876 passed / 0 failed / 21 ignored), build, strict Clippy, and `git diff --check` all pass.

Accordingly T-0343 is implementation/fixture complete. The independent final T-0319 review must now evaluate the cumulative T-0325 through T-0343 recovery authority/boundedness chain and existing watchdog/stale-daemon evidence, and it still may not be self-approved from this implementation lineage. See `docs/orchestrator/review_bundles/T-0343_CODEX_PREREQUISITE_EXECUTABLE_DISCOVERY_AUTHORITY_REVIEW_BUNDLE.md`.

## T-0344 wake-repair Python discovery authority

The continuing audit found that T-0340 bounded Python launcher execution but left default wake-repair discovery dependent on caller executable/environment search state: `Get-Command py.exe`, `Get-Command python.exe`, `$env:LOCALAPPDATA`, and `$env:USERPROFILE`. Public one-command recovery invokes wake repair without an explicit Python override, so caller state could influence which interpreter obtained execution authority.

T-0344 preserves explicit `-PythonExecutable` as intentional operator authority while removing caller search/environment authority from default discovery. LocalAppData/UserProfile are derived from `Environment.GetFolderPath`, Windows root from `Environment.SystemDirectory`, and only deterministic launcher/interpreter paths are considered. Default candidates must be real non-reparse files whose canonical full path exactly matches the expected candidate; Python paths reported by `py.exe` receive the same validation before bounded Python-version validation. T-0340's 5-second launcher bound and temporary-file output channel remain intact.

Repository verification passes via sanctioned `verify_project` (`cargo fmt --check`, `cargo test`, `cargo build`), strict all-target/all-feature Clippy passes, and `git diff --check` passes with only existing LF→CRLF warnings. The dedicated Windows PowerShell recovery harness was not executed in this run because CatDesk's allowlist shell mode correctly rejected nested interpreter execution; no unrestricted-shell exception or bypass was requested. The T-0344 review fixture now guards against `Get-Command`/environment search authority and synthetic PATH shadows, but approved Windows-harness execution remains required before cumulative final acceptance.

Accordingly T-0344 is implementation/repository-verification complete but not independently accepted. The independent final T-0319 review must now evaluate the cumulative T-0325 through T-0344 recovery authority/boundedness chain and existing watchdog/stale-daemon evidence, and should execute the Windows recovery harness through an approved path. See `docs/orchestrator/review_bundles/T-0344_WAKE_REPAIR_PYTHON_DISCOVERY_AUTHORITY_REVIEW_BUNDLE.md`.

## T-0345 recovery tunnel-client pinned executable identity closure

T-0341 removed caller PATH from `Find-TunnelClient`, but the default CatDesk user-local `current` and legacy tunnel-client candidates still crossed the recovery authority boundary after only leaf existence / path resolution. Because that client attests official Secure MCP runtime state during `plan`/`recover`, a leaf reparse point or resolved-path identity drift remained able to influence which executable received recovery verification authority.

T-0345 preserves explicit `-TunnelClientPath` as intentional operator authority while hardening default pinned discovery. Default candidates must materialize as `System.IO.FileInfo`, reject `FileAttributes.ReparsePoint`, normalize expected and observed paths through `System.IO.Path.GetFullPath`, and compare them using `StringComparison.OrdinalIgnoreCase` before the executable identity is returned. Caller PATH search remains absent.

The existing T-0341 recovery fixture was extended in place: explicit override, current/legacy pinned discovery, PATH-shadow rejection, and missing-client fail-closed behavior remain covered, while deterministic source guards now require reparse rejection, full-path normalization, exact path comparison, and file-identity inspection. Sanctioned repository verification passes formatting/full Cargo tests/build, strict all-target/all-feature Clippy passes, and `git diff --check` passes. No fresh nested-PowerShell recovery-harness pass is claimed where command policy prevents it; no protection was bypassed to manufacture evidence.

Accordingly T-0345 is implementation/repository-verification complete but not independently accepted. The independent final T-0319 review must evaluate the cumulative T-0325 through T-0345 recovery authority/boundedness chain and existing watchdog/stale-daemon evidence, including approved Windows recovery-harness execution where required. See `docs/orchestrator/review_bundles/T-0345_RECOVERY_TUNNEL_CLIENT_PINNED_IDENTITY_CLOSURE_REVIEW_BUNDLE.md`.

## T-0346 ordered migration handoff audit — no implementation required

A proposed tunnel-client authority handoff was traced through the actual ordered migration path before code mutation. `Invoke-OrderedOfficialRuntimeMigration` invokes `setup-secure-mcp.ps1 -Mode migrate`; that branch performs bounded configuration migration and does not enter tunnel-client discovery/execution. The suspected caller-PATH tunnel-client seam was therefore not recovery-reachable. The protected task was closed without forcing a patch, preserving the external-runtime ownership boundary.

## T-0347 Windows inventory query boundedness closure

The continuing recovery audit found a separate real boundedness defect: `Get-LoopbackCatDeskListener`, `Get-CatDeskListenerProcessInstanceForRecovery`, `Get-CatDeskDaemonProcessCandidates`, and `Get-CatDeskDaemonProcessInstanceForRecovery` still performed Windows networking/CIM provider queries in the recovery PowerShell process. A wedged `Get-NetTCPConnection` or `Get-CimInstance Win32_Process` provider could therefore block public `plan`/`recover` indefinitely despite the existing bounded native-process/helper work.

T-0347 adds the read-only `scripts/query-catdesk-windows-inventory.ps1` helper and routes those observations through `Invoke-BoundedWindowsInventoryProbe`. The parent launches only fixed-OS Windows PowerShell, requires exact non-reparse helper-file identity, imposes a 5-second parent deadline plus 64 KiB bounded capture/drain, and fails closed on timeout, termination failure, output drain timeout, overflow, process error, empty output, or malformed JSON. Listener/process rows cross back only as observations. Canonical executable resolution, SHA-256 proof, creation-time comparison, pinned `System.Diagnostics.Process` acquisition, and every destructive process operation remain in the recovery parent.

Deterministic regression runs an indefinitely hanging synthetic inventory helper with a fixture-only 250 ms deadline and requires bounded failure in under 3 seconds. Existing listener ambiguity/dual-stack attribution and stale-daemon behavior remain covered; the real disposable stale-daemon fixture imports the production bounded inventory dependencies and continues through the actual helper path. Final focused verification is `cargo test --test recovery_powershell -- --nocapture` = 2 passed / 0 failed. Sanctioned `verify_project` passes `cargo fmt --check`, full `cargo test`, and `cargo build`; strict all-target/all-feature Clippy and `git diff --check` pass, with only existing LF-to-CRLF warnings from diff validation.

Accordingly T-0347 is implementation/repository-verification complete but not independently accepted. T-0319 remains open for a genuinely separate final review of cumulative T-0325 through T-0347 together with the existing watchdog, stale-daemon, and literal one-command host evidence. See `docs/orchestrator/review_bundles/T-0347_WINDOWS_INVENTORY_QUERY_BOUNDEDNESS_REVIEW_BUNDLE.md`.

## T-0348 interrupted-promotion recovery descendant boundedness closure

The continuing audit found that T-0342's 30-second bounded promotion parent did not bound all mutation descendants. Interrupted rollback could enter `Invoke-PromotionHandoff`, which invokes `restart_catdesk_daemon.ps1`; that handoff launches a detached restart worker and allows up to 120 seconds for readiness. A parent timeout could therefore return control while a detached worker retained daemon-mutation authority.

T-0348 adds a recovery-specific `-RecoveryRollbackOnly` mode to the promotion helper. It preserves the existing protected reviewed-promotion authorization and exact prior-pair validation, but when used by `start-catdesk-stack.ps1::Invoke-InterruptedPromotionPairRecovery` it restores/revalidates the prior canonical binary and manifest and clears the durable transaction without invoking `Invoke-PromotionHandoff`. The public recovery parent then continues through its already-existing canonical identity reload, bounded Windows inventory, pinned-process listener/stale-daemon reconciliation, and final bounded convergence. Ordinary maintainer promotion continues to require and prove its existing handoff behavior.

Regression coverage proves rollback-only interrupted recovery returns `RECOVERED_PRIOR_CANONICAL`, restores the exact prior hash/manifest, clears the transaction, and records no `handoff:` call; the recovery harness statically requires the bounded caller to pass `-RecoveryRollbackOnly`, while the original T-0342 hanging-helper deadline fixture remains active. `cargo test --test recovery_powershell -- --nocapture` passes 2/2; sanctioned fmt/default full Cargo test/build passes; all-target/all-feature tests and strict Clippy pass; `git diff --check` passes with only existing LF-to-CRLF warnings.

Accordingly T-0348 is implementation/repository-verification complete but not independently accepted. T-0319 remains open for genuinely separate final review of cumulative T-0325 through T-0348, including explicit review that rollback-only recovery preserves reviewed-promotion provenance while transferring daemon reconciliation back to the bounded recovery parent. See `docs/orchestrator/review_bundles/T-0348_INTERRUPTED_PROMOTION_RECOVERY_DESCENDANT_BOUNDEDNESS_REVIEW_BUNDLE.md`.

### T-0349 rollback-only observation independence

T-0349 tightens the T-0348 boundary without expanding recovery authority. Once `Invoke-InterruptedPromotionRecovery` has restored/revalidated the exact prior canonical pair, `-RecoveryRollbackOnly` now clears the durable promotion transaction and returns before resolving the transaction candidate or invoking `Get-PromotionListener`. Thus public one-command recovery no longer spends its bounded interrupted-promotion helper budget on Windows listener/process observation that is irrelevant to disk/provenance rollback. Ordinary maintainer interrupted recovery retains the pre-existing candidate listener proof and canonical handoff path.

Deterministic promotion-fixture coverage makes `ListenerForBuild` throw in rollback-only mode; successful `RECOVERED_PRIOR_CANONICAL` output therefore proves the exact prior binary/manifest restoration and transaction cleanup do not consult listener/process observation or launch a handoff. Sanctioned fmt/default full Cargo test/build, all-target/all-feature tests, strict Clippy, and `git diff --check` are green. The dedicated PowerShell fixture was not re-run because CatDesk's protected allowlist shell rejects nested interpreter execution; no shell-policy bypass was requested or used.

Accordingly T-0349 is implementation/repository-verification complete but not independently accepted. T-0319 remains open for genuinely separate final review of cumulative T-0325 through T-0349, including confirmation that rollback-only recovery's observation-independent early return preserves the exact reviewed-promotion authorization/provenance requirements while leaving daemon reconciliation to the bounded recovery parent. See `docs/orchestrator/review_bundles/T-0349_ROLLBACK_ONLY_RECOVERY_OBSERVATION_INDEPENDENCE_REVIEW_BUNDLE.md`.

### T-0350 project-helper executable identity closure

T-0350 unifies the executable-identity boundary for project-owned PowerShell helpers reached by public one-command recovery. `Resolve-TrustedBootstrapHelperPath` requires a real `FileInfo` leaf, rejects `ReparsePoint`, normalizes expected and observed full paths, and requires exact `OrdinalIgnoreCase` identity before execution. Release recovery is validated before initial dot-sourcing; wake repair, ordered migration, interrupted-promotion rollback, and now the bounded Windows inventory helper all use that shared validator before fixed-OS Windows PowerShell execution. The inventory helper previously duplicated equivalent checks locally; centralizing it removes an independent policy-drift seam without changing its bounded/read-only semantics or acquiring Secure MCP ownership.

Regression coverage requires the shared resolver to retain its leaf/reparse/full-path guards and requires every recovery-reachable project PowerShell helper path above to cross it. Centralization exposed a stale-daemon fixture dependency because that fixture extracts production functions individually; importing `Resolve-TrustedBootstrapHelperPath` into the fixture restored its production dependency graph. `cargo test --test recovery_powershell -- --nocapture` passes 2/2. Sanctioned fmt/default full Cargo test/build, all-target/all-feature tests, strict Clippy, and `git diff --check` are green, with only existing LF-to-CRLF warnings from the diff check.

Accordingly T-0350 is implementation/repository-verification complete but not independently accepted. T-0319 remains open for genuinely separate final review of cumulative T-0325 through T-0350, including confirmation that all project-owned PowerShell helper execution in public recovery crosses the shared exact non-reparse leaf-identity boundary while preserving dirty-worktree tolerance and external Secure MCP ownership. See `docs/orchestrator/review_bundles/T-0350_RECOVERY_PROJECT_HELPER_EXECUTABLE_IDENTITY_CLOSURE_REVIEW_BUNDLE.md`.

### T-0351 public lifecycle bootstrap-engine identity closure

T-0351 closes the pre-import executable-authority gap at the public `catdesk.ps1` lifecycle facade. The facade previously checked only that `scripts/start-catdesk-stack.ps1` existed as a leaf before dot-sourcing it. Because the shared T-0350 `Resolve-TrustedBootstrapHelperPath` validator is defined inside that lifecycle engine, it could not protect the engine bootstrap itself; a reparse-point replacement of the facade-relative engine leaf could therefore gain execution authority before recovery policy loaded.

The facade now derives and normalizes the exact expected engine path, resolves it with `Get-Item -Force`, requires a real `FileInfo` leaf, rejects `ReparsePoint`, normalizes the observed `FullName`, requires exact `OrdinalIgnoreCase` expected/observed identity, and dot-sources only that validated observed path. The change does not add runtime ownership, restart/migration, release-promotion, browser, wake-target, Scheduler, or Git authority.

Regression coverage requires every identity check above to precede dot-sourcing and rejects regression to the former `Test-Path -PathType Leaf` existence-only authority check. `cargo test --test recovery_powershell -- --nocapture` passes 3/3; the sanctioned project verifier passes formatting/default full Cargo tests/build; all-target/all-feature tests and strict Clippy pass; and `git diff --check` is green. Accordingly T-0351 is implementation/repository-verification complete but not independently accepted. T-0319 remains open for genuinely separate final review of cumulative T-0325 through T-0351. See `docs/orchestrator/review_bundles/T-0351_PUBLIC_LIFECYCLE_BOOTSTRAP_ENGINE_IDENTITY_REVIEW_BUNDLE.md`.

## T-0354 cumulative addendum — wake-repair descendant containment

The continuing recovery audit found that wake-runtime repair's Python launcher, `venv`, and `pip` operations had finite immediate-child deadlines but could still leave spawned repair-owned descendants running after timeout. `scripts/repair_wake_bridge_environment.ps1` now uses a wake-repair-local Windows Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, assigns the exact native child process handle before accepting the operation, fails closed if containment assignment fails, and closes the job on timeout and ordinary completion. Thus a timed-out or supposedly completed repair child cannot leave Python/venv/pip descendants mutating after one-command recovery regains control. The generic recovery/tunnel native runner is unchanged so no new descendant-kill authority reaches the externally owned Secure MCP runtime.

Deterministic regression executes a synthetic parent which spawns a delayed descendant that would write a marker after the parent exceeds a 250 ms timeout; after the descendant's would-be write time, the marker remains absent. The focused sanctioned Rust→PowerShell recovery fixture passes 1/1, the sanctioned project verifier passes formatting/default full Cargo tests/build, `cargo test --all-targets --all-features` passes, strict all-target/all-feature Clippy passes, and `git diff --check` is green with only existing line-ending warnings. Accordingly T-0354 is implementation/repository-verification complete but not independently accepted. T-0319 remains open for genuinely separate final review of cumulative T-0325 through T-0354. See `docs/orchestrator/review_bundles/T-0354_WAKE_REPAIR_DESCENDANT_CONTAINMENT_REVIEW_BUNDLE.md`.

## T-0355 cumulative addendum — wake-runtime interpreter executable identity

The next recovery audit found that `Test-WakeBridgeRuntime` still treated the fixed provisioned wake venv interpreter `.catdesk\wake-bridge\venv\Scripts\python.exe` as executable authority after only `Test-Path -PathType Leaf`. T-0355 now sends that exact path through the shared `Resolve-TrustedBootstrapHelperPath` boundary before the existing bounded health probe executes it. The shared boundary requires a real file leaf, rejects reparse points, normalizes expected/observed full paths, and requires exact identity, so a reparse-point or resolved-path replacement cannot redirect the probe. Invalid identity remains a fail-closed unhealthy-runtime result; browser ownership, repair semantics, bounded process execution, and the externally owned Secure MCP runtime are unchanged.

Regression coverage finds the production `Test-WakeBridgeRuntime` AST, requires the shared trusted resolver, rejects regression to the former leaf-existence-only check, and requires execution to remain through `Invoke-BoundedRecoveryHelper -FileName $python`. The focused Rust→PowerShell recovery harness passes, the sanctioned project verifier passes, all-target/all-feature tests pass, and strict all-target/all-feature Clippy passes. Accordingly T-0355 is implementation/repository-verification complete but not independently accepted. T-0319 remains open for genuinely separate final review of cumulative T-0325 through T-0355. See `docs/orchestrator/review_bundles/T-0355_WAKE_RUNTIME_INTERPRETER_IDENTITY_REVIEW_BUNDLE.md`.

## T-0356 cumulative addendum — autostart-supervisor public facade identity

The unattended-recovery audit then found that the persistent autostart supervisor invoked workspace-relative `catdesk.ps1` for both public `status` and `recover` without independently proving that the facade itself was the exact regular non-reparse workspace file. T-0351 hardens the recovery engine imported *inside* `catdesk.ps1`, so this was a pre-import authority seam rather than a duplicate of T-0351.

T-0356 adds `Resolve-TrustedLifecycleFacadePath`: the supervisor derives the exact expected facade from its already-resolved workspace root, requires `Get-Item -LiteralPath` to return a real `System.IO.FileInfo`, rejects `FileAttributes.ReparsePoint`, normalizes expected and observed full paths, and requires exact `OrdinalIgnoreCase` equality before either lifecycle action can execute. The supervisor still invokes only the public facade; its retry/backoff, mutex, runtime-pending behavior, and non-ownership of the official Secure MCP runtime are unchanged.

Regression coverage requires the trusted facade resolver to precede public invocation, requires the exact file/reparse/full-path checks, and rejects regression to leaf-existence-only trust. The sanctioned project verifier, all-target/all-feature tests, focused Rust→PowerShell lifecycle/recovery fixture, strict all-target/all-feature Clippy, and final `git diff --check` all pass; the diff check emits only the existing LF-to-CRLF working-copy warnings. Accordingly T-0356 is implementation/repository-verification complete but not independently accepted. T-0319 remains open for a genuinely separate final review of cumulative T-0325 through T-0356. See `docs/orchestrator/review_bundles/T-0356_AUTOSTART_SUPERVISOR_PUBLIC_FACADE_IDENTITY_REVIEW_BUNDLE.md`.

## T-0359 cumulative addendum — independent-review repair

A genuinely separate Codex/Terra-high T-0319 review became possible after the operator restored Codex CLI authentication/configuration. Session `adc-t0319-r1-independent-recovery-final-review-20260908` completed with `REJECT_WITH_CONCRETE_DEFECTS` rather than being treated as self-review. It identified one current handoff-authority defect and one review-time fixture failure.

The current defect was in `scripts/restart_catdesk_daemon.ps1`: the detached `restart_catdesk_daemon_worker.ps1` crossed the trusted Windows PowerShell `-File` execution boundary after only workspace-relative path construction plus leaf existence. T-0359 adds `Resolve-TrustedRestartWorkerScriptPath`, which derives the exact `$PSScriptRoot` worker path, requires a real `FileInfo`, rejects `ReparsePoint`, normalizes expected and observed paths with `IO.Path.GetFullPath`, and requires exact `OrdinalIgnoreCase` identity before the detached handoff. Existing old-process identity transfer, pinned-process termination/readiness, trusted OS PowerShell selection, reviewed promotion semantics, and external Secure MCP non-ownership remain unchanged. Regression coverage requires these guards and rejects the prior leaf-existence-only trust shape.

The same independent review observed one stale-canonical-daemon candidate-enumeration fixture failure. The first implementation-lineage reruns did not reproduce it, so T-0359 deliberately did not weaken production ambiguity/refusal or exact process-selection semantics merely to satisfy a one-off observation. A later genuinely separate re-review did reproduce the failure and produced the evidence that became T-0360 below. T-0359's worker-script identity repair remains independently confirmed, but T-0319 remains open until the T-0360 candidate receives a fresh separate verdict. See `docs/orchestrator/review_bundles/T-0359_INDEPENDENT_REVIEW_REPAIR_REVIEW_BUNDLE.md`.

## T-0360 cumulative addendum — daemon-candidate narrowing before bounded inventory cap

Fresh independent Codex/Terra-high session `adc-t0319-r2-independent-recovery-rereview-20260908` confirmed the T-0359 restart-worker identity fix and then reproduced the stale-canonical-daemon candidate-enumeration failure in the sanctioned recovery harness. A subsequent unchanged host-side rerun passed 3/3, establishing an intermittent selection problem rather than a permanently invalid fixture.

The bounded `Daemons` inventory helper already narrowed `Win32_Process` by `Name='catdesk.exe'`, but it applied `Select-Object -First 65` before the parent checked the required `--catdesk-daemon` mode token. On a host with enough same-image CatDesk GUI/other-mode processes, those unrelated rows could occupy the bounded set and hide the stale canonical daemon before exact canonical classification was possible.

T-0360 moves the existing daemon-mode narrowing into the provider-side read-only query: `Name='catdesk.exe' AND CommandLine LIKE '%--catdesk-daemon%'` is now applied before the 65-row cap. This does not make image name or command line destructive authority. The parent retains exact daemon-token verification plus canonical path/SHA-256, creation-time, pinned process-instance, listener, foreign-row, and ambiguity checks. More than 64 relevant daemon candidates still produces the one-extra-row fail-closed overload.

A deterministic source regression requires the daemon-token narrowing to occur before `Select-Object -First 65`; the real disposable stale-daemon integration fixture remains active. Two consecutive post-repair `cargo test --test recovery_powershell -- --nocapture` runs pass 3/3, and fmt, full all-target/all-feature tests, all-target/all-feature build, strict Clippy, and `git diff --check` are green. No live daemon/tunnel/browser/protected-target/promotion/Scheduler/Git mutation was used to obtain the evidence.

T-0360 is implementation/verification complete but does not self-accept T-0319. A fresh genuinely separate Codex/Terra-high reviewer must now decide the cumulative T-0325-through-T-0360 recovery chain. See `docs/orchestrator/review_bundles/T-0360_STALE_DAEMON_INVENTORY_ENUMERATION_REPAIR_REVIEW_BUNDLE.md`.

## T-0362 cumulative addendum — production inventory access repair

T-0361 independently reproduced the stale-fixture failure after T-0360.
T-0362 established that the actual user-level production mismatch was not only
the WQL filter: direct CIM and legacy WMI reads for the fixture PID both
returned `Access denied`, while the old helper used `SilentlyContinue` and
therefore emitted an empty candidate inventory. Fixed local limited-information
process handles now supply command line/path/creation observations, and native
TCP tables supply bounded loopback listener/PID observations. The helper emits
only token-matching daemon rows with the existing 65-row one-extra refusal and
a separate raw 257-row snapshot ceiling. Parent exact process/path/hash/
creation/handle/listener/ambiguity authority is unchanged. Verification is
green, but this implementation lineage did not self-accept T-0319.

## T-0363 cumulative addendum — independent T-0319 acceptance closure

A genuinely separate Codex/Terra-high reviewer then evaluated the cumulative
recovery candidate through T-0359/T-0362 and produced the required durable
verdict at
`docs/orchestrator/review_bundles/T-0363_T0319_R4_DURABLE_INDEPENDENT_VERDICT.md`.
The explicit verdict is **ACCEPT**.

That review rechecked the two last concrete rejection points rather than relying
on controller green state: the T-0359 detached restart-worker exact regular,
non-reparse normalized identity requirement remained present, and the T-0362
production stale-daemon inventory path correctly filters relevant daemon rows
before the bounded one-extra cap while leaving canonical path/SHA-256,
creation-time, pinned-process, listener, and destructive authority in the
recovery parent. The sanctioned Windows recovery suite passed 3/3, and the
reviewed completion verification recorded 897 passing tests.

T-0319 is therefore independently accepted and closed. Historical addenda above
that state `T-0319 remains open` retain their original point-in-time meaning and
are not rewritten. This acceptance closes the cumulative recovery/destructive-
process review gate only; it does not establish that the currently serving
canonical release contains the accepted source generation, does not live-accept
Qwen continuation, T-0223, or the native GUI, and does not authorize a raw
legacy reload or self-created reviewed-promotion authority. T-0324 protected
reviewed deployment/source parity is the next core blocker.
