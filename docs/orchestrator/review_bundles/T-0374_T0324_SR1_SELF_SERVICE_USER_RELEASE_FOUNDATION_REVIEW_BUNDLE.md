# T-0374 — T-0324 SR1 self-service user release foundation

## Scope and authority

`src/user_worker_release.rs` adds a repository-only immutable ordinary-worker release-store foundation. It reuses reviewed source/build/review/attestation identifiers as mandatory manifest bindings and is deliberately not a second signer, promotion authority, supervisor front door, or external-route owner. The existing product-root main-image authority remains unchanged until a separately reviewed host-live cutover consumes this foundation.

## State and refusal model

`UserWorkerReleaseManifestV1` binds schema, monotonic generation, fixed role `catdesk-ordinary-worker-v1`, image SHA-256/length, source snapshot identity, exact review session/record/authority digest, and attestation identity/digest. Prepared content is stored below a generation-plus-canonical-manifest-digest version directory and remains inert until exact expected-generation activation.

The source now provides explicit `prepare`, `readback`, `activate`, `rollback`, and `reconcile` primitives. `current.json` and `previous.json` are separate receipts. Activation revalidates canonical manifest and image bytes before advancing state by expected-generation CAS; same/lower-generation activation is refused. Rollback requires the exact expected current generation, revalidates the exact prior immutable release, restores it, and consumes the matching previous receipt. Reconcile revalidates accepted current/previous state after interruption while treating stale `.stage-*` directories as inert residue.

Malformed identity fields, wrong role/schema, empty/oversized or mismatched image, changed same-generation bytes, stale CAS, manifest/image tampering, malformed/orphan pointer state, unsafe symlink/reparse objects, missing preparation, and failed readiness refuse. Exact prepare replay is idempotent.

## Supervisor and runtime boundary

The module is dormant and does not launch or reconfigure a backend. `read_current_user_worker_release()` resolves only the Windows known-folder current-user profile, descends fixed `AppData/Local/CatDesk/WorkerReleases` components through `ProtectedDirectoryGuard`, and reopens the current pointer, immutable version, manifest, and image with no-follow handle-relative reads. It accepts no production root, filename, executable path, or hash. `fixed_current_user_worker_registration()` then represents only that validated release through the existing fixed `WorkerBackendRegistrationV1`; `fixed_current_worker_registration()` still requires the executing worker image to equal the manifest digest, and the control pipe still replaces worker claims with OS-attested peer evidence. A later host cutover alone may invoke readiness/activation, retaining existing rollback semantics. This release foundation contains no external Secure MCP ownership/configuration surface and does not create, stop, authenticate, or reconfigure that runtime.

## Changed files attributable to T-0374

- `src/user_worker_release.rs`
- `src/main.rs` module registration line (`mod user_worker_release;`; the file already contains unrelated accumulated CatDesk changes)
- `docs/orchestrator/review_bundles/T-0374_T0324_SR1_SELF_SERVICE_USER_RELEASE_FOUNDATION_REVIEW_BUNDLE.md`

No live release, ProgramData/Program Files mutation, UAC, signing, browser/wake, project-target, tunnel, Scheduler/service, Git publication, or external-project action occurred.

## Verification

After the provider repair budget exhausted, ChatGPT independently inspected the implementation against the approved T-0374 acceptance criteria and found the provider version incomplete despite its two passing tests: it lacked required readback/rollback/reconcile primitives and broad interruption/refusal coverage. ChatGPT completed only that bounded T-0374 source work and reran verification.

- `cargo test user_worker_release -- --nocapture` — PASS: 7 passed, 0 failed. Coverage includes inert preparation/readback, monotonic expected-generation CAS, readiness refusal, idempotent exact replay, same-generation conflict, tamper refusal preserving current, stale interrupted stage recovery, exact rollback/stale rollback CAS, malformed/orphan durable state refusal, Windows reparse refusal, and source-level exclusion of legacy/external-route authority.
- `cargo fmt --check` — PASS.
- `cargo test` — PASS (full Rust suite; pre-existing ignored environment-specific advisor test remains ignored as before).
- `cargo build` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- Git/diff inspection — bounded T-0374 files inspected; `src/main.rs` is known to contain many pre-existing unrelated accumulated changes, so independent review must attribute only the `user_worker_release` registration to this ticket.

## Residual boundary / next ticket

T-0374 is a source-only foundation, not live serving parity. A separately reviewed T-0324 follow-on must derive the user root from trusted OS identity, use the protected handle-bound filesystem primitives for the production adapter, bind the selected immutable release into the fixed supervisor registration/OS peer-attestation seam, and prove readiness plus exact rollback while preserving the externally owned Secure MCP runtime. Only after that serving generation is live may the guarded paired project+wake target CAS/readback move to the current chat and natural event-driven wake acceptance proceed.

## Independent review request

## Residual-adapter verification reconciliation

The residual production-facing, read-only adapter adds no lifecycle command or
MCP surface. Its root is obtained by `SHGetKnownFolderPath`, never by PATH,
environment, workspace, or caller input; all durable production reads begin
from its retained `ProtectedDirectoryGuard` chain. The current release is
eligible for supervisor representation only when its immutable bytes match the
manifest and the existing fixed registration accepts the running image. This
does not replace the pipe's server-side OS peer-image attestation.

- `cargo test user_worker_release -- --nocapture` — PASS, 8/8.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --all-targets --all-features` — PASS, 906 unit tests plus
  integration targets.
- `git diff --check` — PASS; known CRLF warnings are pre-existing worktree
  conversion notices.

The remaining boundary is live lifecycle invocation/readiness/rollback against
an independently reviewed candidate. It is not attempted here; current-chat
rebinding and event-driven wake remain unaccepted.

Independent review must reject this ticket or any later integration if it treats this store as an attestation issuer, permits caller-selected production paths/hashes/roles, bypasses server-side OS-attested peer verification, lets ordinary health self-bless a release, weakens generation CAS/rollback, or takes ownership of the external Secure MCP runtime. T-0374 is not self-accepted; controller/final-review closure remains required.
