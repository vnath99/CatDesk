# T-0375 — T-0324 registry-schema count regression repair

## Cause and boundary

`autonomy_project_registry_bind` intentionally has thirteen closed-world `oneOf`
forms. The stale regression
`exposed_registry_bind_supports_only_exclusive_target_cas_mode` still expected
twelve. The thirteenth form is the already-reviewed T-0324 cached-connector
bridge: it requires exactly `projectId`, `decision`, and `expectedSha256`, and
its decision pattern is `^DESIGNATED_CHAT_TARGET_URL=\\S+$`. The production
handler binds this CatDesk-only request to the reviewed paired project-target
and effective-wake-target CAS/readback transaction; it is not a new schema or
authority introduced by this ticket.

## Exact change

Only the assertion in `src/delegated/autonomy_supervisor.rs` changed. It now
requires exactly thirteen alternatives and finds the paired designated-target
alternative by its exact pattern, then verifies its exact three required keys.
This preserves detection of both unintended schema cardinality changes and
loss or widening of the paired-CAS form. No production schema, dispatcher, or
handler behavior changed.

## Verification

- `cargo fmt --all -- --check` — pass.
- `cargo test exposed_registry_bind_supports_only_exclusive_target_cas_mode -- --nocapture` — pass.
- `cargo test cached_designated_chat_target_cas_keeps_catdesk_registry_and_wake_target_coherent -- --nocapture` — pass.
- `cargo test cached_connector_target_cas_requires_exact_fixed_decision_shape -- --nocapture` — pass.
- `cargo clippy --all-targets --all-features -- -D warnings` — pass.
- `cargo test --all-targets --all-features` — pass (900 unit tests plus integration targets).
- `git diff --check` — pass; only pre-existing CRLF conversion warnings were emitted.

## Attribution and review request

Attributable files are this bundle and the regression assertion in
`src/delegated/autonomy_supervisor.rs`. The dirty worktree was preserved. No
live target, wake, daemon, tunnel, Scheduler/service, signing, UAC, Git, or
external-project state changed. T-0375 is not self-accepted; a fresh
independent final review is required before T-0374 is reverified.
