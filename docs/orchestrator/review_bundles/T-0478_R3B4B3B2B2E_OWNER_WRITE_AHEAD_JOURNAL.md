# T0478 R3B4-B3B2B2E — Owner write-ahead journal source

Date: 2026-10-14. Scope: dormant witnessed candidate staging and protected filesystem helpers. No installed component, MCP route, WakeHost, signed image, tunnel, or scheduling state changed.

## Source change

- Before creating a candidate root, staging creates a separate generated direct child of the canonical OS temp directory with the token-derived user's protected owner-only DACL. It writes `intent.json` with the canonical snapshot ID, reviewed source commit, SHA-256 of the validated manifest, and generated root name. The fixed record is create-only, size bounded, file-synced, and read back through a pinned single-link regular-file handle.
- After root creation and owner/DACL readback, staging writes create-only `root.json` binding the intent hash to the opened root's Windows volume/file-index identity. Source bytes are staged only after this readback succeeds.
- A read-only reader reopens pinned journal and optional root handles, verifies exact names, protected owner/DACL, canonical record encoding, manifest and intent hashes, and opened root identity. A missing root record is an intent-only state, not deletion authority. A mismatch fails closed. The readback reopen requests `READ_CONTROL` specifically; ordinary protected child opens keep their prior access mask.
- The existing typed failure paths retain an in-process root lease on post-create error. An indeterminate native create error still grants no cleanup authority. Journal files remain forensic evidence after process exit; there is no generated-name scan or automatic retry/delete.

## Verification

- Windows process-crash simulation test: drop the journal guard, reopen and verify the pre-root intent; create and bind a root, drop both guards, reopen and verify the same root object; tamper the root object ID and require rejection while the root remains. The test removes only its own known flat records and empty directories.
- Focused snapshot suite: 12 passed, 0 failed. Full `cargo test --all-features --quiet`: exit 0; main binary 1,077 passed, 26 ignored, and remaining binary/integration suites passed. Strict `cargo clippy --all-targets --all-features -- -D warnings`, formatting and diff checks passed locally.

## Limits and acceptance gates

This is process-crash forensic persistence only. File sync plus pinned-handle readback has not proved power-loss durability of directory metadata on every filesystem. There is no trusted owner generation or bounded durable operator-attention state, journal retirement protocol, competing-owner exclusion across processes, worker lifecycle, or live MCP manifest binding. The reader does not authorize cleanup. T0478-C/D, T0479, production Recovery, and P0/P1 acceptance remain outstanding.

GitHub Actions dispatch currently returns HTTP 422, `Actions has been disabled for this repository`; exact-commit independent Windows Rust, Python Wake/advisor, and Independent WakeHost Rust job results are required before this source increment is accepted.
