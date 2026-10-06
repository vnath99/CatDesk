# T-0060A Production Preflight Review Bundle

## Delivered

- Added `scripts/catdesk-production-acceptance.ps1`, an internal read-only
  preflight/compare harness.
- Added deterministic `scripts/test-catdesk-production-acceptance.ps1` fixtures.
- Documented the maintainer-only acceptance sequence in the README and canonical
  architecture.

## Fixed gates and redaction

The preflight JSON has `schemaVersion`, `stage`, `overallState`, a fixed gate
array, a canonical build fingerprint, fixed lifecycle state, and an opaque
instance fingerprint. It does not emit usernames, absolute paths, task XML,
process command lines, URLs, routes, tunnel IDs, environment values, wake
configuration contents, browser-profile contents, or raw errors.

| Gate | Meaning |
| --- | --- |
| `CANONICAL_RELEASE` | `target/release/catdesk.exe` exists and matches the adjacent SHA-256 manifest. |
| `PUBLIC_LIFECYCLE` | Root `catdesk.ps1` exists and parses. |
| `LIFECYCLE_STATUS` | Read-only public status is READY, or reports distinct external-runtime pending attention. |
| `AUTOSTART_OWNERSHIP` | Public autostart status reports enabled; disabled is attention, conflict/unavailable fail. |
| `PERSISTENT_SUPERVISOR` | Expected no-timeout/battery-friendly task settings and persistent monitor defaults exist. |
| `WAKE_RUNTIME_PRESENCE` | The minimal wake config fields, configured in-workspace profile presence, and wake Python runtime are measured without emitting their values. |
| `WAKE_TARGET_BINDING` | The supported exact ChatGPT conversation URL is SHA-256-compared to the operator-supplied opaque fingerprint. Missing, malformed, or mismatched fingerprints fail closed; neither URL nor hash is emitted. |
| `RETENTION_EVIDENCE` | T-0057 execution/post-audit evidence exists and remains within the compact threshold. |
| `EXTERNAL_RUNTIME_OWNERSHIP` | Measured config must be `official_runtime` and the existing alias/runtime must pass the established read-only status/health check. |
| `CANONICAL_LISTENER_INSTANCE` | The configured loopback MCP listener must positively match the canonical executable/path/hash and be the sole local CatDesk process before an opaque instance fingerprint is emitted. |

T-0060A-R2 makes comparison fail closed. Both inputs must be complete,
schema-version-1 `PRE_REBOOT_PREFLIGHT` snapshots with the exact fixed gate
set, supported fixed state labels, a valid canonical build fingerprint, and a
bounded non-UNKNOWN instance fingerprint. A final PASS requires unchanged
build, a new instance, `READY`, post overall PASS, and PASS for every post
preflight gate. A missing, duplicated, malformed, unsupported, or extra
snapshot field/gate is comparison FAIL; the three selected identity fields can
never make a handcrafted minimal post snapshot pass. The only ATTENTION result
is the measured `LOCAL_READY_EXTERNAL_RUNTIME_PENDING` state with its
`LIFECYCLE_STATUS` gate ATTENTION and every other gate PASS.

## Fixture coverage

T-0060A-R1 replaces assumed wake/external-runtime/process-name gates with
measured fail-closed evidence. The fixture suite covers correct/wrong/missing
wake-target fingerprints, missing runtime/config, official runtime verified vs.
non-official/unverified runtime, canonical vs. noncanonical/ambiguous listener,
retention evidence, redaction fixtures, and full post-reboot comparison cases:
healthy PASS; the narrow external-runtime pending ATTENTION; failed wake target
or external-runtime ownership; autostart attention; missing/duplicate gates;
wrong stage/schema; invalid identity; unchanged instance; and a minimal
handcrafted snapshot. It uses seams and never reaches Task Scheduler, browser
profile contents, tunnel runtime, or a live daemon.

Required checks:

- `powershell -ExecutionPolicy Bypass -File scripts/test-catdesk-production-acceptance.ps1`
- `powershell -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1`
- `powershell -ExecutionPolicy Bypass -File scripts/test-catdesk-autostart-supervisor.ps1`
- `powershell -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- `git diff --check`

No live Scheduled Task, daemon, browser, external runtime, credential, reboot,
or Git publication action was performed for T-0060A through T-0060A-R2. A
direct live preflight invocation through MCP `run_command` remains blocked by
the existing restricted shell allowlist; R2 did not alter that policy.
