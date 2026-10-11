# T0478 R3B4-B3A — Protected disposable source snapshot staging

**Date:** 2026-10-11. **Canonical Chat:** https://chatgpt.com/c/6acba3bf-e740-8331-8896-8a25fefdbf16.

## Verified predecessor

B2 source commit `124adc5719d2755af51bd61084746400422e2a86` independently passed GitHub Actions Windows run `38172447911` with Rust, Python Wake/advisor, and independent WakeHost Rust all **SUCCESS**. The existing GitHub mirror is complete. The stable production supervisor Recovery remains incomplete (root unavailable, startup definition unreadable, startup policy unproven, port3201 not probed). Neither source-only work nor GitHub CI implies production promotion.

## B3A source change

- `src/isolated_fixture_snapshot.rs` is a dormant module registered in `src/main.rs` with no exposed MCP endpoint, process launcher, ordinary workspace write permission, or production/release permission.
- For `stage_witnessed_candidate_snapshot`, the source commit must pass the **fixed live HTTPS GitHub branch/CI witness** through `fetch_live_witness`. Only then can the existing fixed candidate Git-object reader supply two Git-tracked object byte buffers `Cargo.toml` and `src/main.rs`. The source commit of the manifest must match the opaque witness. No mutable working-tree file contents are copied.
- The source-only staging path acquires a handle-pinned, no-reparse guard on canonical OS temp, creates a **new** random direct-child `catdesk-dev-fixture-<uuid-simple>` via `ProtectedDirectoryGuard::create_child`, and retains the pinned ancestor chain. Existing paths are not adopted.
- `stage_candidate_into_owned_root` first checks that a supplied pinned root is exactly a canonical OS-temp direct-child with the generated fixture name, validates the full manifest and candidate set **in memory before any writes**, requires every expected source path exactly once and all SHA/size checks, then writes only flat manifest entries with existing protected `write_new_regular_in`. Existing target paths are never overwritten.
- Each new file is re-opened using existing `read_relative_regular_unique` (one-link, no-follow, 512KiB, handle-based parent) and independently remeasured with `verify_file_bytes` prior to declaring staged bytes valid. Root pin/identity is rechecked around writes.
- A `StagedCandidateSnapshot` retains its pinned `ProtectedDirectoryGuard` and manifest in memory. This is **not** a signed manifest, a general filesystem sandbox, a runtime service, an MCP trust grant or a production artifact.

## Tests and proof

- Three focused Windows tests: a properly named generated temp fixture root accepts one expected in-memory candidate blob and rereads matching bytes; duplicate attempt refuses overwrite; tampered candidate/extra unlisted candidate refuses; arbitrary OS-temp root or unapproved child name refused; a pre-existing hardlink at the expected fixture leaf refuses overwrite and the separately owned external test file remains unchanged.
- Tests create and delete **only their own generated OS-temp test directories**. After dropping directory guards, cleanup uses the test's exact owned paths. No live CatDesk process or production root is touched.
- Local `cargo fmt --check` **PASS**, strict all-target/features `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings` **PASS**, focused `cargo test --locked --offline --bin catdesk isolated_fixture_snapshot` **3/3 PASS**, full `cargo test --locked --offline --bin catdesk` **PASS**. Independent GitHub Windows CI **required** after feature commit; do not claim B3A independently accepted until terminal all-three-job success.

## Open risks — do not activate yet

- There is **not yet a reviewed owner-controlled cleanup/retirement** for a partially staged directory. A failure after creating one file can leave a generated, inactive temp directory. The B3A routine explicitly leaves only that generated owned root; it must not silently call unsafe pathname cleanup on a path another actor may have substituted. Add owner-bound, handle-identity-checked bounded cleanup before activating a real launcher or worker. Test-only cleanup is limited to known test-owned paths.
- The generated fixture root is not yet proven to have an owner-only ACL at creation, and a hostile same-user process may still mutate ordinary content if it can obtain write sharing. An immutable GitHub-reviewed source snapshot **must** get runtime per-request SHA checks (R3B4-C) before serving any source.
- The live `fetch_live_witness` HTTPS verification and fresh root producer have not been invoked as an end-to-end live canary in this increment. The source path is dormant, with no user-facing runnable `catdesk dev run` command.
- B3A uses existing protected relative writers and readbacks but does not prove independent Windows CI until a new run completes. It is not an autonomous coding agent. B3B must close trusted owner/ACL/cleanup boundaries; C binds manifest to the isolated reader; D bounds own PID/port child lifetime. Then proceed T0479 stable production versioned worker and Recovery.

Official externally owned Secure MCP tunnel, independent production WakeHost, signed installed T0215 epoch1 and pending signed T0366 epoch2 are unchanged. Seven preexisting unrelated untracked files preserved.
