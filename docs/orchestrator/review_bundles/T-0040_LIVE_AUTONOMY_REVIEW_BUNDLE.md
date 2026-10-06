# T-0040 Live Autonomy Review Bundle

Status: `PARTIAL_WITH_OPERATOR_AND_PLATFORM_GATES`.

## Executive Summary

T-0040 proved a safe detached CatDesk daemon replacement without touching the
existing external Secure MCP tunnel. The replacement now selects the existing
Control Computer mode through an explicit local runtime flag rather than
synthetic console input. Local MCP and tunnel readiness recovered after the
restart.

The required existing Codex conversation was discovered through the supported
direct app-server protocol, but it is not eligible for the requested binding:
its canonical workspace does not match this worktree and it cannot accept
direct input. CatDesk correctly did not bind or resume it.

Python is installed locally and the deterministic wake-bridge tests pass.
SeleniumBase and a dedicated manually authenticated browser profile remain
operator-gated, so no browser was launched and no wake message was sent.

## Live Runtime

| Capability | Status | Evidence |
| --- | --- | --- |
| Detached daemon replacement | PASS | Restart handoff recorded `RECOVERED_PENDING_TRANSPORT_CHECK`; the replacement owns loopback local MCP. |
| Local MCP | READY | Local `initialize` and `tools/list` returned HTTP 200; 61 tools were listed. |
| Secure MCP tunnel | CONNECTED_VERIFIED | Existing tunnel `/healthz` and `/readyz` returned HTTP 200 after restart. |
| Updated binary | ACTIVE | Release binary was rebuilt from this worktree before the final handoff. |
| Operator-local Codex executable | AVAILABLE | Presence was validated only; its value was not read into logs or state. |

The tunnel process was not enumerated for control, stopped, restarted, or
reconfigured by the handoff helper.

## Restart Correction

The prior helper failed because it launched a hidden interactive TUI and tried
to infer readiness from the previous listener list. The corrected path:

1. requires the operator-local Codex executable setting to already be present;
2. stops only the explicit CatDesk PID;
3. launches the release binary with `--auto-start-computer`;
4. waits for that replacement PID to own the configured loopback MCP port and
   return HTTP 200 at the local root health endpoint;
5. persists bounded handoff state and leaves the external tunnel untouched.

Normal interactive launches are unchanged. `--auto-start-computer` is explicit
and rejects duplicate use.

## Existing Codex Thread Binding

The installed Codex CLI direct app-server protocol initialized successfully.
Its current `thread/list` response uses `result.data` and thread `name`, so the
CatDesk parser was updated to accept that shape while preserving fail-closed
ownership handling.

That parser correction is source-verified but was not rebuilt and redeployed
after the binding gate: there was no safe live thread to bind, so T-0040 did
not restart the healthy daemon merely to expose an unusable control path.

The requested title had one nonsecret candidate:

| Candidate ID | Canonical workspace match | Direct input available | Result |
| --- | --- | --- | --- |
| `019f4b3a-c8cb-7c50-8a6c-323f603f35ff` | No | No | `THREAD_BINDING_MISMATCH` |

No binding, resume, or new routine Codex thread was created. The required
CatDesk worktree binding remains blocked until an exact workspace-matching,
non-owned thread is available or the operator explicitly changes the binding
decision.

## Accounting And Review Inbox

The live MCP surface includes `autonomy_execution_accounting`,
`autonomy_review_inbox_list`, and `autonomy_review_inbox_ack`. Existing durable
accounting and review-inbox behavior remains covered by deterministic Rust
tests. No new meaningful provider task was run after the thread-binding gate,
so there is no new authoritative Codex usage delta. Historical T-0031 remains
`630023 ms` with usage `UNKNOWN_NOT_CAPTURED`.

The generic project-registry/scheduler module exists, but a project-registry
MCP control surface and the requested real-project registrations were not
claimed complete because the exact CatDesk thread binding is blocked.

## Wake Bridge

Python 3.12 and 3.14 were discovered locally. SeleniumBase was not present in
the discovered interpreters. No dependency was installed and no profile,
credential, cookie, browser storage, CAPTCHA, or ChatGPT UI was accessed.

`tests/test_wake_bridge.py` passed using the existing Python 3.12 interpreter:
5 deterministic tests passed. Live wake acceptance, browser selector checks,
and exact dedicated-conversation MCP acceptance remain operator-gated pending a dedicated
manually authenticated profile plus an approved project-local SeleniumBase
installation.

## T-0040 Dedicated Wake/MCP Gate Repair

The setup path now calls `wake_profile_login.py --require-mcp-ready`. In the
operator-owned headed browser, it opens only the configured exact conversation
and instructs the operator to enable CatDesk MCP there. Setup proceeds only
after the fixed `MCP_READY` acknowledgement. The helper neither probes CatDesk
MCP nor reads page text, connector state, credentials, cookies, browser
storage, or profile files. This makes the required dedicated-conversation/MCP
handoff explicit while leaving acceptance of the actual MCP turn to CatDesk.

T-0040-R1 binds that acknowledgement to normal bridge execution. The helper
writes an ignored, non-secret receipt for the exact conversation/profile pair;
the bridge rejects a missing, malformed, stale (over four hours), or mismatched
receipt before it can claim a wake event. Deterministic coverage includes exact
target binding, stale-receipt rejection, and receipt field minimization.

T-0040-R2 makes that handoff single-use. The receipt is atomically claimed and
consumed only after a fresh pending event is selected and immediately before
the browser write boundary. A no-op/debounced run retains the receipt, but any
delivery attempt consumes it; a later event therefore requires a new explicit
MCP confirmation. A competing or crashed receipt claim fails closed rather than
permitting a second browser delivery.

The `catdesk_wake_bridge_run_once` MCP tool is now registered in the
write-capable CatDesk surface. It requires an explicit boolean confirmation and
has a fixed project-local invocation only; ChatGPT cannot supply another
executable, profile, conversation URL, or provider. It creates no synthetic
event, depends on the existing durable inbox, and exposes a dry run that does
not launch the browser. Registration and unconfirmed-invocation rejection have
deterministic Rust coverage.

Local source checks for this repair: `cargo fmt --check`, Clippy with warnings
denied, and the two focused T-0040 MCP tests passed. The full Rust suite ran
451 tests: 433 passed, 7 failed, and 11 were ignored. The remaining failures
are the pre-existing unavailable advisor-program and Windows process-tree
access-denied cases; the prior missing T-0040 tool-registration failure no
longer occurs. CatDesk remains responsible for independent verification.

## Remaining Acceptance Status

| Area | Status |
| --- | --- |
| Long-poll review-event experiment | NOT_STARTED |
| CatDesk acceptance DAG | BLOCKED_BY_THREAD_BINDING |
| Real project registry | BLOCKED_BY_THREAD_BINDING |
| Multi-project scheduler acceptance | BLOCKED_BY_THREAD_BINDING |
| Codex/Qwen live routing acceptance | NOT_STARTED; no allowance was intentionally consumed |

## Verification

- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- Focused `cargo test auto_start_computer -- --nocapture`: 1 passed.
- Focused `cargo test codex_app_server -- --nocapture`: 4 passed.
- `cargo test --no-fail-fast`: 403 passed, 0 failed, 9 ignored.
- Existing Python compile plus `tests/test_wake_bridge.py`: 5 passed.
- `git diff --check`: PASS. Line-ending warnings were emitted by Git but no
  whitespace error was reported.

## Git Evidence

- Branch: `orchestrator/chatgpt-codex-autonomous-loop`.
- Base HEAD before the existing uncommitted T-0030 through T-0039 work:
  `b958eb9fff4522168ebb1ae4a726209896a27451`.
- Current tracked working-diff Git blob: `cd725dfb5b374de0d6b486c16f866007fbd36c7a`.
- No commit, push, merge, PR, release, or deployment was performed.

## Recommended Next Step

Resolve the exact Codex-thread binding intentionally: either make the existing
conversation use the canonical `CatDesk-codex-loop` workspace and become
available for direct input, or explicitly authorize a different project/thread
mapping. Separately, approve creation of a project-local Python environment and
SeleniumBase installation after the dedicated ChatGPT browser profile and
conversation target are prepared. Only then resume the live wake, DAG, and
multi-project acceptance phases.
