# T-0412R5 — Missing Link Library Diagnostic

## Scope

This review boundary covers only the already-present candidate edits in `src/reviewed_build.rs` that make a failed protected reviewed-build diagnostically actionable without broadening build or environment authority. The edits were made immediately before session creation and are therefore reviewed as pre-session candidate edits rather than attributed to post-claim implementation.

## Intended behavior

- Preserve the existing bounded Cargo stderr capture, classification, and SHA-256 diagnostic.
- For MSVC `LNK1104` / `LNK1181` only, retain at most one strictly validated `.lib` basename in `ReviewedBuildFailureDiagnosticV1.missingLinkLibrary`.
- Never persist a full linker path, unrestricted stderr, URL, environment value, compiler command, or arbitrary compiler output.
- Collapse a full quoted Windows or slash-separated path to its final basename before validation.
- Accept only non-empty basenames up to 96 bytes ending in `.lib` and containing ASCII alphanumeric characters plus `.`, `_`, or `-`.
- Keep the field optional with serde default / skip-when-none so older diagnostic records remain readable.
- Capture across the existing streaming classifier tail so a linker diagnostic split across pipe reads can still produce the safe basename.
- Do not alter the product-derived MSVC/Windows SDK environment in this ticket.

## Candidate changes

`src/reviewed_build.rs` now:

1. Defines `MAX_LINK_LIBRARY_BASENAME_BYTES = 96` and updates the diagnostic privacy comment.
2. Adds optional `missing_link_library` to `ReviewedBuildFailureDiagnosticV1`.
3. Adds `extract_missing_link_library_basename()` with strict basename-only validation.
4. Carries the optional basename through `CargoStderrClassifier` and `BoundedCargoStderr` into `summarize_cargo_failure()`.
5. Adds focused regressions for safe full-path collapse, unsafe-name rejection, and reader-chunk boundary extraction.

## Verification evidence before finalizer

- First command `cargo test cargo_failure_diagnostic_retains_only_safe_missing_library_basename -- --exact` compiled successfully but matched 0 tests because the Rust test name is module-qualified. It is not counted as regression success.
- Corrected focused command `cargo test cargo_failure_diagnostic_` completed successfully: 5 passed, 0 failed. This includes both new regressions and the existing Cargo failure diagnostic tests.
- A prior `verify_project` request hit the ChatGPT client timeout and produced no retained log containing the new test name, so no full-verification result is claimed from that call.
- A direct `cargo fmt --check` request was rejected by guarded shell policy before execution; it is not treated as a formatting failure.

## Review requirements

The autonomy finalizer must independently run the approved `rust_full` policy: formatting, strict Clippy, full Cargo tests, and authoritative diff checks. Any failure must be repaired only if attributable to this diagnostic change. No protected build, promotion, reload, installation, Wake-target mutation, T-0425 resume, or Git publication is authorized by this ticket.

## Expected next boundary

If T-0412R5 reaches `COMPLETED_VERIFIED` and its independent review is accepted, use that exact review record for one fresh protected reviewed-build attempt. The resulting `result.json` should include `missingLinkLibrary` when the failure remains LNK1104/LNK1181. Only then repair the specific product-derived toolchain path needed for that library; do not import ambient Visual Studio environment state.
