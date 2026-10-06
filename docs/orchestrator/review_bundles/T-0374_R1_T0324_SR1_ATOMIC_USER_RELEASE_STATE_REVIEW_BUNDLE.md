# T-0374-R1 / T-0324-SR1 Atomic User Release State Review Bundle

Date: 2026-09-10
Status: IMPLEMENTED_AND_VERIFIED_PENDING_INDEPENDENT_REVIEW

## Scope and lineage

This is the repair continuation of semantic ticket T-0374 / T-0324-SR1. The original autonomous session `adc-t0374-t0324-sr1-self-service-user-release-foundation-20260910` is intentionally `CANCELLED` after repair-budget exhaustion and was not resumed. T-0375 (`adc-t0375-t0324-registry-schema-count-regression-repair-20260910`) is independently accepted and closed.

The repair is source/test/documentation only. No live serving cutover, signing/UAC, Program Files/ProgramData mutation, wake/target/tunnel mutation, Scheduler/service work, Git publication, or external-project mutation was performed.

## Defect

The T-0374 user-release foundation represented accepted release authority with separate `current.json` and `previous.json` commits. Activation wrote previous then current; rollback wrote current then deleted previous. A process interruption between those operations could persist `previous == current` (or another half-transition shape), after which `reconcile()` correctly rejected the invalid relation and the release state could remain wedged.

## Repair

`src/user_worker_release.rs` now defines one strict canonical `ActiveStateV1` record stored as `active-state.json`:

- `schema`
- exact `current` pointer (`generation`, `manifest_sha256`)
- optional exact `previous` pointer

The parser denies unknown fields, requires schema 1, validates both pointer identities, and rejects `previous.generation >= current.generation`.

Activation now:

1. refuses when readiness is false;
2. opens and validates the complete committed active state when present, including exact prepared manifest/image bindings for current and prior;
3. enforces the existing expected-generation CAS and monotonic next generation;
4. validates the next immutable prepared release;
5. constructs one complete next state with new current and old current as optional previous; and
6. commits only that state.

Rollback now:

1. opens and validates the complete committed state;
2. enforces expected-current generation CAS;
3. requires the one prior release and revalidates it;
4. constructs one complete state with prior as current and `previous: null`; and
5. commits only that state.

The commit path serializes a bounded canonical state to an inert, uniquely named `.active-state.<nonce>.tmp`, syncs the staging file, rechecks any existing destination as a plain bounded file, and atomically replaces the destination. On Windows the replacement uses `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`. A failed/interrupted staging object is never read as authority. Legacy split pointer files are no longer consulted.

Prepared release directories remain immutable/inert until activation. Exact manifest digest, generation, worker image SHA-256 and length validation are unchanged. Existing symlink/reparse refusal remains in place for release root/version/image objects, and committed active state must itself be a plain bounded file.

## Regression coverage

The release test family now covers:

- prepared state remains inert until readiness-approved activation;
- monotonic generation and stale generation CAS refusal;
- idempotent exact prepare and same-generation conflict refusal;
- prepared-image tamper refusal without losing committed current;
- exact one-prior rollback and stale rollback CAS refusal;
- malformed/equal-generation current/previous active-state refusal;
- interrupted `.active-state.*.tmp` residue is inert while the prior committed state remains readable;
- activation commits current+previous together and rollback commits current+no-previous together;
- tampering a syntactically valid committed current manifest binding blocks activation, rollback, and reconcile;
- Windows reparse substitution remains refused;
- manifest review/attestation identity bindings remain strict; and
- no `target/release`, Program Files, external tunnel module, or environment-selected profile authority is introduced.

The test-root helper also adds an atomic nonce to the timestamp suffix so parallel release regressions cannot alias a temporary root.

## Verification

- `cargo fmt -- --check` — PASS.
- `cargo test user_worker_release` — PASS: 11 passed, 0 failed for the release family; all other filtered harnesses clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --workspace --all-targets --all-features` — exit 0. Primary suite: 888 passed, 0 failed, 21 ignored; remaining integration suites pass. Output contains a known AppContainer fixed-helper permission-denied child probe, but the encompassing tests and command remain successful and it is outside this ticket's source.
- `cargo build --release --locked --target-dir .catdesk/verification-targets/t0374-r1` — PASS. The first MCP invocation exceeded its 120-second response window while compiling; the immediate same-target retry waited on the build lock and completed successfully in 28.85 seconds. This isolated build is verification-only and is not deployable/release authority.

Relevant command logs include:

- `.catdesk/logs/1789067361-b6945e0a-ffaf-42da-919f-da7851ab8464.log` — fmt pass.
- `.catdesk/logs/1789067424-60f2aad7-d1bc-4b8a-8431-ff755022a9e4.log` — focused test pass.
- `.catdesk/logs/1789067443-b2784d29-f1a9-4a76-8938-3e8455a87d53.log` — strict clippy pass.
- `.catdesk/logs/1789067517-a88a5a6c-41c9-4abb-9210-daacdd787dea.log` — full test pass.
- `.catdesk/logs/1789067861-ba901d01-e463-4dde-bb29-57a0d131f5b9.log` — isolated release pass.

## Execution/control-plane notes

The CatDesk MCP transport health probe timed out once, but subsequent CatDesk control/workspace calls succeeded. Session inventory proved there was no active T-0374-R1 worker, the original T-0374 was `CANCELLED`, and T-0375 was `COMPLETED_VERIFIED`.

Codex could not be selected from this MCP context because provider status returned `OPERATOR_CONFIGURATION_REQUIRED`. The approved local Qwen path was then attempted, but new delegated creation was blocked by a pre-existing T-0322 delegated journal stuck at `CANCEL_REQUESTED`; an idempotent cancel request did not finalize it. No worker was therefore launched. Per the current provider policy, ChatGPT completed the bounded repair directly using only CatDesk workspace tools rather than bypassing the supervisor or mutating host/runtime authority.

## Files in this R1 boundary

- `src/user_worker_release.rs`
- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `docs/orchestrator/review_bundles/T-0374_R1_T0324_SR1_ATOMIC_USER_RELEASE_STATE_REVIEW_BUNDLE.md`

## Review questions

Independent review should specifically verify:

1. `active-state.json` is the sole current/prior release authority and no legacy split-pointer read path remains.
2. Both activation and rollback transition current/prior state with one atomic replacement and stale temp residue cannot become authority.
3. Existing committed state is cryptographically/structurally revalidated before either transition.
4. Readiness and generation CAS remain fail closed.
5. Rollback exposes at most one prior accepted release and clears that prior atomically.
6. Prepared releases remain immutable/inert and exact manifest/image validation is unchanged.
7. No caller-selected root, product-root signing, live runtime/tunnel ownership, protected host mutation, or Git publication authority was introduced.

## Remaining boundary

T-0374-R1 is not independently accepted merely because implementation verification is green. Independent final review is required next. Only after independent acceptance may a separately bounded host-live serving-generation cutover/readiness/rollback canary be considered; this bundle authorizes no such live action.
