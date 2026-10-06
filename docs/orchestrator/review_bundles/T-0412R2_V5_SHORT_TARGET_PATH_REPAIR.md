# T-0412R2 — V5 Short Target Path Repair

## Scope

Session: `adc-t0412r2-v5-short-target-path-repair-20260926`  
Contract: `fnv1a64:a7402a743788a6ca`  
Logical repair: T-0412 V5 Windows reviewed-build linker blocker.

This implementation is limited to the reviewed-build worker path layout in `src/reviewed_build.rs`, deterministic regression coverage in that module, and this review bundle. It does not promote, reload, install, change Wake targets, modify the external Secure MCP runtime, publish Git, or clean/reset unrelated dirty worktree state.

## Failure evidence and root cause

The accepted T-0411-bound V5 reviewed-build retries `465286253071480b8fb9afa95c296d26` and `ea299f644c6b4eb89e2331a5b6098dce` both terminated as `BUILD_FAILED_OR_AMBIGUOUS` / `REVIEWED_BUILD_FAILED` with `CARGO_BUILD`, exit 101, classification `CARGO_LINK_FAILED`, and no attestation.

The retained Cargo diagnostic proves MSVC `link.exe` was found and invoked. It failed with `LNK1104` while creating build-script executables under a path of the form:

`.catdesk/reviewed-build-control/generations/<attempt>/builds/<same-attempt>/target/release/build/<crate>/<build-script>.exe`

The failing output path exceeded the practical classic Windows linker path ceiling. The important defect is not missing linker discovery or missing Windows SDK libraries: the active retry generation is already attempt-scoped, but the worker redundantly repeats the same 32-character attempt id under `builds/<attempt>` before the Cargo target.

## Security/authority analysis

`resolve_active_control_root` already descends into `generations/<activeAttemptId>` and revalidates that generation's `attempt.json` against the active pointer, including exact attempt id and attempt digest. Therefore a retry generation's `BuildControlRoot.guard` is already bound to the authoritative attempt.

The nested `builds/<same-attempt>` directory under that guard adds path length but no additional authority.

The repair preserves:

- reviewed-source snapshot validation;
- protected-directory/no-follow pinning;
- exact active-generation attempt/digest validation;
- isolated offline Cargo-home seeding;
- exact pinned Cargo and rustc evidence;
- worker process/job containment;
- post-Cargo pinned target traversal and open-handle acquisition of `release/catdesk.exe`;
- candidate publication at `target/reviewed-builds/<attempt>/catdesk.exe`;
- candidate evidence, attestation, and promotion boundaries;
- legacy root behavior for installations without an active retry-generation pointer.

## Implementation

### Retry-generation source layout

`materialize_snapshot` now returns the exact materialized source `PathBuf`.

For the legacy unversioned control root it preserves:

`builds/<attempt>/source`

For an already attempt-scoped active retry generation it uses:

`source`

### Retry-generation target layout

`build_target_guard` now preserves the legacy layout when `control.guard.path() == control.base_guard.path()`.

For active retry generations it creates/opens:

`target`

directly beneath `generations/<attempt>`, rather than:

`builds/<attempt>/target`

This removes approximately 40 characters from every Cargo/MSVC output path on the observed installation while keeping the target beneath the same pinned, attempt-bound protected generation.

### Regression coverage

The existing materialization test now asserts the legacy control root still materializes at `builds/<attempt>/source`.

A Windows regression `active_generation_target_layout_does_not_repeat_attempt_id` constructs an attempt-scoped generation guard and verifies the target is exactly `<generation>/target` and does not reintroduce the `builds` nesting.

## Attribution note

The first narrow source edits were made during live root-cause diagnosis immediately before this T-0412R2 contract was created. They are intentionally disclosed as pre-session candidate edits rather than being misrepresented as contract-timestamp-attributable implementation work. T-0425 was subsequently paused at stateVersion 5, and this T-0412R2 session was created/validated/approved and directly claimed before further test/review mutation.

## Verification

Current status at bundle creation:

- Source/security review: PASS for the intended narrow path-layout design.
- CatDesk transport after the earlier recovery incident: `CONNECTED_VERIFIED`, local MCP `READY`, 89-tool self-check, same external official tunnel runtime.
- Generic `run_command` rejected `cargo fmt --check` before Cargo execution; this is command-policy rejection, not a formatting failure.
- Dedicated `verify_project` was invoked but the client call timed out before an authoritative result returned.
- Search of retained logs for the new regression name found no match, so no current-patch Cargo-test pass is claimed yet.

Required before acceptance:

1. approved formatting verification;
2. focused reviewed-build regression execution;
3. full Cargo test profile;
4. strict Clippy;
5. git diff check;
6. authoritative session diff and independent final review.

## Acceptance and next boundary

T-0412R2 is not accepted merely because the source change is plausible. It must finish `COMPLETED_VERIFIED` with independent review.

After acceptance, a **fresh** reviewed-source authority and a **new** protected V5 PREPARE/CONFIRM must be used. Terminal attempts `465286...` and `ea299...` must never be reused. The new attempt must reach `BUILD_ATTESTED` before reviewed promotion. Only reviewed promotion may establish canonical/serving parity and durable `reviewed-promotion.json` LKG authority.

Only after one-command recovery is proven against that reviewed authority should the paused T-0425 Wake session resume.
