# T0478 R3B4-B3B2B2B — Exact delete-handle preflight

Date: 2026-10-14. Scope: source-only groundwork for owner-bound fixture retirement. No fixture runtime cleanup or production release is enabled.

## Implementation

- Moved the reviewed native opened-handle disposition routine from `reviewed_source_snapshot.rs` into shared `windows_protected_fs.rs`. Reviewed snapshot staging cleanup and fixed receipt rollback now call one primitive; it accepts an opened handle, never a path, and retains the Windows extended-disposition then legacy fallback behavior.
- Added `open_reviewed_regular_for_delete` for a previously retained artifact. It opens one manifest-known direct child under an already pinned parent with DELETE access and read-only sharing, refuses reparse/directory and multi-link objects, compares the current volume/file-index identity, exact byte length and bounded SHA-256, and returns the still-open file handle without deleting it. A retained read lease must be released before the DELETE open can succeed; the returned handle blocks subsequent writers and delete opens.
- Added disposable Windows tests for active-lease refusal, wrong object identity, wrong digest, hardlink refusal, changed bytes refusal, and positive exact opened-handle disposition. Tests remove only their own generated flat temporary files and empty roots.

## Verification

- Focused `reviewed_delete_handle` tests: 2 passed.
- Existing `reviewed_source_snapshot` tests: 26 passed.
- Root `cargo test --all-features --quiet`: exit 0, main binary 1,070 passed / 0 failed / 26 ignored, integration suites passed. The AppContainer child access-denied output is the expected fail-closed child fixture; its parent test passed.
- Root strict `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`: pass.

## Remaining B3B2B2B authority

This helper is deliberately not a fixture retirement route. A real transaction still must verify the token-derived owner and pinned root DACL, enumerate the exact pinned directory, compare **all** manifest children and retained identities before the first disposition, release read leases, prevent competing owners, and delete the exact opened empty root. Crash residue needs durable trusted ownership evidence and operator attention on uncertainty. T0478-C manifest-gated MCP service remains after this gate. No runtime, installer, signer, Task Scheduler or Secure MCP tunnel change was made.
