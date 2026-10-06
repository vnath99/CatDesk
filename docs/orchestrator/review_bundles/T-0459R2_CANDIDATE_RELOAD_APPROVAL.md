# T-0459R2 candidate reload approval

## Preconditions

The isolated bootstrap session
`adc-t0459r1-source-current-bootstrap-build-20261005` is inactive and
`COMPLETED_VERIFIED`. Its durable finalization records `rust_full` verification
as `PASSED`; its required R1 review artifact remains present.

The candidate exists as a regular, non-reparse workspace-contained file. This
task did not rebuild it and did not require a temporary measurement seam.

## Read-only candidate measurement

- Candidate path:
  `.catdesk\verification-targets\autonomy-release\release\catdesk.exe`
- SHA-256: `ada0afee5d4c75630932929bbf804a3e53153f171c1db5c60340e5967a34f9ef`
- Byte length: `26838016`

## Canonical approval binding

`src/daemon-reload-approval-request-v1.json` now contains the canonical
schema-1 `catdesk-daemon-reload-approval-v1` request for precisely that
workspace-relative candidate identity. The canonical compact inner request
SHA-256 is `44ce651d1e221030e945b4da364d5299457bb2068abe833c53e8d75c67ddcc4a`.

The replaced prior file bound a different `target-verify` candidate and is not
authority for this isolated candidate. This artifact is a review input only;
it neither executes a daemon reload nor grants promotion, LKG, recovery, or
reviewed-build authority.

## Non-actions

No daemon reload, protected reviewed-build operation, promotion, recovery,
Wake/tunnel/runtime mutation, Git operation, or external Secure MCP action was
performed. No candidate bytes were rebuilt or modified.

## Verification

- `cargo test daemon_reload_approval::tests -- --nocapture`: passed (3 tests).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `cargo test --workspace --all-targets --all-features`: passed (1,030 root
  tests in this workspace profile; existing ignored tests remained ignored).
- `git diff --check`: passed; only inherited CRLF conversion warnings were
  emitted for existing dirty tracked files.

Independent review must remeasure the file before any typed reload preflight or
confirmation is considered.
