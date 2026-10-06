# T-0307 / T-0223 Existing Main-Image Bootstrap Reconciliation Review Bundle

## Scope and sole classification

**Classification: `EXISTING_MAIN_IMAGE_ARTIFACT_STALE_OR_MISSING`.**

This is a read-only reconciliation of the historical T-0215/T-0217 product
trust-root route, separate from T-0306's ordinary reviewed-build/promotion
result (`TRUSTED_CANDIDATE_PREREQUISITE_MISSING`). It neither creates a signed
image, envelope, provenance record, promotion authority, nor performs a
bootstrap, installation, rotation, reload, or activation.

`EXISTING_MAIN_IMAGE_BOOTSTRAP_PATH_AVAILABLE` is not established because no
currently demonstrable independently reviewed, signed T-0299-or-newer image
and its canonical envelope are available. `MAIN_IMAGE_BOOTSTRAP_INTEGRATION_DEFECT`
is not established because the current source retains the reviewed fail-closed
consumption mechanism and there is no valid current artifact whose consumption
is blocked by a source defect.

## Immutable trust model

| Trust edge | Current source authority | Required invariant |
| --- | --- | --- |
| Signer root | `src/reviewed_build.rs` (`REVIEWED_MAIN_IMAGE_PRODUCTION_PUBLIC_KEY`, root id `catdesk-main-image-root`) | The production embedded Ed25519 keyring/root is compiled in; no caller, MCP tool, or runtime argument supplies a key. |
| Envelope | `ReviewedMainImageReviewEnvelopeV1::parse_canonical` and `verify_strict` in `src/reviewed_build.rs` | Exact canonical signed fields bind product, purpose/policy, root/version, epoch, payload SHA-256/length, review id, and build id. Malformed, alternate, unsigned, or wrong-purpose data fails closed. |
| Fixed transport and accepted state | `REVIEWED_MAIN_IMAGE_INCOMING_*` and `REVIEWED_MAIN_IMAGE_ACCEPTED_ENVELOPE_PATH` in `src/reviewed_build.rs` | Incoming payload/envelope are at compiled ProgramData paths; accepted receipt is at a compiled Program Files path. Neither is caller-selected authority. |
| Epoch/replay control | `classify_reviewed_main_image_epoch` and `read_accepted_reviewed_main_image_envelope` in `src/reviewed_build.rs` | Only a newer epoch is fresh; exact accepted-envelope recovery is narrow; lower or conflicting same-epoch input is refused. |
| First executable and rotation | `execute_reviewed_main_image_bootstrap_install`, `execute_reviewed_main_image_rotation_as_administrator`, and fixed `src/main.rs` flags | Bootstrap/rotation use fixed roots, remeasure bound bytes, fixed transaction/receipt semantics, and rollback/attention outcomes. They do not accept arbitrary executable/path/hash authority. |

This establishes a mechanism capability only. It does not make a source build,
test digest, `target/release` binary, or the T-0304-R3 isolated verification
output an independently reviewed main image.

## Artifact and chronology audit

| Record/artifact | What it proves | T-0299-or-newer image authority? |
| --- | --- | --- |
| `docs/orchestrator/review_bundles/T-0215_FIRST_IMAGE_ENVELOPE.v1` | Historical signed epoch-1 envelope for `review-t0215-trust-root-activation-20260822`, with payload SHA-256 `2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459`. | No. It predates T-0299 and the bound payload is not a current T-0299+ executable. |
| `docs/orchestrator/review_bundles/T-0215_T0154_R5D_R3J_PRODUCT_TRUST_ROOT_MAIN_IMAGE_AUTHORITY_PROVIDER_REVIEW_BUNDLE.md` | T-0215 mechanism and externally owned private-signing boundary. | No current signed payload/envelope. |
| `docs/orchestrator/review_bundles/T-0216_T0215_IDEMPOTENT_BOOTSTRAP_RECOVERY_REVIEW_BUNDLE.md` | Historical epoch-1 accepted/incoming recovery semantics and historical host observation. | No current protected-state readback or T-0299 chronology. |
| `docs/orchestrator/review_bundles/T-0217_ROTATION_SIGNING_PAYLOAD.txt` and `T-0217_SIGNED_REVIEWED_MAIN_IMAGE_ROTATION_REVIEW_BUNDLE.md` | Exact historical, unsigned epoch-2 signing request and mechanism readiness. | No. The payload is not a completed signed envelope/executable, and it predates T-0299. |
| `.catdesk/reviewed-build`, `.catdesk/promotion-control`, `target/reviewed-builds`, and `.catdesk/reviewed-source-snapshots` readback | No current ordinary reviewed candidate/authorization chain; snapshot directory is empty. | No. This reconfirms T-0306 but is not substituted for main-image evidence. |

The bounded workspace search found no later reviewed-main-image/trust-root bundle
and no signed main-image executable/envelope whose chronology can be proved
T-0299-or-newer. A hash, review documentation, or source test without the
payload plus signed canonical envelope chain is not image authority.

## Protected-host observability limit

The fixed host paths include the compiled ProgramData bootstrap and rotation
inboxes, Program Files accepted and rotation receipts, and the fixed installed
`C:\\Program Files\\CatDesk\\CatDesk.exe`. They were not opened, read, or
modified. The current old daemon has no dedicated safe read-only MCP/main-image
status surface (`src/mcp.rs` has no reviewed-main-image tool), while
`verified_current_reviewed_main_image_digest` is an internal fixed-role reader
in `src/reviewed_build.rs`, not an exposed old-daemon authority.

Therefore current protected receipt/image state is precisely
**`UNOBSERVABLE_FROM_CURRENT_SAFE_SURFACE`**, not "absent". This limitation
does not weaken the artifact requirement: a path becomes available only when a
valid current signed artifact/envelope chain is demonstrable under the accepted
trust root and its consumption route is safely reviewable.

## Decision and next bounded action

The missing proof is an independently reviewed, signed T-0299-or-newer
main-image executable **and** its canonical product-root envelope, with a
chronology and fixed-root consumption binding that the T-0215/T-0217 mechanism
can validate. Repository-only work cannot reduce that absence without creating
the forbidden signing/provenance/release artifacts.

No operator bootstrap command is emitted in this state. The only future
prerequisite is production of that artifact through the existing externally
authorized reviewed release/signing authority; afterwards, a separate
read-only reconciliation must prove its exact artifact and envelope before any
fixed bootstrap action can be considered. T-0223 activation remains prohibited
and `OPERATOR_BOOTSTRAP_REQUIRED`; the order remains **T-0223 ->
T-0222/T-0139 -> T-0152 -> T-0155**.

## Source decision, attribution, and prohibited-action audit

No product source change was made. There is no proven integration defect and no
new bootstrap authority, unsigned route, caller-selected path/hash/command, or
new evidence store.

T-0307 attribution is documentation only:

- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `docs/orchestrator/review_bundles/T-0307_T0223_EXISTING_MAIN_IMAGE_BOOTSTRAP_RECONCILIATION_REVIEW_BUNDLE.md`

The repository is an accumulated dirty worktree; all other status entries are
pre-existing or outside this ticket and are not attributed to T-0307.

This ticket did not create a key, signature, envelope, candidate, reviewed
image, promotion authorization, or provenance record; and did not read or
mutate Program Files/ProgramData, bootstrap/install/rotate/promote/reload,
activate a supervisor, invoke a browser wake, change a target/profile, alter
Secure MCP/tunnel, operate on external projects, publish Git, or restart the
live daemon.

## Verification and independent-review checklist

Documentation-only verification:

- `cargo fmt --all -- --check` - PASS (the existing environment emitted only
  `could not canonicalize path C:\\Users\\Volap`).
- `cargo test --workspace --all-targets --all-features t0215` - PASS: 5
  focused signer-root/envelope/payload-binding tests and the T-0215
  policy-vector integration test passed; 882 unrelated unit tests were
  filtered.
- `cargo test --workspace --all-targets --all-features reviewed_main_image` -
  PASS: 3 focused bootstrap/trust-root tests passed; 884 unrelated unit tests
  were filtered.
- `cargo test --workspace --all-targets --all-features t0217` - PASS: 3
  focused rotation tests and the T-0217 policy-vector integration test passed;
  unrelated tests were filtered.
- `git diff --check` - PASS; only pre-existing working-copy line-ending
  warnings were emitted.

Independent review should verify the sole classification, the historical
epoch-1/unsigned-epoch-2 distinction, the explicit
`UNOBSERVABLE_FROM_CURRENT_SAFE_SURFACE` boundary, the fact that T-0304-R3 is
verification-only, preservation of immutable signer/envelope/epoch semantics,
and that no unrelated dirty-worktree changes are attributed to T-0307.
