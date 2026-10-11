# T0478 R3B4-B1 — Fixed read-only candidate Git-object reader

2026-10-11, canonical CatDesk Chat52.

## Scope and authorization limits

This change introduces `src/isolated_fixture_git_objects.rs` as a **dormant source-only** candidate Git-object reader. It does NOT copy objects into a fixture, write manifests, bind source authority into MCP, change production or verified GitHub review state, or enable a coding agent. It must not be mistaken for a trusted reviewed-source producer until an external, exact GitHub branch/commit/CI witness, bounded trusted snapshot copy, and runtime manifest enforcement exist. Only the two exact source paths `Cargo.toml` and `src/main.rs` are permitted.

- Reuses `delegated::github_bootstrap::resolve_trusted_git_identity` to select a canonical fixed-slot Git executable rather than ambient PATH and rechecks its identity before every operation. No arbitrary executable/path is accepted via MCP.
- Executes only `git --no-replace-objects cat-file -t|-s|blob <lowercase-40hex>:<fixed-path>` through `tokio::process::Command`, *not a shell*. Suppresses stdin, stderr and lazy object network fetching, marks process kill-on-drop, 12-second overall timeout per request, and bounds stdout to 32 bytes for type/size or 512KiB for content with a one-extra-byte overflow check. The module never calls `git fetch`, `checkout`, `add`, `commit`, or `push`.
- Requires type `blob`, parsable bounded decimal size, exact byte count, valid UTF-8 without NUL; then passes candidate bytes through the existing fixed-scope `assemble_candidate_manifest`. No return value is labeled as reviewed/approved source evidence.
- Adds targeted tests for exact argv sequences, commit/path denials, wrong Git object kinds, size mismatches/invalid sizes, oversize and binary content. These tests are not allowed to call a real Git executable or modify the checkout.
- Source code is **not invoked** by normal CatDesk or the read/search fixture; it remains `#[allow(dead_code)]` in the module index. No new global file access exists.

## Verification status

- Local `cargo fmt --all` and `cargo fmt --check` PASS.
- Local `cargo check --locked --offline --bin catdesk`, strict `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings`, and `cargo check --locked --offline --all-targets` PASS (test code compiles).
- Focused `cargo test ... isolated_fixture_git_objects` and full `cargo test --locked --offline --workspace` returned **INVALID_ARGUMENT from the serving CatDesk MCP gateway**, so **no local test-execution success is claimed** for B1. The previous R3B4-A2 source commit `37a55d6` independently passed GitHub Windows CI `38161909658` 3/3.
- New GitHub Windows CI required to prove B1 tests, full suite, fmt/Clippy and separate Python/WakeHost jobs before accepting. This is NOT deployment.

## Independent CI regression and scoped repair (Chat52)

- Initial source commit `51919e6b900a21a835a95558c1504400f15b17e5` failed GitHub Windows Actions run `38163372088` in the main Rust suite: `isolated_fixture_git_objects::tests::fixed_commands_only_and_no_caller_source_path` asserted that `"a".repeat(40)` was an invalid commit. Forty lowercase `a` characters are valid *hexadecimal syntax* and a Git command should attempt a read and fail only if the object is absent. Test assertion was invalid, not the production fixed-path validator. Corrected test to forty `g` characters (nonhex), retaining the fail-closed test's real purpose.
- Local `cargo fmt --check`, focused B1 tests **2/2 PASS**, strict all-target Clippy PASS, and full `cargo test --locked --offline --bin catdesk` exit0 after the correction. Source repository `git --no-replace-objects cat-file -t HEAD:Cargo.toml` explicitly returned `blob` and exit0 in normal workspace; does not prove approved GitHub review.
- The same CI job also ran a separately isolated protected-build `appcontainer_fixed_helper_writes_only_fixed_output_child` test that failed its one-off Windows probe; this is **not** the named main Rust suite's failing test. Do not modify protected AppContainer/signing machinery to fix the unrelated candidate Git test. New Windows CI for the small correction is required before marking B1 accepted.

## Next safety work

R3B4-B2 requires an independently verified exact GitHub branch/commit/CI proof bound to a fixed local workspace, and a read-only runner-test harness with fake responses for timeouts, Git identity drift and wrong object paths. R3B4-B3 requires temp-root creation and handle-bound byte copying through audited Windows ProtectedDirectoryGuard/write_new_regular_in; then independent re-read/digest validation. R3B4-C must bind verified immutable manifest in process-owned state before serving only allowed leaf data. B4-D owner-bound PID/port lifetime and restart semantics. Protect signed pending T0366, ordinary production/independent Wake and externally owned Secure MCP tunnel; preserve seven older untracked items.
