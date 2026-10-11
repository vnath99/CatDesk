# T0478 R3B4-B2A — Injectable fixed Git-object retrieval test seam

Date: 2026-10-11. Canonical conversation Chat52 https://chatgpt.com/c/6acba3bf-e740-8331-8896-8a25fefdbf16.

## Distinguish GitHub mirror, development worker and production recovery

GitHub mirroring/publication and connector access are operationally complete. This ticket only evolves T0478's **isolated developer fixture**, not the independent WakeHost, signed production versioned worker or stable supervisor. Production Recovery remains incomplete: `SUPERVISOR_ROOT_UNAVAILABLE`, `SUPERVISOR_STARTUP_POLICY_UNPROVEN`, `SUPERVISOR_STARTUP_DEFINITION_READ_FAILED` and `SUPERVISOR_3201_NOT_PROBED`. Do not conflate source-only fixture acceptance with startup/recovery acceptance.

## Source change

- Extracted `collect_candidate_from_runner`, a small async generic phase-result runner within dormant `src/isolated_fixture_git_objects.rs`. System runner still obtains a fixed-slot, fingerprinted Git executable through `resolve_trusted_git_identity` and invokes only bounded `cat-file -t/-s/blob` on exactly two source paths and exact commit syntax.
- Fake deterministic output tests now prove ordered `Cargo.toml` then `src/main.rs` operations, Type/Size/Bytes phase and exact byte limits, canonical candidate manifest assembly, and **early fail-closed** rejection for a non-blob object before any more Git requests.
- This does not read actual working-tree contents for source; does not touch protected production state; does not add new MCP permissions; does not trust an unsigned user-supplied manifest, local HEAD or arbitrary JSON GitHub proof.
- Independent review of GitHub branch+commit+CI evidence and a trusted Git-object byte-source producer are still pending R3B4-B2/B3. A caller supplying a 40-hex commit string is **not** equivalent to a reviewed GitHub commit.

## Corrected prior CI regression

Original Git reader source commit `51919e6` CI `38163372088` FAILED one main Rust test, caused by a faulty test assertion that all-letter lowercase hex string `"a".repeat(40)` is nonhex; fixed to `"g".repeat(40)` and published scoped correction commit `47e14ef6afae96bbba1bf6fd42b78f4f96d900d3`. Local focused tests, strict Clippy and full main Rust suite then passed. The follow-up correction GitHub run `38170517509` must be separately verified terminal (3/3) before marking B1 accepted. Separately logged historic isolated AppContainer helper failure is unrelated and remains in its own protected-build lane, not a justification to change signer/recovery.

## Local verification and CI gates

`cargo fmt --all` PASS, `cargo fmt --check` PASS, `cargo test --locked --offline --bin catdesk isolated_fixture_git_objects` **4 passed, 0 failed**, full `cargo test --locked --offline --bin catdesk` exit0, `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings` PASS. Test-subprocess fixture auth/HTTP semantics already accepted in earlier R3B3. Require new Windows CI Rust + Python Wake/advisor + independent WakeHost Rust to pass for the next committed source increment. These are source/test checks, NOT actual GitHub review authority or full production acceptance.

## Next

1. Require independent, exact GitHub connector readback for expected remote branch, SHA and CI three-job success, use an opaque non-forgeable reviewed approval/attestation boundary rather than trusting caller-provided JSON.
2. Implement immutable Git-object copy from exact trusted commit into fresh bounded temp fixture root using existing Windows protected handle-rooted creation; remeasure bytes and provenance.
3. Hold trusted manifest in process, enforce read/search against expected SHA-256, refuse injection/tamper/reparse/hardlink; prove with real Windows subprocess and negative tests.
4. Add owner-bound ephemeral PID/port launch/stop/restart with no production or official external Secure MCP duplicate.
