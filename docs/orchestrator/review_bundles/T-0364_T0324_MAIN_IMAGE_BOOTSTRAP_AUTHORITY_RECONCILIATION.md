# T-0364 — T-0324 main-image bootstrap authority reconciliation

## Classification

`TRUSTED_T0299_BOOTSTRAP_AUTHORITY_NOT_DEMONSTRABLE_FROM_CURRENT_SAFE_SURFACE`.

T-0319 is independently accepted through T-0363; that recovery verdict creates no reviewed-release, bootstrap, signer, producer-attestation, or promotion authority.

## Read-only artifact audit

| Evidence | Result | Bootstrap authority? |
| --- | --- | --- |
| T-0215 first-image envelope | Complete signed epoch-1 envelope bound to SHA-256 `2421a90a...eee28459`. | No: historical pre-T-0299 payload. |
| T-0217 rotation payload | Epoch-2 request bytes bound to SHA-256 `552cb917...c21482`, with no signature/envelope. | No: unsigned and historical pre-T-0299. |
| Workspace source, `target/release`, isolated verification output | Mutable or verification-only bytes. | No: not a signed main image, immutable snapshot, attested candidate, or promotion input. |
| Generic independent review / T-0319 verdict | Recovery and source-review evidence. | No: neither signer nor payload/envelope binding, producer attestation, or promotion authorization. |
| `.catdesk/promotion-control/reviewed-build-attestation.json` | Workspace-visible canonical current location absent, as T-0324 records. | No current producer-issued attestation. |
| Current reviewed-build/candidate/snapshot chain | No workspace-visible T-0299+ chain. | No ordinary reviewed-build/promotion route. |

The fixed ProgramData incoming inboxes, Program Files accepted/rotation receipts, and installed main image are **UNOBSERVABLE_FROM_CURRENT_SAFE_SURFACE**. The old daemon has no safe read-only main-image status operation; this is not evidence that protected-host artifacts are absent. Conversely, no workspace-visible protected record demonstrates a current T-0299-or-newer signed payload plus canonical envelope.

## Conclusion and next legitimate mechanism

T-0306 remains `TRUSTED_CANDIDATE_PREREQUISITE_MISSING`; the old serving generation cannot produce the review → immutable snapshot → fixed build attempt → producer attestation → promotion chain. T-0307 remains `EXISTING_MAIN_IMAGE_ARTIFACT_STALE_OR_MISSING`: epoch-1 is historical and epoch-2 is unsigned. No source defect is established.

The minimum legitimate cross-generation mechanism is an independently reviewed **T-0299-or-newer** main-image payload and its exact canonical envelope under the existing immutable product root, purpose, and epoch policy. CatDesk may safely prepare or review fixed identity/measurement material only in a separately authorized workflow. It cannot generate a key or signature, select arbitrary bytes, create a producer attestation, authorize promotion, or stage protected host paths. The externally owned product-root signer must sign the exact approved binding, and an administrator must separately authorize fixed-path staging and zero-parameter bootstrap/rotation consumption.

No bootstrap, rotation, promotion, reload, wake, tunnel, protected-state, signing, provenance, dedicated-producer, external-project, or Git action occurred.

## T-0324 boundary

Keep T-0324 blocked from *deployment* until an exact current signed image exists, but do not treat candidate designation itself as an operator decision. On 2026-09-09 the operator explicitly delegated CatDesk main-image/version designation to ChatGPT. ChatGPT independently determines when rotation is required, audits the executable-affecting source/version state, designates the exact immutable candidate and canonical signing payload, and carries the workflow through every technically accessible reviewed local surface. Codex/Qwen may provide implementation/review evidence but do not select release bytes.

For T-0324, ChatGPT has independently determined that a new signed generation is required: the serving canonical image is pre-T-0299 while accepted runtime/control-plane behavior required for the remaining acceptance sequence is newer. Preserve the existing compiled Ed25519 product root and private-key isolation. Do not substitute raw reload, direct promotion-script execution, manual copy, mutable `target/release`, isolated verification output, caller-selected hash/path, generic review, or protected-lock deletion. The exact immutable candidate/hash/length/epoch/signing payload remains to be finalized by T-0360 after source-lineage audit and a locked release build from that frozen candidate. After the resulting signature is verified and fixed-path consumption succeeds, reconnect without taking Secure MCP ownership, prove serving parity, reconcile T-0322, then run the bounded Qwen continuation canary.
