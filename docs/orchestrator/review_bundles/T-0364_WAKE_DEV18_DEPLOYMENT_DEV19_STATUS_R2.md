# T-0364 Wake dev.18 interruption-safe deployment / dev.19 idle-status follow-up

Date: 2026-09-19
Canonical Wake target: https://chatgpt.com/c/6aaecf61-e314-83ea-8b78-e2d35c7b3311
Canonical target generation: 10
Canonical target digest: 86752787e1be8a3f1ff0dd9d76a3224061c35952ff3b22a073ff7cda47513732

## dev.18 accepted deployment facts

- Wake dev.18 was installed through the revised interruption-safe installer and reviewed Rust activation path.
- Immutable current/reviewed directory readback: 1.0.0-dev.18-b4ffea9a627b-4cf273730cb0.
- reviewed-install-status reported compiledVersion/currentVersion/reviewedCandidateVersion all 1.0.0-dev.18 and alreadyCurrent=true.
- Live WakeHost returned RUNNING on PID 46728 after activation.
- Secure MCP remained CONNECTED_VERIFIED and local MCP READY; external tunnel ownership was unchanged.
- The canonical generation-10 Wake target and digest were unchanged by deployment.

## dev.18 browser transport proof

- Startup-only SeleniumBase diagnostic against the independent Wake browser profile returned OK.
- A bounded independent-profile auth probe against the dedicated Test chat returned AUTH=EDITOR_READY.
- A fresh manual installed-adapter diagnostic targeted only the Test chat and produced READY -> BEFORE_SUBMIT -> SENT.
- Its durable receipt used evidence EXACT_USER_MESSAGE_APPENDED for event adapter-dev18-receipt-diagnostic-20260919-1745.
- The diagnostic message explicitly said NOT natural acceptance. It is transport/debug evidence only and cannot satisfy canonical natural-wake acceptance.

## stale observability defect found after transport proof

Despite authenticated EDITOR_READY and successful dev.18 delivery, transport status continued to report browser=ATTENTION, login=ATTENTION and submission=CLAIMED while queueDepth=0 and all seven remaining events were stale generations. Runtime status recomputed queue/stale counts but carried historical transient attempt fields from status.json.

## dev.19 correction

Wake source is bumped to 1.0.0-dev.19. runtime::status now normalizes browser/login/submission to NOT_OBSERVED / NOT_OBSERVED / IDLE whenever there is no current-generation actionable queue item. Historical last-event/receipt/timestamp fields remain available.

A focused runtime regression seeds stale ATTENTION/ATTENTION/CLAIMED values with no current queue and proves status returns NOT_OBSERVED/NOT_OBSERVED/IDLE while preserving lastEventId.

Verification:
- cargo fmt --check: PASS.
- cargo test -q -p catdesk-wake: PASS; 3 runtime/unit + 3 process-tree + 22 protocol/store tests.
- cargo clippy -q -p catdesk-wake --lib --bins -- -D warnings: PASS.
- focused Windows installer/recovery regression: PASS.
- dev.18 browser retry/store hardening from R1 remains unchanged and previously passed focused Python bridge verification.

## dev.19 deployment state at this checkpoint

- WakeHost dev.18 remains RUNNING and is the safe installed fallback.
- dev.19 WakeHost release build completed.
- The CatDesk/Binagotchy release build is still in LTO/link under the dedicated wake/target/catdesk-gui target. Later tool probes created redundant Cargo waiters; the original active build chain is preserved and no live runtime process is being killed.
- Do not run the dev.19 installer until the active GUI build completes and the target lock drains.

## acceptance boundary

Final natural Wake acceptance is still open. It requires a fresh legitimate CatDesk review event for generation 10, a real persisted USER message in the canonical conversation, and a correlated durable EXACT_USER_MESSAGE_APPENDED receipt. Manual Test-chat diagnostics, direct adapter diagnostics, stale events, or bridge-run-once calls are not acceptance evidence.