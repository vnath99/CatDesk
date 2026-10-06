# T-0459R1 — Source-current bootstrap build

## Purpose

Session: `adc-t0459r1-source-current-bootstrap-build-20261005`

This session performs no source repair. It produces one fresh workspace-contained isolated release candidate from the current accepted CatDesk source so the serving daemon can be bootstrapped to the already-reviewed T-0457R1 and T-0458 control-plane fixes.

Accepted source authority:
- T-0457R1 review-inbox automatic retention: COMPLETED_VERIFIED / PASSED, independently ACKed.
- T-0458 unclaimed PREPARED reviewed-build supersession: COMPLETED_VERIFIED / PASSED, independently ACKed.
- Natural T-0458 Wake acceptance is complete on dev.84, canonical generation 28.

## Why the bootstrap is required

The currently serving daemon predates T-0458. Its reviewed-build PREPARE path therefore cannot supersede the abandoned active PREPARED generation even though current source can. Repeating PREPARE under the old daemon would preserve the deadlock.

## Fixed build boundary

Use only:
- `CARGO_BUILD_RELEASE_ISOLATED`
- `GIT_DIFF`

The isolated verifier owns the release candidate under the workspace-contained verification target. This executable is bootstrap/verification material only; it is not reviewed promotion, LKG, canonical release, or Git publication authority.

Any serving handoff must use the existing typed `catdesk_daemon_reload` preflight/confirm path with a separately reviewed `src/daemon-reload-approval-request-v1.json` binding the exact candidate relative path, SHA-256, and byte length.

## Non-actions

This session does not:
- change CatDesk source;
- stage/commit/push Git;
- bind GitHub publication authority;
- PREPARE/CONFIRM a protected reviewed build;
- create reviewed promotion/LKG authority;
- alter Wake target/profile/events;
- mutate the external Secure MCP runtime.

## Next

After contract-approved verifier PASS, measure the isolated candidate through the typed daemon-reload approval workflow, independently review that exact request, run daemon-reload PREFLIGHT/CONFIRM, reconnect, prove source-current serving parity, then use serving T-0458 to supersede the abandoned PREPARED generation and resume the protected build -> GitHub publication path.
