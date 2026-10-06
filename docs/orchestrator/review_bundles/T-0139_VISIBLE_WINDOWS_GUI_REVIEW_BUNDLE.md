# T-0139 / T-0128 - Visible Windows Binagotchy GUI (R2 acceptance audit)

## R1 diagnostic and R2 scope

The R1 implementation added a real native GUI, but the independent review
could not use its repository-wide diff for attribution: the workspace already
contained many unrelated modified and untracked paths from earlier tickets.
The previously required T-0139 review bundle was also absent from the evidence
presented to that review.  This R2 audit preserves all unrelated dirt, records
the focused boundary below, and refreshes this required bundle in this session.

The current R1 Codex diagnostic records two repaired implementation defects
from the initial attempt (the `PAINTSTRUCT` child rectangle needed `Copy`, and
the source-ownership regression must not match its own test strings).  The
current source contains both repairs; focused and full verification below pass.

## Exact T-0139 attribution boundary

This is the task attribution set, not `git diff HEAD`:

- `src/windows_gui.rs`
- `src/main.rs` (only the native GUI module declaration, exact mode routing,
  and routing regression)
- `scripts/start-catdesk-stack.ps1`
- `scripts/test-start-catdesk-stack.ps1`
- `src/delegated/autonomy_observability.rs` (R2's minimal provider-route
  presentation correction: explicit Qwen actor/provider classification)
- `docs/orchestrator/review_bundles/T-0139_VISIBLE_WINDOWS_GUI_REVIEW_BUNDLE.md`

Repository-wide `git diff` and `git status` intentionally contain unrelated
prior-ticket changes and untracked paths.  They are **not** the T-0139
attribution boundary and were neither reset, stashed, reverted, cleaned, nor
otherwise changed by this work.

Focused audit at R2 close recorded `src/main.rs` as modified and the five
other T-0139 paths above as untracked in this intentionally dirty checkout.
The broad `git status --short` contained 117 entries.  That number and the
broad diff are context only; the explicit list above is the review boundary.

## Before-state and production lifecycle

The supported `catdesk.ps1 start` and `recover` paths enter
`Invoke-CanonicalStackBootstrap` in `scripts/start-catdesk-stack.ps1`.  Before
T-0139, that path started the canonical executable only as
`--catdesk-daemon` with `-WindowStyle Hidden`; `run_native_daemon` owns the
local daemon and has no desktop window.  The existing Ratatui/crossterm UI is
a terminal fallback, not a Windows taskbar window.

After canonical daemon/runtime readiness and only after the operational
checkpoint, the supported lifecycle calls `Start-CatDeskBinagotchyGui`.  It
first requires `Environment.UserInteractive` and an interactive process
session (`SessionId > 0`).  Session-zero/headless/service contexts therefore
remain daemon-only.  The launch is intentionally best-effort after readiness:
a read-only window failure cannot alter the verified daemon or externally-owned
Secure MCP transport state.

## Native desktop implementation and ownership boundary

`--catdesk-binagotchy-gui` is a closed zero-argument mode.  Its Windows code:

- registers the fixed `CatDeskBinagotchyGuiV1` class and calls
  `CreateWindowExW` with `WS_OVERLAPPEDWINDOW | WS_VISIBLE`, creating a real
  top-level desktop window/taskbar presence named `CatDesk Binagotchy`;
- owns the fixed local mutex `Local\\CatDesk.BinagotchyGui.V1`; on a later
  invocation it waits at most 500 ms for the fixed window, then uses
  `FindWindowW`, `ShowWindow(SW_RESTORE)`, and `SetForegroundWindow` rather
  than creating a duplicate;
- uses normal window handling for minimize, and handles `WM_CLOSE` only by
  destroying the GUI window; `WM_DESTROY` ends only the GUI message loop.

The module never creates `AppState`, starts the native daemon, starts a
reviewed-build worker, stops CatDesk, or owns/restarts Secure MCP.  Closing or
minimizing the GUI therefore cannot stop the daemon.  Explicit `catdesk.ps1
stop` remains the only supported daemon-stop operation.

## Read-only operator fields and T-0056/T-0076 semantics

The GUI calls the existing durable `read_snapshot` reader for
`.catdesk/autonomy`; it does not create a second orchestration truth source or
call MCP to mutate state.  It shows:

- overall status and the positive-evidence actor: `CODEX_PROVIDER`,
  `QWEN_PROVIDER`, `CATDESK`, `VERIFIER`, `WAKE_BRIDGE`, `CHATGPT_WEB`,
  `OPERATOR`, waiting/idle/unknown as the existing T-0056 precedence model
  determines;
- distinct durable provider route: `CODEX`, `QWEN`, rate-limit/unavailable
  variants, or `CHATGPT_WEB`; an unproven route never becomes an active actor;
- project, ticket, session/state, model, and reasoning;
- active, wall-clock, waiting, evidence-completeness, and `CHATGPT_WEB`
  timing evidence from T-0055/T-0056 accounting;
- last step, next expected action, wake policy, unread/pending attention; and
- redacted tunnel mode plus a 125 ms loopback-only local-MCP readiness label.

R2 fixes the concrete Qwen omission from R1: a live provider turn with the
durable `QwenFallbackActive` route now renders `QWEN_PROVIDER` / `QWEN`; the
same route without positive live evidence remains `UNKNOWN` rather than being
presented as active work.

## Verification evidence

Focused checks:

- `cargo test windows_gui -- --nocapture` - 4 passed.
- `cargo test
  autonomy_observability::tests::qwen_provider_route_is_rendered_without_claiming_activity_from_route_alone
  -- --nocapture` - 1 passed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File
  scripts/test-start-catdesk-stack.ps1` - passed.  Its fakes prove headless
  skip and the exact `--catdesk-binagotchy-gui` lifecycle launch after verified
  readiness; no real window, daemon, or tunnel is started.

Required Rust verification:

- `cargo fmt --check` - passed.
- `cargo clippy --all-targets --all-features -- -D warnings` - passed.
- `cargo test` - 691 passed, 0 failed, 21 existing ignored; the two existing
  Windows lifecycle tests and two existing measure tests also passed.
- `cargo build` - passed.
- `git diff --check` - passed.

## Known limitations and independent live Windows acceptance

No production release, daemon, GUI, tunnel, browser wake, or service was
activated during this task.  A separate approved interactive-host reviewer
must perform the following:

1. In an interactive Windows desktop session with an already valid canonical
   release, run supported `catdesk.ps1 start` or `catdesk.ps1 recover`.
2. Confirm one visible `CatDesk Binagotchy` top-level window/taskbar entry
   appears after the normal verified lifecycle result and its content is
   redacted/local-only.
3. Run the same supported lifecycle command again.  Confirm the existing
   window is restored/foregrounded, not duplicated, while the daemon remains
   singular.
4. Minimize, then close the GUI.  Confirm `catdesk.ps1 status` still observes
   the local daemon; only explicit `catdesk.ps1 stop` may stop it.
5. Exercise a session-zero/headless/service invocation and confirm that it
   does not attempt a GUI while preserving normal daemon/tunnel behavior.
6. Exercise a durable Qwen fallback and ChatGPT review/waiting state, if
   available, and confirm the provider/actor labels match the read-only
   snapshot semantics above.

This bundle documents a focused implementation audit and local verification.
It does not claim independent CatDesk/ChatGPT acceptance or any live-host GUI
result.

## R3 correctness closure (T-0220)

### Defects found

Independent review identified two bounded native-GUI correctness defects in
the R2 source:

1. `render_snapshot` applied `String::truncate(MAX_GUI_TEXT_CHARS)`.  The
   limit is a display-character limit, but `String::truncate` accepts a UTF-8
   byte index.  A project, ticket, model, status, or step containing a
   multibyte character at the 2,048-character boundary could therefore panic.
2. The Win32 message loop treated every `GetMessageW` result `<= 0` as normal
   completion.  `0` is the normal `WM_QUIT` result, while `-1` is an API
   failure and must not silently report GUI success.

### Exact R3 changes

Only these paths are attributable to the T-0220 repair:

- `src/windows_gui.rs`
- `docs/orchestrator/review_bundles/T-0139_VISIBLE_WINDOWS_GUI_REVIEW_BUNDLE.md`

`truncate_display_characters` now collects at most
`MAX_GUI_TEXT_CHARS` Unicode scalar values.  It replaces the byte-index
truncate and leaves the existing per-field control-character sanitization in
`bounded` intact.  The rendered display is consequently capped at 2,048
characters without a UTF-8-boundary panic.

`MessagePumpDisposition` and `classify_message_pump_result` make the Win32
contract explicit: a positive `GetMessageW` result dispatches the message,
zero quits normally, and a negative result returns the fixed bounded error
`CATDESK_GUI_MESSAGE_PUMP_UNAVAILABLE`.  Cleanup of the workspace slot and
single-instance mutex still happens before that result is returned; no raw OS,
path, credential, or platform error detail is surfaced.

The focused tests cover ASCII exactly at and beyond the cap, an emoji at the
old unsafe byte boundary, CJK content, short Unicode, control-character
sanitization, and classifier values `-1`, `0`, and positive dispatch values.
They use the pure classifier only; this repair does not try to manufacture a
live Win32 API failure.

### Preserved R2 invariants

This repair did not change the closed zero-argument GUI mode, the fixed mutex
and restore/foreground behavior, the top-level Windows window construction,
read-only snapshot sourcing, actor/provider mapping, interactive/headless
lifecycle policy, best-effort launch behavior, or daemon and Secure MCP
non-ownership.  It does not launch a GUI, daemon, tunnel, browser wake, or
any lifecycle operation.

The repository remains intentionally dirty from unrelated prior tickets.  The
repository-wide status/diff is not the T-0220 attribution boundary; the two
paths above are.  Both were untracked already as part of the accepted T-0139
R2 focused set, and this R3 update changes their contents without resetting or
altering unrelated worktree state.

### R3 verification evidence

- `cargo test windows_gui -- --nocapture` — 8 passed, 0 failed.
- `cargo fmt --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test` — 695 unit tests plus 4 integration/measure tests passed; 21
  existing host-specific tests remained ignored; 0 failed.
- `git diff --check` — passed.

Visible interactive-desktop activation remains pending separate independent
host acceptance.  The R2 live-host procedure above remains the required way
to verify the taskbar window, singleton foregrounding, headless skip, and
daemon independence without using this provider turn to start production
services.
