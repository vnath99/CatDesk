# T-0474 R1 — Fixed-path read-only Windows rotation handle preflight

Date: 2026-10-10 (America/New_York). Canonical human/hourly Chat51: https://chatgpt.com/c/6acaae13-4b18-83e9-b6ca-3c5e00cf47a8.

## Motivation and exact current evidence

The latest operator-run T0372 read-only inspection (UTC 2026-10-11T01:51:46.5645762Z) at `.catdesk/t0372-rotation-state.json` found an exact, strongly bound T0366 signed epoch2 transaction:

| Artifact | Current observation |
|---|---|
| Program Files installed image | exact T0215 epoch1, SHA 2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459, 25,134,592 B |
| Protected pending signed envelope | exact T0366 signed epoch2 SHA 42ababb6dcd47ec20db97775d7d441eb9fcadc806d3f304b7f221809bfda892c, 507 B |
| Protected installed rotation envelope | absent |
| Protected staging image | exact T0366 SHA d09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f, 26,302,464 B |
| ProgramData incoming image | same exact T0366 payload |
| ProgramData incoming signed envelope | same exact T0366 envelope |

Separately, the fixed operator-run Ed25519-public-root T0472 readback independently reported `SIGNED_MAIN_IMAGE_READBACK state=VERIFIED bootstrapEpoch=1 installedRotationEpoch=0 pendingRotationEpoch=2` with exactly those installed and pending SHA/length identifiers. The T0372 inspector alone hashes public file bytes; it is not an Ed25519 trust-root proof. These corroborating observations are not yet a protected image rotation.

Historical Sep10 `.catdesk/t0373-rotation-result.json` shows `REVIEWED_BUILD_MAIN_IMAGE_ROTATION_INSTALL_FAILED` for this exact signed T0366 identity. Existing T0373 mutating script is **not rerunnable** because it requires no pending/staging receipt. Rust source `src/reviewed_build.rs` retains staging and current-image handles and calls `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`. The historical error class can arise at stage create/copy/recheck, MoveFileEx, or post-replacement verification. Now that exact completed staging is visible and installed remains epoch1, a blocked atomic replacement is a plausible leading explanation, but is **not established** by error category alone.

## New source-only operator diagnostic

`scripts/t0474_readonly_rotation_open_probes.ps1` is fixed-no-parameter Windows PowerShell. It verifies hashes/lengths and no-reparse status for five fixed, known public host file paths (installed epoch1, staged and incoming T0366 image, pending and incoming signed T0366 envelope), then probes with Win32 `CreateFileW` and immediately closes the handles:

1. `GENERIC_READ` + `FILE_SHARE_READ | FILE_SHARE_DELETE`, matching the rotation reader's intended open combination, on installed/staging/incoming images;
2. request only `DELETE` access (without setting delete disposition), `FILE_SHARE_READ|WRITE|DELETE`, on the same three objects. This is an **access test** only and never deletes, renames, replaces, truncates or writes any target file.

Record only `OPEN_OK` vs Win32 `ACCESS_DENIED` (5), `SHARING_VIOLATION` (32) or the bounded numeric code, plus whether the current token is administrator. Win32 5 from non-admin context is **not proof** that administrator rotation is blocked; Win32 32 does not identify the holding process; passing both probes does **not** guarantee `MoveFileExW` can replace the executable. Exact public hashes/lengths are rechecked after probes to reject changed file state.

The script writes only the workspace result `.catdesk/t0474-rotation-open-probes.json` and prints the same bounded result. It never opens a write handle or calls a mutating API on a protected file and does not elevate. It dynamically compiles a fixed C# `DllImport` `CreateFileW`/ `CloseHandle` shim via `Add-Type`; compiler temporary files outside protected locations are not security/rotation state. It uses no arbitrary arguments/paths and performs no signer/private-key, service, browser, controller, tunnel or Wake action.

Exact operator invocation (one-line only, ordinary non-admin PowerShell):
```powershell
cd 'C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop'; & '.\scripts\t0474_readonly_rotation_open_probes.ps1'
```

If failed, preserve original signed receipts and report bounded error. If successful, CatDesk MCP can read only new workspace JSON. No automatic rotation or admin action is authorized by any probe result. Prior source T0473 archive `docs/orchestrator/review_bundles/T-0473_R0_ARCHIVED_T0372_HOST_READBACK_20260910.json` preserved historical Sept10 observation.

## Verification gates

- CI `.github/workflows/ci.yml` adds a **nonexecuting** Windows PowerShell syntax check plus C# P/Invoke shim compilation from the exact embedded fixed source for the new script; the fixed file-open probes themselves must **not** run in generic GitHub CI.
- Script requires an operator host, not CatDesk arbitrary PowerShell `run_command` while allowlist is active; no attempt to bypass shell-mode boundaries.
- Local static checks and GitHub CI will be recorded after the scoped source commit. No runtime CreateFileW success claims until a real operator-run result exists.
- T0473 historical evidence/documentation commit `307c1e4` passed all 3 GitHub Windows CI jobs, run `38102506078`; the older 0a16ed3 run 38101313833 separately failed Windows file ACL/sharing fixture tests, not attributed to main-image rotate.
- Independent source review of T0470/T0471/T0472/T0474, next signed image rotation decision, source-current product-root signed release, approved guarded BUILD_ATTESTED promotion, nine-layer Recovery, and atomic paired Chat51 target gen>=32 remain future gates. WakeHost is STOPPED at obsolete Chat48 gen31.

## Prohibitions

No direct protected file edits, no clearing signed pending, no replacement/rename of staging, no raw MoveFileEx/installer, no re-running T0373 mutator, no source-current unsigned debug image promotion, no unrestricted CatDesk shell, no unnecessary elevation, no offline signing-key exposure, no external secure tunnel mutation or stopped Python browser Wake restart. Preserve seven unrelated historical untracked files.
