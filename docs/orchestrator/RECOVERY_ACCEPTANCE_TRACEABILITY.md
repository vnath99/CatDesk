# CatDesk recovery acceptance traceability

Updated: 2026-09-07

This is a working verification matrix for the recovery-first milestone. It is
not a review approval and it does not replace the independent T-0319 review
record. It maps the operator contract to the current implementation and the
strongest available evidence, so later work does not regress to manual hash,
PID, LKG, or tunnel troubleshooting.

| Requirement | Current implementation boundary | Current evidence | State |
| --- | --- | --- | --- |
| One public command | `catdesk.ps1 recover` delegates to the bounded canonical stack bootstrap | Fresh project-root invocation on 2026-09-07 returned `CONNECTED_VERIFIED` in 3.4 seconds | Verified on host |
| Canonical binary/sidecar repair without blessing arbitrary bytes | `catdesk-release-recovery.ps1`; only an already-authoritative durable LKG pair is eligible. Operational health may re-use a matching LKG but cannot mint or advance rollback authority. | `test-start-catdesk-stack.ps1` covers missing, malformed, mismatched, split-brain sidecars, binary repair, and refusal to create LKG authority from operational health alone | Fixture pending T-0322 re-verification |
| Missing/corrupt LKG pointer | `Get-CatDeskLastKnownGoodRelease` selects only a unique highest valid generation; `Save-CatDeskLastKnownGoodRelease` repairs its pointer after validation | Canonical bootstrap fixture covers corrupt/missing pointer reconciliation and damaged/ambiguous refusal | Fixture verified |
| Missing LKG authority | Read-only assessment returns fixed `LKG_AUTHORITY_MISSING`; no invented promotion authority is used | Canonical bootstrap fixture covers the missing-authority refusal | Fixture verified |
| Durable authority after a healthy recovery | Bootstrap writes a bounded `operational_verified` LKG record only after local daemon and official runtime are verified | Canonical bootstrap fixture creates and restores an operationally verified LKG snapshot | Fixture verified |
| Healthy daemon idempotence | Exact listener/path/hash identity is adopted before any relaunch path | Canonical bootstrap fixture and fresh repeated public recovery evidence | Fixture and host verified |
| Stale daemon without listener | Recovery may stop only one exact canonical `--catdesk-daemon`; foreign/multiple rows fail closed | Disposable real-process fixture plus two watchdog-only daemon-loss cycles | Fixture and host verified |
| Existing external runtime reuse | Runtime is observed and bounded but remains externally owned | Public recovery returned `CONNECTED_VERIFIED`; no tunnel recreation performed | Host verified |
| Runtime observation diagnostics | Bounded fixed gates classify timeout, overflow, malformed status, readiness, `healthz`, and `readyz` errors | Canonical bootstrap fixture covers dynamic health URL/file, timeout, oversized, malformed, `healthz`, `readyz`, and transient status shapes | Fixture verified |
| Durable watchdog recovery | Autostart supervisor makes bounded public recovery calls and avoids restart bursts during transport observation failure | Two public `stop` -> watchdog-only `READY` cycles and focused supervisor fixture | Host and fixture verified |
| Listener identity in production preflight | Exact canonical daemon candidate plus matching listener PID is required; unrelated same-name processes do not cause ambiguity | Direct production-path fixture covers exact, PID-mismatched, and multiple-candidate shapes; live no-argument preflight also passed | Fixture and host verified |
| Browser wake readiness | Wake runtime/target are deliberately separate, browser-owned acceptance surfaces | Current preflight fails those gates closed because they are unconfigured; recovery did not mutate them | Explicitly outside recovery ownership |
| Independent recovery acceptance review | T-0319 evidence bundle contains the required reviewer checklist | `docs/orchestrator/review_bundles/T-0319_WATCHDOG_AND_STALE_DAEMON_RECOVERY_ACCEPTANCE_REVIEW_BUNDLE.md` | Pending independent review |

Current regression commands:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-stale-canonical-daemon-recovery.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-autostart-supervisor.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-promote-reviewed-catdesk-build.ps1
cargo test --test recovery_powershell -- --nocapture
./catdesk.ps1 recover
```

The final public command is the operator-facing acceptance command. The other
commands are maintainer regression checks and must not become an operator
troubleshooting sequence.
