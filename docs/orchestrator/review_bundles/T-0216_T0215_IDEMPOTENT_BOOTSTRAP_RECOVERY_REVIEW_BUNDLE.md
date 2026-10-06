# T-0216 — T-0215 Idempotent Reviewed Main-Image Bootstrap Recovery

Status: **IMPLEMENTED AND LOCALLY VERIFIED; LIVE OPERATOR RETRY PENDING**  
Implementation/review owner: **ChatGPT Web through bounded CatDesk Workspace tools**  
Date: 2026-08-22

## Live defect that triggered this ticket

The first authenticated T-0215 bootstrap reached the product trust-root path and then returned `REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_REFUSED`.

Read-only administrator diagnostics established all of the following simultaneously:

- final destination `C:\Program Files\CatDesk\CatDesk.exe` was absent;
- fixed incoming payload existed with length `25134592` and SHA-256 `2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459`;
- incoming signed envelope existed at epoch `1`;
- accepted receipt `C:\Program Files\CatDesk.reviewed-main-image-accepted.v1` existed at epoch `1`;
- incoming envelope and accepted receipt were byte-identical, both SHA-256 `2684020a2bb4c4be81a207af713cdb4239a41e65a48628c07f3bbbaca42b3a81`;
- the live daemon was still running from a workspace candidate, not from the missing Program Files destination.

This proves signature/envelope/policy/payload authentication succeeded and the signed generation was durably consumed before the final image was created.

## Root cause

Two T-0215/T-0214 assumptions combined into a live recovery defect:

1. `persist_accepted_reviewed_main_image_envelope` consumed the fresh signed epoch before final image installation, which is intentionally conservative for anti-rollback.
2. `SystemWindowsReviewedMainImageBootstrapOperations::install_exact_destination_from_opened_source` required `C:\Program Files\CatDesk` to already exist and refused if it did not. The bootstrap path did not create that fixed parent.
3. A retry of the same already-authenticated epoch was then rejected by the original `incoming.epoch <= accepted.epoch` rule even though the final destination was still absent.

Deleting/resetting the accepted receipt or weakening monotonic state would make the recovery problem a security bypass and was rejected.

## Corrective security model

T-0216 introduces an explicit epoch disposition:

- **Fresh**: there is no accepted receipt or `incoming.epoch > accepted.epoch`.
- **ExactAcceptedRecovery**: `incoming.epoch == accepted.epoch` and the canonical signed incoming envelope is byte-for-byte identical to the independently verified accepted receipt.
- older epoch: refused;
- same epoch with any different signed/canonical content: refused.

Before either fresh acceptance or exact recovery, the fixed final destination must be absent. This remains a first-image bootstrap, not an update channel.

For a fresh generation, the accepted signed envelope is still consumed before returning bootstrap authority. For `ExactAcceptedRecovery`, the receipt is not rewritten, deleted, reset, or rolled back. The existing signed receipt remains the anti-rollback authority.

The incoming payload is still opened through the fixed no-follow transport path, measured from the retained handle, and checked against the signed payload SHA-256 and byte length before any install authority is returned.

## Fixed Program Files parent recovery

The administrator-only fixed-policy installer now safely ensures the one compiled parent `C:\Program Files\CatDesk`:

- policy destination must exactly equal the compiled `REVIEWED_MAIN_IMAGE_DESTINATION`;
- if the parent is absent, every already-existing ancestor through `C:\Program Files` must first pass the existing non-reparse directory-chain validation;
- only the exact fixed `CatDesk` child is created with `fs::create_dir`; `create_dir_all` is not used;
- an `AlreadyExists` race is accepted only for subsequent revalidation, never as proof;
- the resulting parent must be a directory, not a symlink/reparse point, must pass the Windows reparse attribute check, must pass the full fixed parent-chain validator, and must canonicalize case-insensitively to the exact compiled parent;
- the final `CatDesk.exe` still uses `create_new` and any existing destination is refused.

No caller path, destination, hash, key, signature, service/account, or environment input was added.

## Anti-rollback and replay invariants retained

T-0216 does **not** permit arbitrary same-epoch replay. Exact recovery requires the same verified canonical signed envelope already persisted as the accepted receipt, and the fixed final destination must still be absent.

Therefore:

- older signed epochs remain refused;
- same-epoch conflicting signed envelopes remain refused;
- newer signed epochs remain fresh generations;
- a successfully present final destination prevents bootstrap replay;
- accepted state is never deleted/reset by recovery;
- no new signature or private-key access is needed for the interrupted epoch-1 install.

## Changed source surface

Only the reviewed-main-image bootstrap logic in `src/reviewed_build.rs` was intentionally changed for T-0216, plus this review bundle.

Key new/changed symbols:

- `ReviewedMainImageEpochDisposition`
- `classify_reviewed_main_image_epoch`
- `reviewed_main_image_fixed_destination_is_absent`
- exact-recovery handling in `acquire_product_signed_reviewed_main_image`
- `SystemWindowsReviewedMainImageBootstrapOperations::ensure_fixed_destination_parent_is_safe`
- strengthened existing-destination refusal before create-new install
- focused `t0216_exact_accepted_recovery_is_narrow_and_non_authoritative` regression test
- updated T-0215 epoch-state test for exact accepted recovery vs same-epoch conflict

## Verification

Performed from the canonical CatDesk workspace after the direct corrective implementation:

- CatDesk `verify_project` → **PASSED**
  - `cargo fmt --check` → **PASSED**
  - full `cargo test` → **PASSED**
  - `cargo build` → **PASSED**
- `cargo test t0216 -- --nocapture` → **PASSED (2/2 focused T-0216 tests)**
- `cargo test t0215 -- --nocapture` → **PASSED (5/5 T-0215 unit tests + stable policy-digest integration vector)**
- `cargo clippy --all-targets --all-features -- -D warnings` → **PASSED**
- `git diff --check` → **PASSED**; only pre-existing LF/CRLF warnings were emitted
- `cargo build --release --locked` → **PASSED**

The repository has a long-lived dirty/untracked history, so a global Git diff is not treated as task authority. The T-0216 changes were made by exact Workspace replacements and independently rechecked through the task-specific source symbols and focused tests above.

## Rejected worker evidence

A bounded local Qwen 3.8 delegated run reported `COMPLETED_VERIFIED` for T-0216 but did not implement the required source correction and captured unrelated historical repository content as its diff. That completion claim was explicitly rejected. The accepted T-0216 implementation is the subsequent direct bounded Workspace implementation and the verification listed in this bundle.

## No-live-mutation statement

The T-0216 implementation/test pass did not:

- delete, reset, rewrite, or weaken `C:\Program Files\CatDesk.reviewed-main-image-accepted.v1`;
- alter the already-staged ProgramData payload/envelope;
- create `C:\Program Files\CatDesk` during tests;
- install the final Program Files image;
- request/read/copy the production Ed25519 private key;
- change MCP/autonomous authority surfaces;
- modify the external Secure MCP tunnel;
- publish, merge, or push Git state.

## Independent pre-live Windows canonical-path correction

Before asking the operator to rerun the elevated bootstrap, ChatGPT independently re-read the T-0216 source and found one Windows live-compatibility defect in the new fixed-parent revalidation: `fs::canonicalize(parent)` was compared textually against the non-verbatim compiled `C:\\Program Files\\CatDesk` path. Elsewhere CatDesk already normalizes Windows verbatim canonical paths because Rust may return the `\\\\?\\C:\\...` form. Without the same normalization here, a legitimate fixed Program Files parent could fail closed even though all security checks passed.

The T-0216 implementation now routes both the canonical result and the fixed expected parent through the existing `crate::command::normalize_windows_verbatim_path` helper before the case-insensitive exact-path comparison. No path authority was broadened: the destination remains the compiled constant, metadata/reparse checks and the fixed ancestor-chain proof run first, and this normalization only reconciles equivalent Windows path syntax.

A new ordinary Windows regression test `t0216_windows_canonical_parent_identity_normalizes_verbatim_paths` proves both an explicit `\\\\?\\C:\\Program Files\\CatDesk` normalization case and the production canonical-identity helper against a real temporary directory. After this correction, `cargo test t0216 -- --nocapture`, full `verify_project`, strict Clippy, `git diff --check`, and `cargo build --release --locked` all pass.

## Remaining live acceptance

The existing epoch-1 incoming envelope and accepted receipt are already identical and cryptographically accepted, and the final destination is absent. After building this corrected release utility, the next operator action is to rerun the same zero-parameter administrator bootstrap command using the newly built T-0216 `target\release\catdesk.exe`.

Expected behavior:

1. the existing accepted epoch-1 envelope is classified as `ExactAcceptedRecovery` rather than rollback;
2. the fixed `C:\Program Files\CatDesk` parent is safely created/revalidated if absent;
3. the exact already-signed incoming payload is copied handle-to-handle to create-new `C:\Program Files\CatDesk\CatDesk.exe`;
4. destination evidence must match the signed payload evidence;
5. the accepted epoch-1 receipt remains unchanged;
6. the command returns `REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_INSTALLED_PENDING_T0212_ACCEPTANCE`.

Only after that live result and readback verification should T-0215/T-0216 close and the project advance to T-0212 dedicated-producer real-host acceptance.
