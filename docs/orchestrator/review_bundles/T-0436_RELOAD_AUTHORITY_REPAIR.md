# T-0436 — Reviewed daemon-reload authority repair

## Scope

This bounded direct ChatGPT task repairs only the source contract for `catdesk_daemon_reload`. It does not execute a daemon reload, prepare/confirm a protected reviewed build, promote a release, mutate LKG/recovery state, mutate Wake, resume T-0425, change Secure MCP ownership, or publish Git history.

## Trigger

The source-current T-0436 toolchain resolver is verified against the real host, but the serving daemon predates that resolver. The existing reload MCP surface had two defects:

1. its public reviewed `PREFLIGHT/CONFIRM` intent was not implemented by the handler, which still consumed the legacy `dryRun` form; and
2. a generic acknowledged independent review did not by itself bind reload authority to one exact candidate, allowing an authority-substitution design gap if a valid review were reused with a different `buildPath` / SHA-256.

## Repair

- Added pure `src/daemon_reload_approval.rs` with purpose `catdesk-daemon-reload-approval-v1`.
- The only typed review artifact is `src/daemon-reload-approval-request-v1.json`.
- The canonical request binds exact workspace-relative candidate path, SHA-256, and byte length.
- The approval module has no MCP, process launch, protected-state writer, runtime, or activation authority.
- `catdesk_daemon_reload` now has a closed two-shape schema only:
  - `PREFLIGHT + buildPath + expectedSha256 + recordId`
  - `CONFIRM + buildPath + expectedSha256 + confirmationToken`
- Legacy `dryRun`, `decision`, `RESULT`, and unrelated fields are not part of the public reload schema.
- Active autonomous mutation/verification blocking remains the first handler gate.
- PREFLIGHT:
  1. canonically measures the candidate,
  2. resolves/re-measures the acknowledged independent review,
  3. requires exactly one current immutable typed reload-approval artifact binding that candidate,
  4. remeasures the candidate after review resolution,
  5. derives a purpose-separated reload authority digest,
  6. atomically persists the review binding together with the existing short-lived reload preflight token/path/hash.
- CONFIRM accepts no caller-supplied review record. It recovers the review identity only from the persisted preflight, remeasures the same acknowledged review/artifact against the current candidate request, requires the resulting binding to equal the persisted binding, then calls the existing native reload helper.
- The former unreviewed public `prepare_reload` / `execute_reload` wrappers were removed; no source callers remain.
- Existing native reload/rollback helper behavior and external-tunnel non-ownership are otherwise unchanged.

## Security properties / regressions

PASS:
- `daemon_reload_approval`: 3 focused typed-authority tests.
- exact typed artifact binds one candidate.
- candidate path/hash/length substitution fails closed.
- hostile relative/absolute/path traversal, invalid SHA and noncanonical artifact forms fail closed.
- reviewed preflight persists its exact review binding.
- changed review authority cannot consume the preflight.
- a reviewed preflight cannot be downgraded into an unreviewed execution.
- public schema is exactly reviewed PREFLIGHT/CONFIRM.
- legacy/unrelated handler shapes fail closed when idle.
- active mutation still blocks reload before request execution.
- acknowledged review for candidate A cannot authorize candidate B through the real CatDesk review-authority resolver.
- removing the exact typed artifact invalidates authority.

## Verification

- `cargo test daemon_reload_approval -- --nocapture`: PASS (3/3).
- `cargo test reviewed_reload_preflight_binding_is_persisted_and_cannot_be_downgraded -- --nocapture`: PASS.
- `cargo test daemon_reload_schema_is_closed_to_reviewed_preflight_and_confirm -- --nocapture`: PASS.
- `cargo test daemon_reload_handler_rejects_legacy_and_unrelated_shapes_when_idle -- --nocapture`: PASS.
- `cargo test daemon_reload_review_authority_requires_exact_current_candidate_artifact -- --nocapture`: PASS.
- existing `daemon_reload_is_exposed_but_blocked_during_active_mutation`: PASS.
- `cargo clippy --bin catdesk -- -D warnings`: PASS.
- `cargo test --bin catdesk`: PASS.

## Required post-review sequence

This implementation review is not candidate reload authority. After this source task reaches independent final review and is accepted:

1. build one exact source-current candidate;
2. create a separate bounded candidate-approval review whose completion outputs include the canonical `src/daemon-reload-approval-request-v1.json` binding that exact candidate path/SHA-256/length;
3. ACK and naturally Wake-accept that candidate review;
4. use that candidate review record for reviewed reload PREFLIGHT, then exact CONFIRM;
5. prove serving/current parity;
6. issue exactly one fresh protected V5 PREPARE/CONFIRM and require `BUILD_ATTESTED`; stop on any fixed-vocabulary failure;
7. reviewed promotion -> durable `reviewed_promotion` LKG -> supported one-command recovery;
8. only then resume the SAME T-0425 and live-prove T-0429.

## Runtime mutation statement

No daemon reload, protected reviewed-build retry, promotion, LKG/recovery mutation, Wake mutation, T-0425 resume, Secure MCP mutation, or Git publication was performed by this repair task.
