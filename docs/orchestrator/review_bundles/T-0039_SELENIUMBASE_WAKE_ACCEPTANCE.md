# T-0039 / T-0047F SeleniumBase Wake Acceptance

Status: `SOURCE_REPAIR_COMPLETE__OPERATOR_PROFILE_AND_LIVE_ACCEPTANCE_PENDING`.

## Wake-Bridge Repair

`scripts/wake_bridge.py` now treats the review inbox as an event source, not
as a source of project content. Its only browser message contains one durable
record ID and a fixed direction to inspect CatDesk MCP for the current session,
event, diff, verification, accounting, and plan before continuing the bounded
Codex loop.

The durable v2 bridge state records a bounded wake lifecycle for each claimed
record: event creation time when supplied by the inbox, claim time, observable
browser-send time, latency, result, and a fixed operator-attention category.
It stores no page content, configured URL, cookies, tokens, browser storage,
driver diagnostics, command lines, or credential values.

On initial setup, historical unread records with a durable creation time before
the initialization watermark are retired in the bridge ledger rather than
mistaken for a fresh wake. `--retire-backlog` explicitly retires the remaining
backlog without altering CatDesk's durable inbox. Pre-claim persistence still
makes a browser crash fail closed: a wake can be lost, but cannot be sent
twice after restart.

The SeleniumBase sink remains opt-in and requires an existing dedicated
operator-authenticated profile plus an exact credential-free ChatGPT HTTPS
conversation URL. It fails closed for origin drift, CAPTCHA/security checks,
missing or ambiguous composer/send controls, existing drafts, profile/login
unavailability, and unexpected browser errors. It neither opens profile files
nor reads browser storage.

T-0039-R1 adds bounded UI timing: the sink polls only for a unique visible
composer and send control (20 seconds by default, configurable only within
safe bounds), rechecks CAPTCHA/login state while waiting, and waits up to 15
seconds for the composer to clear after the click. The recorded browser-send
time is produced only after that observable UI confirmation; wake latency is
then measured from the durable event creation time to that confirmation, not
merely to the pre-click claim.

T-0039-R2 adds a local operator-bootstrap gate. `--bootstrap` requires an
exact credential-free conversation URL and a project-local dedicated-profile
path, writes a minimal schema-v1 local configuration only when explicitly
requested, and can create an empty profile directory only with a separate
flag. It neither opens SeleniumBase nor reads profile files. Normal runs reject
configuration/profile paths outside the workspace, unrecognized config fields,
and the former scheduled-poll/provider-bypass setting; only the exact browser
sink can produce a `SENT` wake record.

T-0039-R3 closes the setup gap with `scripts/bootstrap_wake_bridge.ps1` and a
pinned isolated requirement manifest. The script creates the virtual
environment only with `-InitializeEnvironment`, installs SeleniumBase only
with `-InstallSeleniumBase`, and writes the bridge configuration only with
`-WriteConfig`. It refuses to replace an existing environment, constrains the
profile to the workspace, and neither launches a browser nor reads credentials.
All generated config, environment, profile, and bridge state are under the new
ignored `.catdesk/wake-bridge/` path. The worker did not execute this script or
perform dependency installation.

T-0039-R4 updates the exact composer write path for the current send state. It
uses the `#prompt-textarea` textbox without relying on a changeable visible
label. After typing, the bridge reacquires a unique visible current send button
and waits for it to be enabled before clicking. Ambiguous or still disabled
controls remain fail-closed selector conditions.

T-0039-R6 closes the editor-identity gap. The selected control must have the
exact `prompt-textarea` ID and be either the contenteditable editor or a native
textarea editor. Draft and
post-submit checks use the appropriate `textContent` or `value` property for
that verified identity; a differently named editor fails closed before typing.

T-0039-R7 moves the one-time MCP readiness failure boundary ahead of the live
runner's durable inbox write. The runner requires the local ignored receipt and
uses `wake_bridge.py --preflight` to validate its exact configured
conversation/profile binding and age before it creates the R7 synthetic event.
Preflight does not launch SeleniumBase, open a browser, change bridge state, or
consume the receipt; consumption remains atomic immediately before the one
actual browser send.

T-0039-R8 removes the stale dependency on a
`form[data-type='unified-composer']` wrapper. The sink now binds directly to
one visible exact `#prompt-textarea` editor and one visible current send button.
The editor identity, uniqueness, draft, enabled-send, login/CAPTCHA, and
post-submit checks remain fail closed; a missing or ambiguous wrapperless UI is
not accepted.

T-0039-R9 handles the wrapperless editor variant that has no current send
control. After typing into the already-validated exact editor, the sink sends
Enter only when no matching send button exists at all; a disabled or ambiguous
button still fails closed. The existing bounded editor-clear check is required
to treat either route as submitted.

T-0039-R10 handles the current roleless contenteditable variant. The sink still
requires the exact `prompt-textarea` ID and `contenteditable='true'`, but treats
the ARIA textbox role as optional. Native exact-ID textareas remain supported;
other element IDs and non-contenteditable non-textarea controls fail closed.

T-0039-R11 closes the concurrent-poller gap observed in the R10 durable state:
the bridge now holds one fixed project-local OS file lock for every mutating
poll, retirement, or delivery loop. A second current-version process returns
the bounded `WAKE_BRIDGE_BUSY` status before it can consume a readiness receipt,
claim an event, write bridge state, or launch SeleniumBase. The lock is tied to
the open process handle and is therefore released on normal exit or process
termination; it contains no browser or credential data.

## CDP phase-one consolidation

The production `wake_bridge.py` sink now follows the proven SeleniumBase UC +
CDP smoke-test path: `activate_cdp_mode`, exact-target validation, bounded
Chrome network-error recovery, editor and idle waits, existing-draft rejection,
native CDP `press_keys`, a safe CDP send click (or native Enter only when no
current send control is visible), and editor-clear confirmation. It preserves
the dedicated profile/config binding, singleton, durable claim-before-browser
idempotency, and concise accounting categories.

The production bridge no longer reads, writes, or consumes `MCP_READY`/
`mcp-ready.json` receipts and no longer exposes `--preflight`; those were
historical live-acceptance scaffolding. The new automated resume prompt prefixes
the triggering review record ID and directs ChatGPT to authoritative CatDesk
state without embedding review-bundle content. Phase 2 remains responsible for
deciding controller-owned automatic invocation.

## Deterministic Coverage

- Existing idempotency, debounce, invalid-inbox, and generic-error-redaction
  coverage remains.
- New tests cover the state-light event-aware message, initial stale backlog
  retirement, explicit idempotent retirement, and claim-to-send latency/result
  accounting.
- Timing/UI tests cover delayed composer readiness and return a browser-send
  timestamp only after the composer clears following the click.
- Bootstrap tests cover creation of an empty project-local profile/config and
  rejection of paths outside the workspace.
- The PowerShell bootstrap script is designed for parser-only offline
  validation; its environment, package installation, configuration, and any
  later manual login are explicit operator actions.
- Current-UI send coverage verifies the post-typing button is reacquired and
  enabled before the one permitted click.
- Editor-identity coverage accepts the two exact prompt-editor forms and
  rejects a similarly shaped but differently named editor.
- R7 coverage invokes the preflight path with a valid local receipt and proves
  it returns ready without consuming the receipt or creating durable state.
- Wrapperless-composer coverage proves the exact editor/send pair can send
  without the historic form wrapper.
- R9 coverage proves a hidden current send button falls back to Enter on the
  exact editor and still requires observable editor clearing.
- R10 coverage accepts an exact roleless contenteditable editor while retaining
  rejection of a differently named editor.
- R11 coverage proves cross-process singleton exclusion, automatic release
  after the holder exits without cleanup, and no poll/state write by a busy
  contender.
- No browser, SeleniumBase environment, ChatGPT conversation, profile,
  credential, cookie, or CatDesk MCP operation was used by this worker.

## Verification

### R7/R8/R9/R10/R11 repair checks

- PowerShell parser validation of `scripts/run_live_wake_acceptance.ps1`:
  passed.
- `git diff --check`: passed (with existing repository CRLF normalization
  warnings).
- Static selector check: passed; the bridge no longer contains a
  `form[data-type='unified-composer']` prerequisite and the wrapperless-editor
  regression is present.
- Static Enter-fallback check: passed; the hidden-button regression targets
  only the exact editor and the implementation still uses the bounded
  post-submit editor-clear confirmation.
- Static roleless-editor check: passed; exact contenteditable selector and
  identity logic no longer require an ARIA role, while the exact-ID requirement
  remains.
- Static singleton check: passed; every mutating CLI path is enclosed by the
  fixed lock, and a busy acquisition returns the bounded status before calling
  `poll_once`.

### CDP phase-one checks

- Production source was checked for the native CDP activation, exact-target,
  network reload/get, idle, `press_keys`, click/Enter, and editor-clear paths;
  the obsolete receipt/preflight production symbols are absent.
- The existing project-local wake Python executable could not start because its
  configured base Python 3.12 executable is missing. No interpreter or browser
  dependency was installed or altered, and no live browser wake was attempted.
- `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D
  warnings` passed. `cargo test --no-fail-fast` completed with 433 passed, 7
  failed, and 11 ignored; the failures are the known missing-advisor executable
  and Windows process-tree access-denied host limitations, not wake-adapter
  tests.
- The deterministic Python regression was not run: `python` is unavailable on
  this worker PATH, and no environment was installed or changed. The worker did
  not run the live acceptance script, launch a browser, access a profile or
  credential, or invoke CatDesk MCP.

- `python -m unittest tests/test_wake_bridge.py -v`: blocked because neither
  `python` nor `py` is available on this worker PATH; no Python runtime was
  installed or altered.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test --no-fail-fast`: 431 passed, 7 failed, 11 ignored. The seven
  failures are pre-existing host-environment limitations (three missing advisor
  programs, one advisor-dependent cancellation assertion, and three Windows
  process-cancellation access-denied assertions); none exercise this Python
  wake bridge.
- `git diff --check`: passed. Git emitted existing CRLF normalization warnings.

## Live Acceptance Gate

The operator bootstrap is now collapsed to one command from the CatDesk project
root:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File ".\\scripts\\setup_wake_bridge.ps1"
```

The wrapper prompts once for the exact current ChatGPT conversation URL,
discovers local Python 3, creates/reuses the project-local wake virtual
environment, upgrades pip, installs the pinned SeleniumBase dependency, writes
the bounded local bridge config, opens the dedicated profile for manual ChatGPT
authentication, retires the historical inbox backlog after the operator closes
the setup browser, and runs the deterministic Python wake tests. It never reads
or prints cookies, browser storage, passwords, tokens, or profile files.

After the setup reports `WAKE_BRIDGE_SETUP_READY`, CatDesk/ChatGPT must create
exactly one fresh review-inbox event and run the isolated wake bridge. Live
acceptance requires one submitted wake message, bounded event-created-to-send
latency in wake accounting, no duplicate on the next poll, and a real new
ChatGPT turn that inspects CatDesk MCP and continues the autonomous Codex loop
without user prompt relay.
