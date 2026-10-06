# T-0302 T-0222/T-0139 Visible GUI Acceptance Readiness Review Bundle

## Scope and preserved boundary

T-0302 is a repository-only readiness audit for the later literal native
Windows GUI acceptance. It preserves T-0301's exact
`OPERATOR_BOOTSTRAP_REQUIRED` classification: the pre-T-0299 live daemon must
first be replaced through the approved fixed reviewed-image procedure before
the reviewed MCP lifecycle can be used. This ticket performed no daemon
replacement, supervisor activation, browser wake, target update, or Secure
MCP/tunnel operation.

T-0302 does **not** accept T-0222/T-0139. Source readiness and deterministic
tests cannot substitute for a visible interactive-host window and the required
host observations below.

## Reconstructed literal acceptance criteria

| Required later observation | Repository readiness evidence | Acceptance class |
| --- | --- | --- |
| One native `CatDesk Binagotchy` top-level window and taskbar entry in an interactive Windows session | Closed `--catdesk-binagotchy-gui` mode creates a fixed Win32 class/title and visible top-level window | Host-visible only |
| No duplicate on a repeated supported lifecycle launch | Fixed local GUI mutex restores/foregrounds the existing window | Host-visible only; source-tested composition |
| Session-zero/headless host remains GUI-free | Lifecycle tests `Environment.UserInteractive` and a positive session ID before launching the zero-argument GUI mode | Host-visible only; source-tested refusal |
| Designated Chat URL is the current authoritative project/effective-wake readback | T-0292 production controller uses `operator_read_designated_chat_target` at construction and `Refresh readback`; fixtures are isolated | Host-visible current-state observation |
| Readiness card shows overall plus T-0224, T-0223, T-0222/T-0139, and T-0152 rows | T-0294 renders only the T-0293 presentation | Host-visible rendering; no gate acceptance inferred |
| Refresh reads rather than mutates | T-0292/T-0294 controllers replace UI state from their existing authority readers only | Host-visible no-op observation; deterministic tests cover it |
| Closing the window does not stop CatDesk | `WM_CLOSE` destroys only the GUI view; the lifecycle considers GUI launch best-effort after verified daemon/runtime readiness | Host-visible only |

T-0222/T-0139 requires literal evidence of the first column. The presence of
the mode, a source test, a fixture, provider completion, or a readiness card
that says `READY` is not that evidence.

## Native GUI composition audit

`src/main.rs` accepts the GUI only through the closed one-argument mode and
passes the current working directory to `windows_gui::run_gui`. The supported
public lifecycle is `catdesk.ps1 start` or `catdesk.ps1 recover`; after its
canonical binary/fingerprint and local/runtime readiness checkpoints,
`Invoke-CanonicalStackBootstrap` calls `Start-CatDeskBinagotchyGui` only when
the process is interactive and has a nonzero session. The launcher passes only
the verified canonical binary and fixed `--catdesk-binagotchy-gui` argument.

The native GUI creates two production controllers from that fixed workspace:

| UI surface | Production authority | Mutation boundary |
| --- | --- | --- |
| `Designated Chat URL` / `Refresh readback` | Existing guarded project/effective-wake target readback | Editing is in-memory. Only explicit `Apply/Update` can attempt the existing CAS; it is not exercised by this ticket or required for visible-GUI acceptance. |
| `Core Acceptance Readiness` / `Refresh readiness` | Existing T-0293 `read_fixed_core_host_acceptance_preflight` presentation | Strictly read-only; refresh replaces in-memory display data. It has no evidence writer, supervisor, browser, target, daemon, or tunnel authority. |

No hidden browser, shell input, caller-selected path, or manual data injection
is needed to display either current production readback surface. The separate
test fixture traits model only those readers and are not reachable from the
native production constructors.

## Exact future host procedure

This procedure is deferred until the T-0301 operator bootstrap has deployed
the independently reviewed T-0299 image and T-0223 has separately completed
its approved status/preflight/activation/continuity acceptance. It is not an
instruction executed by T-0302.

1. From an interactive, nonzero-session Windows desktop, use the supported
   public `catdesk.ps1 start` or `catdesk.ps1 recover` lifecycle surface; do
   not invoke the GUI binary directly or select a path.
2. After its ordinary verified lifecycle status, observe exactly one visible
   `CatDesk Binagotchy` window and taskbar entry.
3. Observe the designated-chat field as authoritative readback, including the
   fixed redacted status. Do not edit or Apply a target merely as acceptance
   proof.
4. Observe the Core Acceptance Readiness card's overall classification and
   T-0224, T-0223, T-0222/T-0139, and T-0152 rows. `READY` means only readiness
   for its next ordered live step unless exact durable evidence says otherwise.
5. Use `Refresh readback` and `Refresh readiness`; verify they update the
   presentation from authoritative readers without a browser wake, target
   change, supervisor action, daemon/tunnel action, or new acceptance claim.
6. Run the same supported lifecycle surface once more. Confirm the existing
   window is foregrounded/restored rather than duplicated.
7. Minimize and close the GUI. Confirm the already verified daemon remains
   observable through the supported status surface. Separately verify a
   headless/session-zero invocation does not start a window.

Only a reviewer observing these host effects may record literal T-0222/T-0139
evidence. A negative or unavailable result remains fail-closed and must not
displace the valid daemon/backend.

## Deterministic evidence and verification

| Check | Result |
| --- | --- |
| `cargo test windows_gui --all-targets --all-features` | Passed: 19 GUI/controller/read-only/closed-mode tests. |
| `cargo test core_host_acceptance_preflight --all-targets --all-features` | Passed: 9 fail-closed evaluator tests, including wrong target/project/session and source/provider-only refusal. |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1` | Passed: canonical bootstrap behavioral test, including GUI launch composition and headless skip seams. |
| `cargo fmt --all -- --check` | Passed. |
| `git diff --check` | Passed; accumulated pre-existing CRLF notices were non-fatal. |

The existing workspace warning that `<USER_PROFILE>
canonicalized was non-fatal for the two Cargo test invocations.

## Files and prohibited-action audit

Attributable T-0302 files are:

- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- this bundle

No product source required repair. No browser/profile inspection or wake,
target/config/registry mutation, daemon replacement, supervisor activation,
ProgramData/Scheduler/service operation, Secure MCP/tunnel operation,
external-project mutation, signing/provenance/dedicated-producer work, Git
staging/commit/publication, reset, clean, or revert occurred.

## Residual dependency and next safe action

The active broad order remains **T-0223 -> T-0222/T-0139 -> T-0152 -> T-0155**.
T-0223 is parked only for the T-0301 operator bootstrap: deploy the
independently reviewed T-0299 image through the approved fixed reviewed-image
release procedure, then reconnect and use only the fixed read-only lifecycle
tools before any separately authorized activation decision. Safe
repository-only work may continue while that host action is pending.

**Status: READY_FOR_INDEPENDENT_REVIEW.**
