# T-0483 R25 — pre-spawn prepared Sol run and retained executable lease (source only)

## Scope

`src/delegated/sol_worker_lifecycle.rs` adds a private `PinnedSolWorkerExecutableV1` and `PreparedSuspendedSolJobChild`. The host-prepared task/turn/selected nonce, ledger and empty output root are consumed before constructing the suspended child command. An internally constructed executable path comes from a retained no-follow, digest-checked regular-file handle and pinned parent directory; a new write/delete open is denied while retained. A bounded native-path readback on the exact suspended child is compared case-sensitively before any future fixed-account bind or resume. The executable lease is carried through the bound and drained Job types. `src/windows_protected_fs.rs` provides the retained artifact's bounded native-path getter.

## Evidence and limits

- Focused lifecycle tests: 15 passed, 1 diagnostic helper ignored. Negative tests cover bad digest/component, reserved or stream names, writer/rename exclusion, wrong image path, case-twin path and same-user fixed-account rejection. Independent read-only review found and fixed the case-twin comparison issue; it also identified the bounded cleanup ownership limit below. Final combined gates passed: root all-features Rust workspace, independent Wake all-features Rust workspace, full Python `pytest.exe -q --tb=short`, strict root/Wake all-target/all-feature Clippy, both format checks, root all-bin build and diff check.
- A caller-supplied `approved_sha256` is **not** an independent policy approval. No trusted binary digest source or fixed-account launcher exists yet. The current private `std::process::Command` path launches under the host user; its same-user diagnostic cannot mint the fixed-worker binding. `QueryFullProcessImageNameW` returns a pathname, not mapped image-section proof. DLL/config/environment, inherited handles, outside-Job descendants and ancestor ACL/lost-write authority remain unproven.
- If bounded failure cleanup cannot prove child exit, an error can release the executable lease without a durable attention owner. This dormant path therefore grants no burn, stage or repository-use authority and cannot be accepted as a live worker route. No actual Sol/medium task→review→Wake cycle occurred.

## Next gate

Add a policy-pinned approved executable digest and a fixed-account launcher that retains exact process/Job/executable ownership across ambiguous failures. Prove child and descendant loss of write authority, inherited-handle and ancestor exclusion, and durable nonce-burn-to-stage recovery before connecting repository use.
