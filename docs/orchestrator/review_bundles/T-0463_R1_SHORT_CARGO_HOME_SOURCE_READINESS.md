# T-0463 R1 — Short isolated Cargo-home repair: source readiness / review request

**Date:** 2026-10-10 (America/New_York).  
**Current conversation:** CatDesk_chat50, `https://chatgpt.com/c/6aca50a3-0da0-83ea-8358-dbc128c4f9ad`.  
**Status:** **SOURCE_READY_FOR_INDEPENDENT_FINAL_REVIEW; NOT an approved CatDesk review record, BUILD_ATTESTED, promoted release, serving deployment or Wake acceptance.**

## Immutable source / verification evidence

- Existing private GitHub branch `orchestrator/chatgpt-codex-autonomous-loop`, source HEAD `3160cd43ca021c2b6b55c29ca61801b0cd9cfa3e`. Commits: `fa86754` (shorten isolated Cargo-home) and `3160cd4` (pin Cargo-home output guard lifetime). Both were already pushed; current tracked source is clean. Eight pre-existing untracked files are **excluded/preserved**.
- GitHub Actions push CI **run 38066384342**, for exact SHA `3160cd43ca021c2b6b55c29ca61801b0cd9cfa3e`, **completed SUCCESS**. Its three Windows jobs all completed SUCCESS: Rust (job `114254771650`), independent WakeHost Rust (job `114254771574`), and full Python Wake/advisor (job `114254771652`). Rust job steps separately show Format, Clippy, isolated PowerShell shell/cwd fixture, and remaining Rust suite all SUCCESS. [CI run](https://github.com/vnath99/CatDesk/actions/runs/38066384342).
- A direct local broad `cargo test --locked --offline` request from a prior turn returned a connector TIMEOUT with unknown child-process state. **Do not claim this direct call passed or failed.** The independent Windows Actions job does prove the configured CI Rust suite succeeded for HEAD. `tasklist`/process inspection through the command gateway was rejected `INVALID_ARGUMENT`; do not bypass its allowlist.

## Grounded failure and repair rationale

- Exact T-0462 old protected build is terminal `BUILD_FAILED_OR_AMBIGUOUS`. The separately manually gated diagnostic found Cargo exit 101 with 4 KiB truncated stderr; a guarded disposable reproduction identified `ring 0.17.14`, MSVC `C1083`, cannot open include file `prefix_symbols.h`. The same disposable package has that header and parent directory, with a measured 264-UTF-16-unit absolute filename; root-cause hypothesis is legacy MSVC/Win32 path-length pressure. This does **not** prove that the old protected attempt's entire stderr matched the disposable replay, or that the new production repair works yet.
- `src/reviewed_build.rs::build_attempt_output_root_guard` preserves the legacy control path, but uses the exact verified attempt id and no-follow pinned `target-verify/rb/<attempt>` root for active reviewed generations. Mismatching attempt ids fail closed with `REVIEWED_BUILD_EVIDENCE_DRIFTED`.
- `seed_isolated_cargo_home` now seeds fresh dependency closure under the short attempt root, using existing hash-checked locked Cargo archives, fixed `--offline`, and the closed worker Cargo environment; the new returned `ProtectedDirectoryGuard` remains alive for worker Cargo execution. Neither scratch source/cache nor target directory grants a reviewed-control generation, immutable attestation or promotion authority.
- Existing targeted tests `active_generation_target_layout_uses_short_attempt_bound_workspace_root` and `offline_worker_policy_never_inherits_ambient_cargo_home` passed; previous focused reviewed-build run passed 97 tests, 0 failed, 8 ignored. The ignored exact-T0462 short-root A/B test was rejected at the command gateway; **do not assert experimental short-path success**.

## Review scope and explicit gates

The above was independently **inspected as source evidence in the current ChatGPT control turn**, but no CatDesk `COMPLETED_VERIFIED` review session/record tied to current HEAD has been produced. A read-only `autonomy_contract_create` proposal for T-0463 was attempted but returned `INVALID_ARGUMENT`; **no contract or worker was started**. Do not synthesize a review record or recycle the older T-0462 record: that older source review predates this repair.

Obtain a real fresh controller independent final-review record tied to the current source snapshot/HEAD, then use the supported guarded `catdesk_reviewed_build` PREPARE/CONFIRM/RESULT only after verifying exact review authority. Require `BUILD_ATTESTED` and an exact attestation before reviewed promotion and serving reload. Run live nine-layer `diagnose` and bounded `recover` acceptance. Only after serving parity, perform the guarded project-registry / WakeHost **paired** Chat50 URL/SHA target migration (desired digest `625ddb77fa11ea42663806c7e9e436c81a6a01368e1d14f6f68c8a7ac456d0fc`, generation >=32), then resume and test one fresh manual Wake and separate natural review wake.

The old installed WakeHost dev.84 remains deliberately **STOPPED** with old Chat48 generation31 and mismatched registry digest. No old Binagotchy CLI launch, Wake test, protected file edits, external secure tunnel mutation, installation or release action was performed.
