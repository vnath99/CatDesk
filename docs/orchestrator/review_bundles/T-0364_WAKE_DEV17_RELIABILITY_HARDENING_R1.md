# T-0364 Wake dev.17 Reliability Hardening — R1 Review Bundle

Date: 2026-09-19
Canonical conversation: https://chatgpt.com/c/6aaecf61-e314-83ea-8b78-e2d35c7b3311
Canonical Wake target generation: 10
Source Wake version: 1.0.0-dev.17
Installed Wake version before deployment: 1.0.0-dev.15

## Scope

Bounded reliability hardening of the already-implemented dev.17 Wake path. This review does not change CatDesk daemon authority, Secure MCP ownership, canonical target authority, or natural-acceptance criteria.

Files directly changed in this bounded follow-up:

- scripts/wake_bridge.py
- tests/test_wake_bridge.py
- wake/src/store.rs
- wake/tests/protocol_store.rs

## Finding 1 — outer browser retry crossed the stated first-write boundary

The dev.17 outer SeleniumBase session loop was correctly restricted by error taxonomy to BROWSER_NETWORK_ERROR, but it did not structurally remember whether the browser had already been written. If a future/current post-write operation surfaced BROWSER_NETWORK_ERROR after cdp.press_keys(), the outer catch could create another browser session despite the design contract saying retries are pre-write only.

Repair:

- A per-session browser_write_started guard is initialized false.
- It flips true immediately before the first cdp.press_keys() instruction, conservatively treating an exception from that instruction itself as potentially user-visible mutation.
- The outer retry catch now refuses all retry once browser_write_started is true.
- Login/profile, CAPTCHA/security, target drift, draft/selector ambiguity and every non-network Attention remain single-attempt/fail-closed.

Deterministic regressions prove:

1. First session BROWSER_NETWORK_ERROR -> second session ready: exactly one fresh retry and typing occurs only in the second session.
2. Three pre-write network failures: exactly three sessions then BROWSER_NETWORK_ERROR.
3. Non-network Attention: exactly one session.
4. BROWSER_NETWORK_ERROR after the first browser write begins: exactly one session; no relaunch.

## Finding 2 — exact-ID stale retirement was not truly idempotent after archival

Store::retire_stale documented evidence-preserving exact-ID retirement and refusal of SUBMITTING/SENT, but after either retirement or SENT archival the queue file is absent. A repeat call therefore failed generically at queue read before it could distinguish already-safely-retired evidence from an already-SENT record.

Repair:

- If the queue event is absent but an archive exists, the archived event and delivery journal are read and validated.
- An exact archived event plus exact STALE / OPERATOR_RETIRED journal with no receipt is accepted as an idempotent no-op.
- Any other archived state, including SENT, is explicitly STALE_RETIREMENT_REFUSED.
- Missing queue + missing archive remains unavailable/fail-closed.
- Normal first retirement still journals STALE/OPERATOR_RETIRED and archive-commits before queue removal.

Direct regressions prove:

- successful retirement preserves exact archive + durable journal and an exact repeated retirement changes no bytes;
- SUBMITTING and SENT both refuse retirement without changing delivery evidence;
- concurrent submit-vs-retire has exactly one safe winner, ending only in SUBMITTING-with-queue or STALE-with-archive;
- queue event ID mismatch refuses without archive/journal creation.

## Verification

PASS:
- cargo fmt --check
- git diff --check -- scripts/wake_bridge.py tests/test_wake_bridge.py wake/src/store.rs wake/tests/protocol_store.rs
- cargo test -q -p catdesk-wake: protocol_store 22/22, process_tree 3/3
- cargo clippy -q -p catdesk-wake --lib --bins -- -D warnings
- pytest tests/test_wake_bridge.py -q -k "outer_network_retry or outer_retry_never": 4/4
- pytest tests/test_wake_bridge.py -q -k "not login_captcha_or_unrecoverable_network_never_presents_browser": 69/69

The single excluded Python test intentionally waits through real three-window network timeout behavior and exceeds the MCP execution budget. Its login/CAPTCHA/target-drift readiness behavior is separately covered by deterministic readiness tests, and the outer network retry matrix above directly covers the deployment-critical behavior without wall-clock sleeps.

## Deployment boundary

The immutable Wake installer declares the same source version, 1.0.0-dev.17, and uses the fixed package/handoff path. Deployment may proceed through wake/install.ps1 only. The external Secure MCP runtime and CatDesk daemon must remain untouched.

After deployment, require readback that:
- installed Wake is dev.17;
- host returns RUNNING when prior desired state was RUNNING;
- canonical target remains generation 10 with digest 86752787e1be8a3f1ff0dd9d76a3224061c35952ff3b22a073ff7cda47513732;
- no manual diagnostic or stale event is counted as natural acceptance.

Final natural Wake acceptance still requires a fresh legitimate CatDesk review event to create a real persisted USER message in the canonical conversation plus a correlated durable EXACT_USER_MESSAGE_APPENDED receipt.
