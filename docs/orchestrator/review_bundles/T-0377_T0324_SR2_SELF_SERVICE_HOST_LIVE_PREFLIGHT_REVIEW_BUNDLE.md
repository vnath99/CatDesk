# T-0377 — T-0324 SR2 self-service host-live preflight

## Authority chain

`preflight_current_user_worker_host()` in `src/user_worker_release.rs` is the sole new read-only surface and has no caller parameters. It resolves the current-user profile with `SHGetKnownFolderPath`, descends only fixed `AppData/Local/CatDesk/WorkerReleases` components through `ProtectedDirectoryGuard`, and reads only canonical `active-state.json`, selected immutable version, manifest, and worker image through no-follow protected operations.

The reader validates active-state schema/current-prior generation relation, immutable generation+manifest directory, manifest digest, fixed ordinary-worker role, image SHA-256/length, reviewed-source identity, review session/record/authority digest, and attestation identity/digest. Missing, malformed, stale/equal-generation, tampered, oversized, or reparse/symlink state refuses before any supervisor/runtime surface is contacted.

After exact local readback, the preflight calls only existing `fixed_current_worker_registration()` with the selected manifest digest. It does not connect to the pipe or send a registration. Existing `bind_request_to_os_attested_peer` remains the later server-side operation that replaces worker claims from an OS-attested connected peer before supervisor mutation.

## Input/output contract

Input is exactly empty: no release root, executable path, image hash, worker role, supervisor endpoint, trust material, or expected identity can be caller supplied.

`UserWorkerHostPreflightV1` permits only:

- `Refused { reason: USER_RELEASE_MISSING }` for no committed fixed-root state.
- `Refused { reason: USER_RELEASE_INVALID }` for protected state, manifest/image, review, attestation, generation, or reparse failure.
- `Refused { reason: SUPERVISOR_WORKER_IMAGE_MISMATCH }` when the running worker fails fixed local registration identity.
- `SupervisorPeerAttestationRequired { generation, manifest_sha256 }` when immutable local evidence and the fixed registration shape agree, but later OS peer attestation is still required.

The final outcome is intentionally not READY and cannot authorize activation.

## Boundaries and regressions

The preflight does not activate/register/swap a worker, open a pipe client, write release state, create a root, modify Program Files/ProgramData, touch wake/target/browser state, or start/stop/adopt/reconfigure the externally owned Secure MCP runtime. Original T-0374 cancellation history and accepted T-0374-R1/T-0375/T-0376 lineage remain unchanged.

`host_preflight_is_read_only_and_requires_later_peer_attestation` proves validated prepared evidence reaches only attestation-required, image mismatch refuses, preparation remains inert, fixed production descent is existing-only, and no pipe-client/registration dispatch appears in the module. Existing release tests cover tampered/missing/reparse state, stale generation/CAS, review/attestation fields, readiness refusal, atomic rollback/interruption handling, and external runtime non-ownership.

## Verification and attribution

- `cargo fmt --all -- --check` — PASS.
- `cargo test user_worker_release -- --nocapture` — PASS, 12/12.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --all-targets --all-features` — PASS (910 unit tests plus integration targets).
- `git diff --check` — PASS; known CRLF warnings are pre-existing conversion notices.

Attributable files are `src/user_worker_release.rs` and this bundle. Fresh independent review is required before a separate live activation ticket uses fixed lifecycle/pipe authority for OS-attested readiness and rollback. T-0377 does not claim serving parity or event-driven wake acceptance.
