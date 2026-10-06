# T-0459R3 — Canonical daemon-reload approval serialization

## Cause

T-0459R2 correctly measured and reviewed the current isolated CatDesk candidate, but wrote `src/daemon-reload-approval-request-v1.json` with one trailing newline byte. The reload authority parser intentionally requires the artifact bytes to equal the canonical compact JSON encoding exactly, so typed reload PREFLIGHT rejected the otherwise-correct review authority before mutation.

## Preserved candidate identity

- Relative path: `.catdesk\\verification-targets\\autonomy-release\\release\\catdesk.exe`
- SHA-256: `ada0afee5d4c75630932929bbf804a3e53153f171c1db5c60340e5967a34f9ef`
- Byte length: `26838016`
- Inner request SHA-256: `44ce651d1e221030e945b4da364d5299457bb2068abe833c53e8d75c67ddcc4a`

No candidate bytes were rebuilt or modified.

## Repair

The approval artifact is rewritten as the exact canonical compact JSON byte sequence with **no trailing newline or whitespace**. No fields or candidate identity values changed.

## Scope / non-actions

This session does not execute daemon reload, rebuild the candidate, touch reviewed-build state, promotion/LKG/recovery state, Wake target/profile, Git index/commit/push/publication authority, or the external Secure MCP runtime.

## Next

After independent verification and explicit ACK, use this exact review record with typed `catdesk_daemon_reload PREFLIGHT` using the candidate path and SHA-256 above. If the daemon remeasurement matches, CONFIRM only the returned persisted token, reconnect, prove current-source serving parity, then resume the protected reviewed-build and GitHub publication path.
