# T-0215 / T-0154 R5D-R3J — Product Trust Root + Reviewed Main-Image Authority Provider

Status: **READY FOR INDEPENDENT REVIEW**  
Implementation owner for this corrective pass: **ChatGPT Web through bounded CatDesk Workspace tools**  
Date: 2026-08-22

## Why this ticket exists

T-0214 accepted a zero-parameter administrator bootstrap/install surface for the first reviewed CatDesk main image, but deliberately left `SystemWindowsReviewedMainImageBootstrapOperations::acquire_independently_reviewed_image` fail-closed because a pathname, workspace build, operator-supplied hash, or review-bundle file is not independent executable authority.

T-0215 adds the missing product-owned authentication layer. A transport image can become `ReviewedMainImageBootstrapAuthorityV1` only after a compiled Ed25519 public trust root authenticates a canonical signed review envelope and the envelope is bound to the exact already-opened payload object.

Repeated Qwen autonomous-owner attempts were not accepted as completion evidence. This pass was implemented directly and then verified from the task-specific source symbols/tests rather than relying on the repository's historical dirty diff.

## Threat model / non-goals

The implementation assumes the offline product signing private key is controlled outside CatDesk. CatDesk, MCP, Codex, and the production executable must never generate, request, load, or store that private key.

The following are explicitly **not authority**:

- `target/release/catdesk.exe` or any other current workspace output;
- a caller-provided path, hash, token, destination, service name, or review identifier;
- the mere existence of a review bundle;
- the fixed ProgramData transport pathname;
- an unsigned/forged/malformed envelope;
- a correctly signed older epoch after a newer epoch has been accepted.

The fixed incoming directory is transport only. An attacker may replace its names/bytes; unless the exact opened payload bytes match a valid product signature and a fresh monotonic epoch, no bootstrap authority is constructed.

## Changed implementation surfaces

### `Cargo.toml` / `Cargo.lock`

Added maintained Ed25519 verification dependency:

- `ed25519-dalek = "2"`

The production code uses only `VerifyingKey` / `Signature`. `SigningKey` and `Signer` are imported only under `#[cfg(test)]` for deterministic hostile fixtures.

### `src/reviewed_build.rs`

T-0215 adds:

- fixed product/root/purpose constants;
- the compiled production Ed25519 public trust root supplied by the operator on 2026-08-22:
  `3bc6a2101687816dc8235efde562db93dbd0ad822d922286640ec5c985239545`; the corresponding private key remains outside CatDesk/MCP/ChatGPT;
- stable bounded failure classes including `REVIEWED_BUILD_MAIN_IMAGE_TRUST_ROOT_UNPROVISIONED`, envelope/signature/transport/payload/rollback failures;
- `ReviewedMainImageTrustRootV1`;
- `ReviewedMainImageEnvelopeV1` and a strict canonical parser;
- Ed25519 verification against the compiled product root;
- exact policy-hash and payload SHA-256/length binding;
- fixed Windows no-follow/open-reparse-point transport opening with no write/delete sharing and exact retained file handles;
- a fixed product-owned accepted-envelope receipt under Program Files for monotonic anti-rollback state;
- write-through atomic receipt replacement and signed receipt readback validation;
- integration of `SystemWindowsReviewedMainImageBootstrapOperations::acquire_independently_reviewed_image` with the signed resolver;
- focused T-0215 hostile tests.

The existing T-0214 zero-parameter CLI is unchanged. No T-0215 path/hash/key/signature argument is added to CLI, MCP, or autonomous schemas.

## Canonical signed envelope

Encoding is bounded ASCII/UTF-8 text with **LF only**, exact field order, one final LF, and no extra/unknown/duplicate fields. Maximum envelope size is 4096 bytes.

The exact canonical form is:

```text
CATDESK_REVIEWED_MAIN_IMAGE_ENVELOPE_V1
product=CatDesk
purpose=reviewed-main-image-bootstrap
root_id=catdesk-main-image-root
root_version=1
epoch=<positive decimal u64>
policy_sha256=<64 lowercase hex>
payload_sha256=<64 lowercase hex>
payload_length=<positive decimal u64>
review_id=<bounded canonical identifier>
build_id=<bounded canonical identifier>
signature=<unpadded base64 Ed25519 signature>
```

The Ed25519 signature covers every byte through the `build_id` line, including its trailing LF, and excludes only the final `signature=` line. Parsing reconstructs canonical bytes and requires byte-for-byte equality, which rejects alternate encodings, CRLF, field reordering, duplicates, unknown fields, noncanonical base64, uppercase hashes, and oversized input.

The signed content binds:

- schema/magic;
- exact product;
- exact bootstrap purpose;
- trust-root id/version;
- monotonic signed epoch;
- exact T-0214 bootstrap policy SHA-256;
- payload SHA-256 and byte length;
- signed immutable review/build identifiers.

## Exact opened-object authority

Production transport locations are compiled constants:

- payload: `C:\ProgramData\CatDesk\reviewed-main-image\incoming\CatDesk.exe`
- envelope: `C:\ProgramData\CatDesk\reviewed-main-image\incoming\review-envelope.v1`

They accept no caller substitution.

On Windows the final transport object is opened with `CreateFileW(... FILE_FLAG_OPEN_REPARSE_POINT ...)`, read-only sharing only, then `GetFileInformationByHandle` rejects a directory or reparse object and bounds its size. Parent components are checked for directory/reparse safety. The payload handle remains open. Existing `evidence_from_open_regular` derives SHA-256, byte length, and stable Windows identity from that same handle. Only those measured bytes are compared with the signed envelope. T-0214 subsequently remeasures the same source handle before copying and validates destination evidence, so a changed source cannot silently become installed authority.

## Anti-rollback / replay state

Accepted signed generations are recorded at the fixed product-owned path:

`C:\Program Files\CatDesk.reviewed-main-image-accepted.v1`

The receipt is the canonical **signed envelope itself**, so its integrity is checked with the same product public root rather than trusting an unsigned epoch file. The Program Files parent is checked for safe directory identity. Updates are written to a fixed create-new `.next` object, flushed, then replaced with `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)` and read back through the no-follow verifier.

Acceptance requires `incoming.epoch > accepted.epoch`. Therefore:

- an older signed generation is refused;
- replay of the accepted generation is refused;
- a different envelope with the same epoch is refused;
- deleting the installed `CatDesk.exe` does not erase the accepted epoch because the receipt is a Program Files sibling, not inside the CatDesk destination directory.

The generation is consumed before returning bootstrap authority. A later installation failure can require a newly signed epoch, but it cannot make an older generation acceptable again.

## Hostile-test matrix

Focused `cargo test t0215 -- --nocapture` passes 5 T-0215 tests covering:

1. valid canonical signed envelope and exact policy binding;
2. unknown/duplicate field shape, CRLF, uppercase hash, oversized envelope, forged signature, wrong signing key, wrong signed purpose, and wrong signed root id;
3. same-length payload mutation via measured SHA mismatch;
4. stale epoch, exact replay, same-epoch conflict, and newer-epoch acceptance;
5. final-file, parent-directory, and dangling reparse rejection when Windows symlink creation is supported;
6. exact compiled production public-root bytes and proof that production/private signing material is never loaded by CatDesk;
7. continued absence of the incoming authority path from MCP/autonomous contract source.

Existing T-0214 bootstrap tests also pass unchanged in their security semantics:

`cargo test reviewed_main_image_bootstrap -- --nocapture` → **3 passed, 0 failed**.

## Verification performed

All commands were run from the canonical CatDesk workspace without live host provisioning:

- `cargo fmt --check` → **PASSED**
- `cargo test t0215 -- --nocapture` → **PASSED (5/5 focused tests)**
- `cargo test reviewed_main_image_bootstrap -- --nocapture` → **PASSED (3/3 focused T-0214 compatibility tests)**
- full `cargo test` through CatDesk `verify_project` → **PASSED**
- `cargo build` through CatDesk `verify_project` → **PASSED**
- `cargo clippy --all-targets --all-features -- -D warnings` → **PASSED**
- `git diff --check` → **PASSED** (only pre-existing Windows line-ending notices)
- after public-root insertion, full CatDesk `verify_project` → **PASSED**
- after public-root insertion, `cargo clippy --all-targets --all-features -- -D warnings` → **PASSED**
- `cargo build --release --locked` → **PASSED**
- exact release measurement → SHA-256 `2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459`, length `25134592`
- exact T-0214 bootstrap-policy digest → `a89cccc440eabe56997dab3fa064f62c1d5b11a9704c50286e506d9dfdd51efb`

The source was then independently searched for the T-0215 product-root/envelope symbols and tests, confirming task-specific implementation exists rather than inheriting the historical dirty diff. The exact unsigned first-image signing bytes were persisted at `docs/orchestrator/review_bundles/T-0215_FIRST_IMAGE_SIGNING_PAYLOAD.txt`.

## No-live-mutation statement

This implementation/review pass did **not**:

- write to Program Files or ProgramData transport state;
- create/configure/delete Windows services or accounts;
- change SCM, service SID, ACL, mandatory label, or dedicated-producer namespace state;
- run T-0212 provisioning;
- reload/promote/recover the live CatDesk daemon;
- alter the Secure MCP tunnel;
- publish/merge/push Git changes;
- generate or read a production private signing key.

The production public root is now provisioned with the operator-supplied 32-byte Ed25519 public key. Unit-test builds still refuse the live ProgramData/Program Files bootstrap path explicitly, so tests cannot mutate live host state merely because the public root is present. The corresponding production private key remains outside CatDesk, the repository, MCP, and ChatGPT.

## Remaining genuine operator-only signing gate before T-0212

The product trust root has now been activated with the operator's public key only. A fresh locked release build containing that public root was measured as:

- payload SHA-256: `2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459`
- payload length: `25134592`
- T-0214 bootstrap policy SHA-256: `a89cccc440eabe56997dab3fa064f62c1d5b11a9704c50286e506d9dfdd51efb`

The exact bytes to sign offline are persisted at:

`docs/orchestrator/review_bundles/T-0215_FIRST_IMAGE_SIGNING_PAYLOAD.txt`

The remaining bootstrap sequence is:

1. Sign that exact file locally with the operator-controlled Ed25519 private key; CatDesk/ChatGPT receives only the resulting 64-byte signature encoded as unpadded Base64.
2. Append only `signature=<unpadded-base64>\n` to form the canonical envelope and independently verify it against the compiled public root and the exact release payload measurement above.
3. Stage only the verified release payload + completed envelope at the two fixed ProgramData transport paths through the explicit administrator/operator boundary.
4. Run the already accepted zero-parameter T-0214 administrator bootstrap install command. It should install the authenticated image and preserve the signed anti-rollback receipt.
5. Proceed to T-0212 real-host dedicated-producer provisioning/security/IPC/retained-handle acceptance.

No private-key material should ever be pasted into ChatGPT, placed in the CatDesk repository, supplied as a CatDesk command argument, or exposed through MCP.

## Independent-review decision requested

Review the cryptographic/parser/Windows transport and rollback logic, focused tests, dependency addition, exact compiled public-root activation, non-mutating unit-test boundary, locked release measurement, and offline-signing payload. T-0215 should close only after the resulting offline signature is independently verified against the compiled public root and the exact release payload is successfully authenticated through the T-0214 bootstrap. The next project action is the narrow offline signature step, then T-0212; do not create another Qwen retry or provenance-design branch unless a concrete defect is found.
