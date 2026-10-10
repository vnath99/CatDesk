# T-0468 R2 — Independent Signed Main-Image Readback Review

## Decision

**SOURCE_REVIEW_PASS; OPERATIONAL_READBACK_BLOCKED.**

The exact source at `65e881bd05b8379bd595650b905b047d047e277f` implements a closed, future-only, zero-argument signed main-image readback path consistent with the intended authority boundaries. This review does not prove that a protected host has accepted any receipt, that a current main image exists at the fixed destination, or that the new command has run successfully in the serving environment. Those facts remain blocked on separately authorized operator-host readback.

## Identity and scope

At review start, `HEAD` resolved to `65e881bd05b8379bd595650b905b047d047e277f`; tracked staged and unstaged diffs were empty. The prior T-0468 R1 report was treated as claims only. This review re-read the committed `src/main.rs`, `src/reviewed_build.rs`, the source checkpoint, and the cited signed-main-image historical authority records. No CLI command was invoked, no binary was built, and no host, signer, envelope, image, daemon, Wake, tunnel, Git index, or remote state was changed.

## Source evidence

- `src/main.rs` dispatches `REVIEWED_MAIN_IMAGE_STATUS_FLAG` through `parse_reviewed_main_image_status_args` before normal command processing. A successful readback prints only the bounded status record; refusal exits with code 2. The fixed flag has no MCP or autonomous-contract exposure.
- `parse_reviewed_main_image_status_args` accepts exactly one instance of the fixed flag and no other arguments. Empty arguments select no status path; duplicates and all extra/path/epoch/output arguments fail with the fixed refusal.
- `run_reviewed_main_image_status_command` uses `production_reviewed_main_image_trust_root`, the fixed bootstrap policy, and the fixed rotation policy. It reads the accepted receipt plus optional installed and pending rotation receipts only through the existing signed-envelope validators. Those validators retain canonical-envelope, purpose/policy/root-identity, and product-root signature verification before payload authority is used.
- `select_signed_main_image_readback` selects the accepted receipt when no installed rotation exists, or the installed rotation receipt when valid. It refuses rollback or ambiguity: installed or pending epochs at or below accepted, a pending epoch below installed, and conflicting installed/pending content at the same epoch. An identical same-epoch installed/pending pair remains an idempotent non-ambiguous representation; pending alone is never promoted to current authority.
- On Windows, the status path opens only the fixed bootstrap destination through `open_fixed_reviewed_main_image_file`. Existing fixed-parent, no-follow regular-file checks are used before the read-only handle is accepted; the handle is opened with read sharing only, excluding write and delete sharing. `evidence_from_open_regular` measures SHA-256 and byte length from that retained handle, and `verify_reviewed_main_image_payload_binding` binds them to the selected signed receipt.
- Before reporting success, the implementation rereads each receipt and remeasures the same open image handle. Any receipt change or handle evidence change becomes `REVIEWED_MAIN_IMAGE_ROLLBACK_STATE_INVALID`, closing the readback TOCTOU window without reopening a caller-selected path.
- The status record is a fixed vocabulary: verified state, bootstrap/installed/pending epochs, payload SHA-256, and payload length. It does not print raw envelope bytes, signatures, paths, or trust-root material. The non-Windows implementation returns `REVIEWED_MAIN_IMAGE_TRANSPORT_UNAVAILABLE` rather than emulating an unsupported platform.

## Test and CI evidence limits

The local current-plan checkpoint records the source author's completed checks: two new exact-CLI tests, three existing signed-main-image tests, `cargo test --locked --offline --bin catdesk`, Clippy, formatting, and diff checks. This review did not rerun tests because the approved task is source-only and forbids a new host CLI/build action.

The same checkpoint records GitHub Actions run `38076645150` as **IN_PROGRESS** at its latest locally available observation. No completed result for that run was available in the reviewed project evidence, so it is not counted as passing CI. Earlier successful runs `38066384342` and `38067618614` support earlier short-Cargo-home source checkpoints only; they do not establish this T-0468 source revision.

## Remaining risks and operational next steps

1. Keep the serving controller and all signed-main-image material unchanged until a separately authorized, read-only host operation is approved.
2. In that operator-owned context, invoke only the fixed zero-argument status flag and retain its bounded result. Do not treat a source-only review as a rotation, installation, or reload authorization.
3. If the status command reports a refusal, preserve the fixed refusal category and seek an independently reviewed remediation; do not infer the accepted epoch, install a receipt, or retry a rotation from this result.
4. If it reports verified, any subsequent signed-image rotation, daemon reload, or recovery action still requires its own signed authority, review, approval, and rollback plan.
5. Obtain a completed result for GitHub Actions run `38076645150` (or an equivalently attributable source gate) before treating the source-test/CI portion as independently evidenced.

## Conclusion

No source defect was found in the bounded readback design: exact parser closure, signed receipt verification, no-follow fixed-image binding, same-handle/receipt rechecks, rollback and pending-state refusal, and non-Windows refusal all have direct source evidence. The command has not been run on the protected host and the available checkpoint does not establish completed CI for this commit. Therefore the source design passes this review, while operational main-image status and any deployment decision remain blocked.
