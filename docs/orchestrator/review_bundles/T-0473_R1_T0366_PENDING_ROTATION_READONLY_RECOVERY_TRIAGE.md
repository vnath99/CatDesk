# T-0473 R1 — Exact signed T0366 pending rotation and read-only recovery triage

Date: 2026-10-10 local / 2026-10-11 UTC. Canonical Chat51: https://chatgpt.com/c/6acaae13-4b18-83e9-b6ca-3c5e00cf47a8.

## Operator-provided signed readback — NEW

The operator ran the already approved, source-reviewed, zero-argument, non-elevated fixed signed-image readback CLI built from T0472, and supplied this bounded line:

```text
SIGNED_MAIN_IMAGE_READBACK state=VERIFIED bootstrapEpoch=1 installedRotationEpoch=0 pendingRotationEpoch=2 payloadSha256=2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459 payloadLength=25134592 pendingPayloadSha256=d09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f pendingPayloadLength=26302464
```

The compiled product Ed25519 public root authenticates the accepted epoch1 and pending epoch2 receipts. The actual installed image matches T0215 epoch1 25,134,592 B, SHA 2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459. No installed rotation receipt. The **pending signed envelope** binds EXACT T0366 epoch2 payload SHA d09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f, 26,302,464 B — NOT historical T0217's distinct signed epoch2 image SHA552cb917998d283e7d901593ffed5a9ceaa108223654aa69349fa7e567c21482, 25,172,480 B.

This is strong, positive operator-host signed evidence for current *receipt* identity; it is **not proof** that the fixed incoming payload, fixed incoming signed envelope or fixed rotation staging executable still exist and match T0366 now. Neither incoming transport nor staging is selected as the installed image by the status code.

## Historical failure and narrowing

- Sept 10 T0372 immutable workspace record `.catdesk/t0372-rotation-state.json` initially showed T0215 installed, accepted signed epoch1, **no pending/installed/staging**, exact incoming T0366 candidate, but old signed T0217 incoming envelope. We made a content-preserving forensic snapshot in `docs/orchestrator/review_bundles/T-0473_R0_ARCHIVED_T0372_HOST_READBACK_20260910.json` BEFORE any request to refresh the T0372 inspector.
- T0373 existing script `scripts/t0373_apply_epoch2_after_protected_state_readback.ps1` relies on pending and installed rotation receipts AND staging being absent. It cannot be rerun in the current pending2 state, and must not be modified to sidestep this guard.
- Sept 10 exact result `.catdesk/t0373-rotation-result.json`: `state=FAILED`, `REVIEWED_BUILD_MAIN_IMAGE_ROTATION_INSTALL_FAILED`, designated T0366 signed 507 B envelope SHA42ababb6dcd47ec20db97775d7d441eb9fcadc806d3f304b7f221809bfda892c, payload SHA d09c677f... length 26,302,464. The matching current pending2 strongly establishes a **persisted, incomplete T0366 transaction**, but error stage not yet independently proved. There may be installed-image file-sharing/lock issues or failure to stage/copy; do not pick one without state evidence.
- Current `src/reviewed_build.rs` can emit `ROTATION_INSTALL_FAILED` from fixed staging create/copy/hash, `MoveFileExW` replacement, or postreplace open/remeasure/binding; the code deliberately folds distinct Windows failure categories into one bounded code. Crucially, the function can RECOVER an identical verified pending receipt through its strict unchanged incoming/predecessor/staged-hash checks, whereas the outer T0373 shell script refuses to begin once any pending exists. **No retry is authorized solely by this inference.**

## Read-only next evidence: seven fixed file identities, no reinstallation

Existing fixed historical inspector `scripts/t0372_inspect_main_image_rotation_state.ps1` performs only fixed-path file existence/length/hash inspection plus public envelope metadata; it writes a workspace JSON `.catdesk/t0372-rotation-state.json` and prints `T0372_ROTATION_STATE_READBACK_COMPLETE`. The Sept10 original JSON is now archived above so a fresh run will not erase the only preserved historical observation. It does NOT cryptographically verify receipts or authorize a rotation; signed trust provenance comes from the separately executed fixed T0472 status.

In the operator's **ordinary, non-elevated Windows PowerShell** on the CatDesk machine, run exactly:

```powershell
cd 'C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop'; & '.\scripts\t0372_inspect_main_image_rotation_state.ps1' | Out-Null; Write-Output 'T0372 read-only state written to .catdesk\t0372-rotation-state.json'
```

This writes only the workspace JSON (no protected path mutation), opens fixed host locations for read-only metadata, does not request UAC, and is NOT the similarly named T0373 installer. If the inspector reports AccessDenied, symlink/ambiguous type, or another failure, stop and provide the bounded error: no workaround/elevation or inference that a protected file is absent. If it succeeds, the ChatGPT/CatDesk read-only `read` MCP action can ingest the workspace JSON without copying full host paths or raw envelope contents into chat.

Compare:
- `rotationStagingCandidate`: absent, present exact T0366 SHA/length, present foreign/unexpected, or access denied.
- `incomingCandidate`: present exact T0366 or missing/mismatched.
- `incomingEnvelope`: present exact T0366 signed SHA42ababb6... size507 or unexpected/missing.
- `installedCandidate`, `pendingRotationEnvelope`, `installedRotationEnvelope`: must remain compatible with the separate trusted signed state; unknown/readback errors fail closed.
- No manually clearing pending/staging, manipulating Program Files/ProgramData, invoking `--catdesk-reviewed-main-image-rotate-fixed-policy` or T0373 installer, copying unsigned binaries, killing serving daemon, replacing external Secure MCP tunnel or starting WakeHost.

## Source/publication context and CI

T0471 promotion-schema source commit 789c75b and T0472 pending readback source commit 82804ac each passed their three Windows CI jobs (runs 38099779040 and 38100806031). The later docs-only commit 0a16ed3 CI 38101313833 failed its Rust job due two Windows test fixture file errors: appcontainer fixed helper create-new `Access is denied` (Win32 5), and stable wake lease fixture `file being used by another process` (Win32 32), while Python and Independent WakeHost Rust passed, fmt/clippy and PowerShell fixture succeeded. Both failures were observed via full CI log; do not assume this is definitively flaky, and do not attribute production signing/rotation problems to CI fixture failures. A request to rerun failed CI through connected GitHub app was blocked by host safety checks; it was NOT rerun. No code fix was invented from this non-attributed docs-only failure.

## Remaining gates

1. Fresh operator-host read-only T0372 inspection of exact incoming/staging and protected snapshot, reconcile with signed fixed T0472 output. Do not rotate on absence/inconsistent evidence.
2. Independently review source T0470/T0471/T0472 and resolve any actionable CI defect; determine exact signed recovery or source-current re-sign path with externally held offline Ed25519 product root. The T0366 signed epoch2 image is older than current T0472 source, so simply completing T0366 rotation does not deploy current controller.
3. Only with exact authoritative candidate, pending-consistent transport, known host lock/ACL state and explicit approved fixed recovery do any mutation. Later serving/current digest and source parity, fresh BUILD_ATTESTED, guarded reviewed promotion, nine-layer Recovery, paired project+Wake Chat51 generation >=32, then browser Wake natural acceptance.

No host protected files directly read or modified in this ChatGPT turn, no rotation, signing, signer disclosure, service/Wake restart, target bind, external Secure MCP edit or unrelated worktree cleanup.
