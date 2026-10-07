# T-0419 R3b — Closed SystemRoot Source-Current Independent Review

Date: 2026-10-07
Session: `adc-t0419r3b-systemroot-source-review-20261007`
Logical task: `T-0419-R3B-SYSTEMROOT-SOURCE-REVIEW`
Reviewed source commit: `fb96890a3027081dc804d89bce0575f3c78f02a7`
Candidate code commits: `ed2c72a2714a6b5a0ebaff04ac6c83c3bd80032b` and `6b9b9ac194e64926c416fe44323ce0eaa03634fb`
Review decision: **PASSED FOR ONE FRESH PROTECTED BUILD ONLY**, conditional on successful CatDesk source-authority binding; not release or live acceptance.

## Bounded failure evidence
- Exact T-0419 R2 reviewed attempt `fa79de2f334c42c3abdbf9d956817fae` was terminal `BUILD_FAILED_OR_AMBIGUOUS`, `CARGO_BUILD`, Cargo exit 101; no attestation.
- Test-only, full-drained-stderr, bounded extractor detected the safe error token `msvcFatalCode=Some("D8037")` twice in local diagnostic logs for `ring 0.17.14`. The extractor and intentionally forced-panic test hook were restored to HEAD before this reviewed source.
- Microsoft Learn documents D8037 as inability to create a temporary compiler IL file, normally associated with the configured TMP directory. A known CreateProcess case separately found the same error when cl.exe could not resolve `SystemRoot`; this is contextual support, not CatDesk-specific root-cause proof.

## Candidate scope and security review
- The reviewed worker still invokes Cargo with `.env_clear()` and exact pinned toolchain values, including `CC`, `AR`, `LIB`, `LIBPATH`, `INCLUDE`, `PATH`, `CARGO_HOME`, `RUSTC`, `SystemDrive`, `TEMP`, and `TMP`.
- The candidate adds only `SystemRoot`, resolved through the existing Windows `GetWindowsDirectoryW` API, with drive-root validation; it never imports ambient `SystemRoot`, VS installation variables, registry toolchain selection, developer-shell state, or arbitrary caller paths. On non-Windows hosts the helper fails closed.
- The closed-environment regression asserts that `SystemRoot` is present and equals the Windows-derived value, and forbids use of `std::env::var(_os)` for ambient `SystemRoot`.
- Source-review changes do not grant authority to mutate external Secure MCP tunnel ownership, WakeHost, canonical release, daemon reload, or Git main/master.

## Verification at reviewed source
- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --locked --offline --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --locked --offline fixed_v5_host_linker_diagnostic_command_is_env_closed_and_has_no_persistence_seam`: PASS, 1/1 focused test.
- `git diff --check`: PASS.
- `cargo build`: PASS in prior candidate verification.
- **Broad `cargo test` is NOT attested as passing.** A previous verify_project invocation returned exit 101; a later long run timed out at the connector boundary. The previously documented AppContainer host permission fixture is one plausible unrelated cause; no fresh evidence established that it is the sole failing test. Do not claim full-suite acceptance.

## Review authority and release gating
- The first R3 read-only review session `adc-t0419r3-systemroot-review-20261007` was `COMPLETED_VERIFIED` but lacked completion-artifact identity and consequently did NOT constitute protected-build authority. This R3b record instead binds this new attributable, durable completion artifact from an absent-at-claim baseline to its exact post-review bytes.
- Review authority must be acknowledged and remeasured by CatDesk; it cannot be substituted with chat text or a stale R2 record.
- If CatDesk's R3b authority preflight accepts, run exactly one fresh protected build. Promotion or daemon reload is forbidden unless the build state is `BUILD_ATTESTED` and the canonical/serving parity chain is proven.
- If D8037 persists, investigate protected temporary directory existence, writable ACLs, disk capacity and runtime DLL/SystemRoot dependency within bounded diagnostics. Do not import arbitrary parent environment.
- Historical manual Wake ATTENTION does not invalidate already accepted natural R1/R2 wakes. Canonical project and Wake target remains generation 30, `https://chatgpt.com/c/6ac63f72-a2ac-83e9-bf61-db8ab9d97224`.

Operator action: NONE. Ordinary continuity: next event-driven Python browser wake on the actionable R3b review event; hourly deadman is fallback, deferring to an active manual writer.
