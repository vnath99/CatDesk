# T-0423 Chat34 Recovery + Supervisor Current-Source Review

## Classification

`CURRENT_SOURCE_ELIGIBLE_FOR_FINALIZER_VERIFICATION_AND_FRESH_REVIEWED_SOURCE_FREEZE_IF_REQUIRED_PROFILES_PASS`

This is a current-source review of an already-dirty CatDesk candidate. It does **not**
retroactively attribute pre-existing implementation bytes to T-0423. Direct ChatGPT
review ownership was claimed before this bundle was created; this bundle is the
T-0423-attributable workspace mutation unless a concrete in-scope repair is later
required.

This review does not install, promote, reload, activate, or mint release/LKG,
supervisor, Wake, tunnel, target, or Git authority. A fresh reviewed-source freeze is
permitted only if CatDesk's direct-work finalizer independently captures the
authoritative diff/snapshot and all contract-required verification profiles pass.

## Reviewed current-source boundaries

### Wake dev.67 continuity

Installed immutable Wake remains
`1.0.0-dev.67-21592c86cff7-57791d5851a3`, bound to canonical Chat34 generation 17
and target digest
`8766b100ea0ba654bc4c8531d99c84e34346a231be1ffa0b4deae30df552427e`.

Manual installed-path diagnostics 007, 009, and 010 are accepted repeatability
passes: generation-bound `EXACT_USER_MESSAGE_APPENDED`, terminal `SENT`, response
timer `COMPLETE`, actionable queue zero, and browser cleanup/no replay. Diagnostic
008 remains immutable historical dev.66 ATTENTION evidence and is not a pass.
Supported installed-package restart continuity also passed while preserving the
010 receipt/timer and actionable queue zero.

The source-side response-completion fix accepts a completed-turn action that is
present in the DOM even when CSS hides it, while preserving exact latest-USER
binding, no-active-Stop, timeout-card, sequence, target, and no-replay constraints.
The fixed Python regression harness includes pre-write network-session retry bounds
and explicitly refuses fresh-session relaunch after the first browser write.

These are manual diagnostic/restart results. A fresh ordinary CatDesk review event
must still be delivered naturally by installed dev.67 before final natural-Wake
acceptance is claimed.

### One-command recovery

`scripts/start-catdesk-stack.ps1` now classifies local MCP readiness with a fixed
redacted vocabulary rather than collapsing every local failure to `PENDING`:

- `READY`
- `LOCAL_MCP_LISTENER_MISSING`
- `LOCAL_MCP_LISTENER_IDENTITY_MISMATCH`
- `LOCAL_MCP_LAUNCHED_PROCESS_MISMATCH`
- `LOCAL_MCP_RESPONSE_TIMEOUT`
- `LOCAL_MCP_RESPONSE_UNAVAILABLE`
- `LOCAL_MCP_RESPONSE_INVALID`
- `LOCAL_MCP_PROTOCOL_UNREADY`

The selective repair branch is intentionally narrow. It can recycle the pinned
canonical CatDesk listener only when all of these are true:

1. a listener exists and matches the canonical CatDesk identity;
2. persisted mode is `official_runtime`;
3. local MCP fails specifically as `LOCAL_MCP_RESPONSE_TIMEOUT`; and
4. the externally owned official runtime independently verifies `READY`.

That branch calls only the existing pinned-listener stop/replacement seam. It
contains no tunnel connect/stop/create/delete/setup migration authority. If runtime
health is unverified, the same local timeout performs no selective mutation.

`-Mode plan` now exercises the actual local MCP request path and official-runtime
verification read-only before reporting `PREFLIGHT_READY`. It can return
`LOCAL_MCP_NOT_READY` with the fixed local gate or
`OFFICIAL_RUNTIME_NOT_READY` without mutation.

The recovery fixture includes distinct timeout classification, healthy-runtime
selective local restart, unhealthy-runtime no-mutation behavior, plan-mode
no-mutation assertions, and a static guard against tunnel-mutation authority in
the timeout repair branch.

### Stable supervisor

The legacy `catdesk_release_recovery` path remains intentionally closed. The fixed
stable-supervisor lifecycle remains purpose-separated from the workspace recovery
script/tunnel owner.

Source-only read-only status observability now adds a bounded
`startupDefinition` label derived from the same fixed startup authority and
reviewed manifest used by the existing policy check. The labels distinguish:

- not evaluated / authority unavailable;
- planned action invalid;
- definition invalid;
- fixed definition absent;
- exact CatDesk-owned definition;
- foreign or ambiguous definition; and
- definition read failure.

This diagnostic does not relax `production_preflight()`, install anything, alter
Task Scheduler state, or probe the 3201 front door.

Fresh serving read-only evidence remains fail closed:

- readiness: `SUPERVISOR_ROOT_UNAVAILABLE`
- reviewed supervisor image: `SUPERVISOR_LIFECYCLE_READY`
- startup authority: `SUPERVISOR_LIFECYCLE_READY`
- startup policy: `SUPERVISOR_STARTUP_POLICY_UNPROVEN`
- preflight: `SUPERVISOR_STARTUP_POLICY_UNPROVEN`
- front door: `SUPERVISOR_3201_NOT_PROBED`

No stable-supervisor activation is authorized by this review while that gate is
unproven.

### Reviewed-build lineage

The current protected reviewed-build active generation is historical T-0411
authority, not a live worker. Its claim/owner/result chain is complete and terminal:
`BUILD_FAILED_OR_AMBIGUOUS`, no attestation, `REVIEWED_BUILD_FAILED`,
`CARGO_BUILD` exit 101, classification `CARGO_LINK_FAILED`.

That family is retryable terminal evidence. It must not be deleted, rewritten, or
blindly retried. A newly acknowledged independent review of the current source may
legitimately supersede it through the existing terminal-retry lineage mechanism.

## T-0423 focused verification

Performed after direct-review ownership was claimed:

1. `cargo test --locked --offline lifecycle_and_reviewed_release_recovery_fixtures_pass`
   - PASS: 1 passed / 0 failed.
   - Durable command log:
     `.catdesk/logs/1790378971-f6adb695-a43a-405d-b4a1-8ccdbf1bf8a9.log`.

2. `cargo test --locked --offline supervisor_lifecycle`
   - PASS: 15 passed / 0 failed in the relevant main suite.
   - Includes stable-supervisor MCP schema/discovery/side-effect-free status,
     fail-closed preflight, exact reviewed fixture lifecycle, and no legacy
     recovery-owner fallback assertions.
   - Durable command log:
     `.catdesk/logs/1790378982-9fc64379-0aae-4869-9591-8a6536ddd6dd.log`.

3. `git diff --check`
   - PASS, exit 0.
   - Existing LF/CRLF conversion warnings only.
   - Durable command log:
     `.catdesk/logs/1790378995-06115452-9a19-4ee0-a77c-5dde692a50c4.log`.

CatDesk direct-work finalization still owns the authoritative required profiles:
approved project recovery tests, full Cargo test, strict all-target/all-feature
Clippy, authoritative diff capture, and independent final review. This bundle must
fail closed if that finalizer does not pass.

## Scope and non-actions

T-0423 did not:

- claim authorship of the pre-existing recovery, supervisor, or Wake changes;
- alter the external Secure MCP runtime or its ownership;
- replay or manufacture a Wake event;
- activate/probe 3201;
- install a stable supervisor root/startup definition;
- mutate reviewed-build/promotion/LKG state;
- clean/reset/commit/push the dirty repository; or
- mutate any external project.

## Review conclusion

The inspected current source preserves the intended fail-closed boundaries for
local-MCP timeout recovery, external tunnel ownership, stable-supervisor startup
policy, and Wake exactly-once behavior. No in-scope source defect was found by the
focused review.

The candidate is suitable to enter CatDesk's authoritative direct-work finalizer.
Only a successful finalizer plus independent final review may establish a fresh
reviewed-source authority. After that authority exists, the next protected release
steps are a fresh reviewed build/promotion attempt, followed by live one-command
recovery acceptance and legitimate stable-supervisor provisioning. Separately, the
new independent-final-review event from this session is the preferred ordinary
CatDesk event for final natural dev.67 Wake acceptance in canonical Chat34.
