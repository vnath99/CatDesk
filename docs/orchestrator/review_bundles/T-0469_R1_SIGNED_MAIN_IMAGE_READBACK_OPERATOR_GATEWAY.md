# T-0469 R1 — Operator-authorized signed main-image readback / installed gateway boundary

**Date:** October 10, 2026. **Canonical ChatGPT thread:** CatDesk_chat50 `https://chatgpt.com/c/6aca50a3-0da0-83ea-8358-dbc128c4f9ad`.

**Classification:** `OPERATOR_READONLY_STATUS_AUTHORIZED; INSTALLED_MCP_COMMAND_GATE_DENIED; SIGNED_HOST_STATE_UNOBSERVED`.

This is an operator runbook and boundary diagnosis, **not** a signed review, deployment, host-status receipt, signature, or reviewed build. The user explicitly authorized the read-only signed main-image diagnostic on this turn.

## What is independently supported

- Source T-0468 at `65e881bd05b8379bd595650b905b047d047e277f` implements `--catdesk-reviewed-main-image-status-fixed-policy` as an exact-only CLI action in `src/main.rs`, using `src/reviewed_build.rs::run_reviewed_main_image_status_command`. It validates canonical, product-root Ed25519 signed accepted and optional installed/pending rotation envelopes, fixed signed policy/purpose, and the installed fixed image's SHA-256 and length using a read-only no-follow opened file; remeasures receipts and same image handle before reporting fixed fields. It does NOT accept an output path, key, alternate candidate, epoch, installer option, daemon operation or Wake action. See T-0468 independent R2 source review (COMPLETED_VERIFIED / ACKED) and GitHub Actions run `38076645150` (all three Windows jobs success).
- A source-current local **debug** executable was built by `cargo build --locked --offline --bin catdesk` (exit 0); it was **not** executed or installed as a reviewed/signed main image. Repository tracked source is at documentation commit `bd279fa`, with no intervening source edits relative to source commit `65e881b`.
- Current serving CatDesk started at `unix:1791334517`, reports Git build identity UNKNOWN, and predates T-0468. It cannot serve that new status flag through the old MCP controller. CatDesk's independent WakeHost remains deliberately STOPPED on old Chat48 target generation31.
- A **dry-run** `CatDesk.run_command` for `.\\target\\debug\\catdesk.exe --catdesk-reviewed-main-image-status-fixed-policy` returned generic dry-run success; the actual invocation returned **`INVALID_ARGUMENT`**. **No process execution, signed receipt, or installed image measurement was established**. Source explains this: `src/mcp.rs::handle_run_command` emits a success-shaped dry-run before checking `validate_shell_mode` (lines circa 1298–1374). The configured `allowlist` shell in `src/mcp.rs::validate_allowlisted_shell` admits Cargo/Git/Codex, etc., but does not admit the CatDesk executable name (circa 3938–3975). Generic remote `INVALID_ARGUMENT` does not expose the precise failed validator, so the allowlist explanation is independently **source-consistent**, not a measured internal-error code. No attempt to switch to unrestricted shell or older plugin was made.

## Narrow manual operator action — read-only, non-elevated first

In an ordinary **Windows PowerShell** terminal on the CatDesk workstation (not ChatGPT, not CatDesk.run_command), use only:

```powershell
cd 'C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop'
& '.\target\debug\catdesk.exe' '--catdesk-reviewed-main-image-status-fixed-policy'
```

No administrative elevation, signing key, `cargo run`, program-files copy, service restart, legacy reload, browser command, executable substitution, extra arguments or installer flag is requested. A normal user may receive a protected-file access denial, which must be reported verbatim as the **bounded fixed error** without retrying with an alternate/unsafe reader.

On success, expected exact output shape is:

```text
SIGNED_MAIN_IMAGE_READBACK state=VERIFIED bootstrapEpoch=<decimal> installedRotationEpoch=<decimal> pendingRotationEpoch=<decimal> payloadSha256=<64lowercasehex> payloadLength=<positiveinteger>
```

A zero `installedRotationEpoch` or `pendingRotationEpoch` means no authenticated receipt at that respective fixed path; it is **not** permission to assume the signed epoch is 1 or to sign epoch2. If failure, return the single `CatDesk signed main-image status: <fixed failure>` line and exit code (if available). Only these fixed status fields or bounded error should be pasted back to the canonical chat; do not share private keys, raw envelopes, credentials, user profile contents, ProgramData/ProgramFiles listings or unrelated logs.

## Verification rules and next authority gates

1. Treat a successful output as an **authenticated observation by the source-current debug tool**, distinct from an independently attested installed/serving image and from a product-root signed rotation approval. Confirm exact source/compiled root and positive output, then reconcile the accepted bootstrap epoch, installed/pending receipts and measured installed image. If the CLI refuses, stop and obtain a separately reviewed fixed host-read transport instead of extracting protected receipt bytes manually.
2. Do **not** promote the scratch T-0464 Cargo build or local debug executable on the strength of this observation. A new executable must have an independently reviewed, exactly bound source and bytes; legitimate **offline** Ed25519 signing by the operator-held product root, plus a signed envelope strictly fresher than accepted epoch, and separately approved administrator fixed-policy rotation. The private signing key must remain outside CatDesk, GitHub, MCP and ChatGPT.
3. After source-current serving is independently verified, perform a **fresh** protected reviewed build with `BUILD_ATTESTED`, guarded promotion, nine-layer recovery diagnostics, and atomic paired Chat50 project/Wake target migration to `https://chatgpt.com/c/6aca50a3-0da0-83ea-8358-dbc128c4f9ad`, digest `625ddb77fa11ea42663806c7e9e436c81a6a01368e1d14f6f68c8a7ac456d0fc`, generation >=32. Then (and only then) reconsider WakeHost restart and manual/natural wake acceptance.

## Explicit non-actions

No protected host file was read. No image was installed, signed, promoted, copied or reloaded. No admin elevation, private key use, arbitrary shell execution, external Secure MCP tunnel change, Binagotchy GUI launch, Wake send/start, old target rewrite or unrelated filesystem/Git mutation occurred in this diagnostic. This document is for reproducible operator handoff and may be committed/published as source documentation only.
