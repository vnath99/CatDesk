# T-0217 — Signed Reviewed Main-Image Rotation

Status: **IMPLEMENTED AND VERIFIED; OFFLINE EPOCH-2 SIGNATURE / LIVE ROTATION PENDING**  
Date: 2026-08-22

## Triggering live evidence

The T-0216 corrected first-image bootstrap returned the expected live success state:

`REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_INSTALLED_PENDING_T0212_ACCEPTANCE`

The resulting signed epoch-1 image is installed at the fixed `C:\Program Files\CatDesk\CatDesk.exe` path. Subsequent T-0212 read-only compatibility evidence exposed a Windows canonical-path defect in the dedicated-producer source/service checks: the legitimate installed path canonicalizes to the Windows verbatim form (`\\?\C:\...`) while those checks compare against the compiled non-verbatim path. Rather than weaken canonical identity or silently reinterpret the signed epoch-1 image, T-0217 introduces a separately signed, strictly newer reviewed-main-image rotation path.

## Security model

T-0217 preserves the first-image bootstrap authority and accepted epoch-1 receipt. Rotation is a distinct product-root purpose and policy:

- purpose: `reviewed-main-image-rotation`
- epoch must be strictly greater than the currently accepted generation;
- the existing installed image must first verify against accepted signed state;
- incoming payload/envelope use fixed compiled transport locations only;
- incoming payload bytes are measured from an already-opened no-follow handle and must exactly match the signed SHA-256 and length;
- replacement is staged at a compiled fixed sibling, revalidated, and crash-safely replaces only `C:\Program Files\CatDesk\CatDesk.exe`;
- monotonic receipt/recovery state prevents rollback, conflicting same-epoch replay, or arbitrary replacement;
- the administrator command has zero arbitrary inputs;
- no private signing material is generated, read, stored, or requested by CatDesk/MCP/Qwen;
- rotation is not exposed through MCP or autonomous command surfaces;
- no service/SCM, Secure MCP tunnel, browser/Scheduler, external-project, or Git-publication authority is added.

## Windows compatibility correction during host review

The partially completed T-0217 worker tree had two rotation helpers compiled out under `#[cfg(all(windows, not(test)))]` while Windows tests referenced them. The helpers are now `#[cfg(windows)]` so the exact production helper logic is exercised by Windows tests. This changes test compilation coverage only and does not expose new runtime authority.

## Verification

T-0217 is green through both the delegated final review and independent host verification. The default target directory later had an environmental linker lock on an existing test executable, so the independent rerun used isolated target directories without stopping the production CatDesk daemon or external Secure MCP runtime.

Passed:

- delegated `verify_project` — PASSED
- isolated `cargo test --target-dir target/t0217-check t0217 -- --nocapture` — PASSED (3 focused unit tests plus T-0217 measurement integration coverage)
- `cargo fmt --check` — PASSED
- isolated strict `cargo clippy --target-dir target/t0217-check --all-targets --all-features -- -D warnings` — PASSED
- isolated full `cargo test --target-dir target/t0217-check` — PASSED
- `cargo build --release --locked --target-dir target/t0217-release-check` — PASSED
- `git diff --check` — PASSED (pre-existing line-ending warnings only)

The temporary release-measurement integration file was neutralized after measurement because CatDesk's protected workspace policy correctly blocks ad-hoc destructive deletion through MCP; it no longer reads or depends on the measured artifact.

## Exact epoch-2 release artifact

Locked release image:

- path: `target/t0217-release-check/release/catdesk.exe`
- length: `25172480`
- SHA-256: `552cb917998d283e7d901593ffed5a9ceaa108223654aa69349fa7e567c21482`

Rotation policy SHA-256:

`c477d5bc486002e2034324b3a40b07d8e87c149cf5f2dab9b12aafee92816705`

## Exact offline signing payload

The exact 422-byte payload (including its trailing LF) is persisted at:

`docs/orchestrator/review_bundles/T-0217_ROTATION_SIGNING_PAYLOAD.txt`

Its content is:

```text
CATDESK_REVIEWED_MAIN_IMAGE_ENVELOPE_V1
product=CatDesk
purpose=reviewed-main-image-rotation
root_id=catdesk-main-image-root
root_version=1
epoch=2
policy_sha256=c477d5bc486002e2034324b3a40b07d8e87c149cf5f2dab9b12aafee92816705
payload_sha256=552cb917998d283e7d901593ffed5a9ceaa108223654aa69349fa7e567c21482
payload_length=25172480
review_id=review-t0217-main-image-rotation-20260822
build_id=build-t0217-rotation-552cb917
```

The corresponding Ed25519 private key remains entirely outside CatDesk/ChatGPT/MCP. The next trust-boundary action is for the operator to sign exactly those 422 bytes with the same external product signing key and return only the 64-byte detached Ed25519 signature encoded as standard Base64. CatDesk/ChatGPT may then construct and independently verify the epoch-2 envelope before any administrator rotation is attempted.

## Remaining live sequence

1. Operator signs exactly `T-0217_ROTATION_SIGNING_PAYLOAD.txt` offline and returns only the detached signature.
2. Independently verify the signature against the compiled production public root and exact payload bytes.
3. Persist/verify the epoch-2 signed rotation envelope and fixed incoming rotation payload.
4. Cross the administrator-only zero-parameter rotation boundary using `--catdesk-reviewed-main-image-rotate-fixed-policy`.
5. Read-only verify the fixed Program Files image, signed epoch/receipt, SHA/length and recovery state.
6. Resume T-0212 dedicated-producer provisioning and full service/SID/namespace/security/IPC/retained-handle acceptance.

No live rotation, service provisioning, Secure MCP mutation, browser/Scheduler mutation, external-project mutation, Git publication, or private-key access occurred during this implementation/review cycle.
