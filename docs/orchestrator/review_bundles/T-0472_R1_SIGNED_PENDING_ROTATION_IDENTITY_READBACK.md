# T-0472 R1 — Signed pending-rotation identity readback (source only)

Date: 2026-10-10. Canonical Chat51: https://chatgpt.com/c/6acaae13-4b18-83e9-b6ca-3c5e00cf47a8.

## New operator-host observation

Operator executed the existing T-0469 approved fixed, read-only status CLI and returned the bounded receipt:

```text
SIGNED_MAIN_IMAGE_READBACK state=VERIFIED bootstrapEpoch=1 installedRotationEpoch=0 pendingRotationEpoch=2 payloadSha256=2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459 payloadLength=25134592
```

This is operator-provided host output from the approved diagnostic, not a new independent MCP read. It corresponds exactly to the historical signed epoch-1 bootstrap payload SHA and length. The source readback verified the compiled production Ed25519 public-root accepted and optional installed/pending envelopes, then remeasured the installed file against the **installed** receipt. An optional pending epoch-2 envelope was present and signed/validated; it is **not installed**. The output does not reveal the pending envelope's payload SHA, length, incoming image availability, readiness, or installability.

The T-0217 2026-08-22 historical pending epoch-2 candidate was SHA `552cb917998d283e7d901593ffed5a9ceaa108223654aa69349fa7e567c21482`, length 25,172,480, originally only an offline signing request. Its current identity CANNOT be inferred just from matching epoch number. It predates latest source T-0470/T-0471. Do not activate or auto-install an unidentified and possibly outdated epoch-2 candidate, or presume a fresh epoch-3 request can safely supersede existing pending state. Source `execute_reviewed_main_image_rotation_as_administrator` requires the signed incoming envelope, incoming image digest/length, accepted predecessor and exact pending agreement. A different incoming epoch/image while pending does not match is refused.

## Narrow source-only status extension

In `src/reviewed_build.rs::run_reviewed_main_image_status_command`, after the same signature verification, fixed-host path opening and receipt/file stability rechecks, append **two already-authenticated public identifiers** to the existing fixed one-line result:

- `pendingPayloadSha256=<64-hex|none>`
- `pendingPayloadLength=<positive integer|0>`

These come from the verified pending envelope, never from an untrusted external caller or caller-selected file. The existing installed `payloadSha256/payloadLength` fields are unchanged. With no pending receipt the new fields are `none` and `0`; the pending epoch remains zero. No signature, trust root, raw receipt bytes, user path, content or signing key is output. No alternate CLI flag, executable permission, signer, installer, target, elevation, Wake or daemon capability is created.

Tests are added to the existing `signed_main_image_readback_` suite to confirm pending and installed cannot be confused, pending hash and size remain from the same signed receipt, and the fixed output still includes the new fields. Requires independent source review and fresh CI before treating it as independently approved operational status.

## Verification and remaining gates

- Local `cargo test --locked --offline --bin catdesk signed_main_image_readback -- --nocapture` passed 2/2 after implementation; full binary suite passed exit 0, strict workspace all-targets all-features Clippy passed, `cargo fmt --check` passed, and `git diff --check` passed (line endings warning only).
- The local `cargo build --locked --offline --bin catdesk` passed, creating an updated **debug diagnostic**, NOT a signed main-image release or installed controller.
- Local source/commit and CI verification to be reconciled before final operator instruction. The fixed readback requires only ordinary non-elevated PowerShell and must return the updated bounded one-line status or bounded refusal; do not attempt the rotation flag. No binary was launched through the blocked CatDesk MCP `run_command`.
- Independent T-0471 promotion schema commit `789c75bfc06aaac0a7db8700e9dad7294ead8b13` passed GitHub Actions run `38099779040`, including all three Windows jobs. This establishes source CI, not active serving/controller parity or rotation authority.

## Actions NOT performed

No protected Program Files/ProgramData file accessed or changed through MCP; no raw signed envelope read; no signing key, signature generation, new receipt, signed-image rotation, source-current daemon deployment, protected promotion, reviewer authority substitution, paired wake-target change, Python browser Wake event, external Secure MCP tunnel mutation, or unrelated untracked-file cleanup.
