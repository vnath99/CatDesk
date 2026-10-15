# T0478 R3B4-B3B2B2C — Owner-bound disposable fixture retirement source

Date: 2026-10-14. Scope: dormant source path and Windows tests. No MCP, worker, installer or production cleanup call is connected.

## Source change

- The disposable snapshot inventory now uses the existing `NtQueryDirectoryFile` enumeration on the pinned directory handle. It checks the exact manifest leaves and refuses directory/reparse entries, then rereads each file through its no-follow, single-link handle and rehashes bytes.
- `StagedCandidateSnapshot::retire_owned_root(self)` consumes the producer-owned in-process snapshot. It first verifies the manifest/inventory and retained object identities, derives the interactive owner SID from the process token, and reads the exact owner/DACL from the pinned root handle. It releases retained read leases, then opens and verifies **all** manifest-known DELETE handles against retained file IDs, sizes and SHA-256 before the first disposition. It rechecks the pinned inventory and DACL, disposes only those opened files, proves the root empty, and reopens the direct child with DELETE access under the pinned temp parent. The reopened root's Windows volume/file-index identity and owner/DACL must match before its opened handle is disposed. A final pinned-parent lookup requires the generated root name to be absent.
- The shared protected filesystem adds a consuming direct-child reopen that drops the original non-delete-sharing handle and compares the new object ID. It requests READ_CONTROL so the reopened handle can verify its actual security descriptor. A test-only hook exercises a replacement at that precise gap.

## Local tests

- Positive generated owner-only fixture with two manifest leaves: exact files and empty root retired.
- Foreign injected leaf: retirement refused before any disposition; both expected and foreign bytes remained.
- Extra BUILTIN\\Users ACE in an otherwise generated fixture root: pinned-handle owner/DACL check refused retirement before any disposition; expected bytes remained.
- Replacement root created at the same generated name after old-handle release: reopened ID mismatch, replacement and original test roots remained.
- Existing hardlink, modified-byte, wrong-ID, active-lease and malformed inventory negatives remain covered by the focused fixture/protected filesystem suites.
- `cargo test --all-features --quiet`: exit 0. Main binary 1,074 passed / 0 failed / 26 ignored; integration suites passed. The AppContainer child access-denied diagnostic is expected inside its passing parent test.
- `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --all -- --check`: exit 0. `git diff --check` and staged diff check pass.

## Remaining gate

The source method has no production caller or durable transaction record. A process crash after the first file disposition could leave an incomplete owned root; this code never scans by name or automatically retries that residue. A live owner must persist trusted snapshot/root identities and a bounded operator-attention outcome, exclude competing workers, and prove failure/restart behavior before enabling cleanup. T0478-C MCP manifest gating, T0478-D worker lifecycle, T0479 versioned/LKG release, and independent three-job Windows CI remain outstanding. Do not treat this source test as installed acceptance.
