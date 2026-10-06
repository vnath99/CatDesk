# Current-source verification prerequisite provenance — 2026-09-25

Observed UTC: `2026-09-26T01:43:24.400553+00:00`. This report is a read-only investigation plus this new documentation artifact. No Rust source, live Wake control, session authority, credentials, browser profile, or installed package was changed by this subtask.

## Finding and scope

`cargo fmt --all -- --check` returned exit `1` and reported **61 files / 208 hunks**. The inherited formatting blocker is broader than the first reported example. Removing or formatting only that example would not resolve the gate. The inventory below is an exact current-source snapshot; a separate test-fixture repair may precede normalization and must retain its separate attribution.

## Provenance of the first reported file

`examples/local_mcp_binagotchy_retire.rs` is untracked, and `git log --all -- examples/local_mcp_binagotchy_retire.rs` returns no history. File creation and last-write timestamps both report **2026-09-21 19:39:35 UTC**. Its current SHA-256 is `d495321d5cdbd5c58581f39f641607fd9d83ec3f2acb7bd87f7d9caea7477ec6`.

The source is a one-off local MCP client hardcoding `catdesk_binagotchy_command` / `retire` for event `review-adc-t0396-r1-isolated-bootstrap-rebuild-20260921-6-independent_final_review`. It reads the inherited token and local route when executed. **It was inspected, never executed.** No matching task contract/plan ownership was found in the bounded search. Association with historical T-0396 diagnostic work is an inference from its embedded event ID and timestamp, not proof of its author or formal task ownership.

The independent-verifier follow-up in `T-0416_V5_HOST_LINKER_DIAGNOSTIC_RUNNER.md` lines 93–102 already names this example as pre-existing/out-of-scope formatting drift. That is direct durable evidence that the failure predates T-0424. T-0424's contract permits `wake/src`, `src`, and its review bundle, and does not permit the root examples directory. Its current plan explicitly says not to edit the example under T-0424 merely to make that task green.

## T-0424 versus inherited drift

Seven reported hunks concern the T-0424 timer additions:

- `wake/src/store.rs`: original check line anchors 565, 621, 1531, 1610 — manual timer validation and its tests (4 hunks).
- `src/mcp.rs`: original anchors 3328, 9149, 9184 — timer error arm and timer catalog tests (3 hunks).

These remain attributable to T-0424 and can be described as formatting its own additions. The other MCP hunks (original anchors 3368, 9356, 9387) concern inherited Binagotchy/target-rollover code. All 23 `wake/src/runtime.rs` formatting hunks are in inherited code; T-0424's public visibility change to the timer projection is not one of those hunks. The root examples, Wake diagnostic examples, reviewed-build implementation/tests, recovery/supervisor work, CLI status, autonomy fixture, and Python bridge harness are separate prior work.

The inventory groups are 2 root examples, 50 Wake examples, 5 root source files, 1 root recovery test, 2 Wake source files, and 1 Wake Python harness.

## Read-only safety validation

Each listed file was read, then independently rendered through `rustfmt --edition 2024 --emit stdout --config skip_children=true`. No formatted output was written to source. A small lexical scanner compared ordered normal/raw/byte/C string and character literals while skipping comments and retaining escapes; UTF-8 decoding and logical CRLF/LF normalization were explicit. All **11243 literals** in the 61-file snapshot were preserved. Each source SHA-256 was rechecked at the end to detect concurrent changes. The comparison passed with no source drift during this measurement.

This is not a full AST or proof of semantic equivalence. Rustfmt legitimately changes import order, optional punctuation, expression braces, and whitespace. No `line!`, `column!`, `stringify!`, `macro_rules!`, or `rustfmt::skip` occurrence was found in the investigated drift scope. However, `include_str!` tests inspect source text in `reviewed_build.rs`, `supervisor_lifecycle.rs`, `autonomy_runtime.rs`, and `binagotchy_cli.rs`. Formatting can affect those source-as-data checks. In particular, the parent independently identified the stale fixed-6000-byte reviewed-build source-slice test; its repair is a separate semantic/test change, not formatting provenance.

## Recommended resolution

1. Finish and independently verify the separately attributed stale source-slice test repair.
2. Treat repository normalization as a distinct verification prerequisite with this explicit file inventory, not as timer implementation or authorship of inherited code. Preserve the dirty worktree and all untracked artifacts. Do not delete examples, disable their Cargo discovery, or weaken CARGO_FMT to obtain a pass.
3. Immediately before normalization, capture source hashes again and compare with this manifest; explain any intentional test-repair difference. Render only these source paths with the repository toolchain, retain a before/after manifest, and verify the source deltas match formatter output. Recheck ordered literal preservation and review non-whitespace token changes rather than claiming exact-token equality when rustfmt adds optional punctuation or reorders imports.
4. Run the same full formatter gate and full tests/strict Clippy required by the authoritative session. Source-as-data tests must execute. Update the review bundle to separate timer behavior, the test-fixture repair, and this normalization prerequisite.
5. Do not fabricate a verified T-0424 result or use this report as install/promotion authority. Parent owns the permitted session transition and subsequent formal finalization.

## Exact pre-normalization manifest

The SHA-256 values cover original source bytes, including original line endings. They are **before hashes**, not installed package hashes or acceptance evidence.

| File | Fmt hunks | Before SHA-256 | Preserved literals |
| --- | ---: | --- | ---: |
| `examples/local_mcp_binagotchy_retire.rs` | 2 | `d495321d5cdbd5c58581f39f641607fd9d83ec3f2acb7bd87f7d9caea7477ec6` | 32 |
| `examples/local_mcp_reviewed_build.rs` | 5 | `b562d909e2f9d27ba48a6a787a12fe1d42e743aefb958d2f657b3dd64788d76f` | 39 |
| `src/binagotchy_cli.rs` | 1 | `67dc867114f814b9804dfd8bf0ab554c24a42d20a2a746734b972de806702094` | 195 |
| `src/delegated/autonomy_runtime.rs` | 1 | `53044ca0b963937ec829437d55e31d9266251b0b486591f83b4abddd2f915e60` | 1218 |
| `src/mcp.rs` | 6 | `a2466ea151d8dad505c8ed0ed13dc06b6ec3e03befce611390d993d33fb6e327` | 5419 |
| `src/reviewed_build.rs` | 23 | `77e48bf287ecd7e1091c343f6b112fc0188004b5cfb81ad0bc25be92b3a790cd` | 1868 |
| `src/supervisor_lifecycle.rs` | 1 | `642f06d62cfaa73a6f6e70380adf7fa7832a13b173405bb1e7228ed27a71ad80` | 183 |
| `tests/recovery_powershell.rs` | 1 | `e6f44285b767ae06721be8a52d4e9803d9f925bdbd2dae02f4f8049ea5d0163e` | 158 |
| `wake/examples/_tmp_queue_probe.rs` | 4 | `ade7409bad747236d61ab9c966448a49df9dd685abdb460c5ff2c8d00aa4ed70` | 18 |
| `wake/examples/adapter_main_exec_log_probe.rs` | 7 | `c53f40f9acb3e5769312a00c1d75d01b39e9147bdccb43c40925509915a38ca8` | 41 |
| `wake/examples/adapter_main_exec_probe.rs` | 6 | `9af52a8464ffa9fc7276bda0a4083ef0371aaf6430cfa48661044e5750ab7816` | 39 |
| `wake/examples/adapter_protocol_readiness_probe.rs` | 4 | `def2e14a7c7bcd0c1b76a62cd1d10b48539a700d13c459f6b1599ce474c8a556` | 27 |
| `wake/examples/adapter_protocol_readiness_probe_zero.rs` | 4 | `584e507fd3cb83331ad79d7d78e6cc5181ce61e292a19df40d2161c57b528f9a` | 27 |
| `wake/examples/adapter_script_probe.rs` | 7 | `72605af5437ee78daf4d80631be811144bd42b5b0eb246f87caa0a5a42ee9a78` | 54 |
| `wake/examples/cargo_cmdline_probe.rs` | 2 | `7dec9a4b9a23e11ebeeb663afa6f3a1d5a6fd44be6de4568af116fc70c5a629d` | 8 |
| `wake/examples/cdp_open_probe.rs` | 4 | `7083ba47d90fc7f0d3b2682c8a82756be40602eb682bed5a2ee0ed5c70226480` | 13 |
| `wake/examples/cdp_presence_probe.rs` | 3 | `30ccd768eab9473f599419a162d98ae8963c7deba1d5c35807e59dbff6dd69d8` | 13 |
| `wake/examples/cdp_reload_reopen_probe.rs` | 4 | `862c015e9be07b565dbe3a57bd0454d8bfff847e5ee4149fd8ea9c55b5f2a286` | 13 |
| `wake/examples/cdp_reopen_probe.rs` | 4 | `282d17e7e4b57413257c3761aab88684db6564ff18bfa1af0250510feb873838` | 13 |
| `wake/examples/claim_gate_probe.rs` | 1 | `fdf7033b6ef9cfa159261f187e3d927649efe875ffedb52728ea20dbc41f7fc4` | 8 |
| `wake/examples/descendants_probe.rs` | 2 | `59f0b822d902858570c20ae520e1840029c18d5c2a8284991b1219abe8a0db34` | 4 |
| `wake/examples/dev19_materialization_inventory.rs` | 2 | `d630e0368996d76907a8589dd43b1cb247756eee3a2f49d0af51a16a0be28af8` | 13 |
| `wake/examples/diagnose_canonical_release.rs` | 2 | `a2ab806c0a4fa4c8337572dbb03d5ea9a67c2cfa30baddf6a519a6244177ac3b` | 17 |
| `wake/examples/direct_cdp_protocol_probe.rs` | 5 | `dc8abcf3461cc9b8f195b80016e086d7e64a5f0170120f38bba46b96a89653a9` | 27 |
| `wake/examples/env_diff_link_probe.rs` | 1 | `1b707f948c9e018a7ff80307aaf47a3adbf17a0331df7a72b237ceea2211ebc5` | 23 |
| `wake/examples/exact_adapter_hidden_probe.rs` | 4 | `0a77d923a300c55ce19cfd4dcee8badbf4169777c87829120e40ee6063ce1d0e` | 19 |
| `wake/examples/exact_adapter_readiness_probe.rs` | 4 | `af164c3c9ec56985c207d30855d3352724e729c9b3a6c2fc6a36554c145e617f` | 20 |
| `wake/examples/extract_t0395_failures.rs` | 1 | `af3a1b9651b821fcdd50bffe3408994b36a998c68f72059ba22532757359c48c` | 17 |
| `wake/examples/extract_t0395_verification.rs` | 1 | `e2cf458108a8955493a68ece1d26bd0c16fd057b00496caa0b465af316c56497` | 12 |
| `wake/examples/hash_catdesk_release.rs` | 1 | `e99f127dd6c84a7eb947c8cbe8ba2965c6ccf39a32855d8c84ae67134fcef564` | 7 |
| `wake/examples/hash_file.rs` | 1 | `f1cb24782f53e74b377e9fa979755b47330c6942a8e1a639b678b11a8732d58c` | 5 |
| `wake/examples/inspect_navigation.rs` | 5 | `072dfa7222249d59cf557765e423aa1e9d3eabee0b5cc6bbab902cebbb992eb8` | 16 |
| `wake/examples/install_lock_probe.rs` | 2 | `44dcf7978d753708580b11dab8959a43f8c655671c27787a158217ff23c15be9` | 5 |
| `wake/examples/linker_location_probe.rs` | 1 | `05bca3c9c0a98effaa513b5c6f8ee234b48132bc2bf7394f5cb5c7879836dcb3` | 10 |
| `wake/examples/list_recent_logs.rs` | 1 | `a5d21d1c2b90af0c8012eeeb0d88077e46f7400a75b8b43b36676087c479aafa` | 3 |
| `wake/examples/live_browser_profile_args.rs` | 2 | `38a36fc8efc8be4c0158496de0c5ee1f56e55629a6e9537e143f40b16067ce11` | 15 |
| `wake/examples/measure_bootstrap_candidate.rs` | 1 | `4351997d184e06767437558f27a358ccbf07196669268c858cdaf8ad57c8ca22` | 7 |
| `wake/examples/msvc_env_probe.rs` | 3 | `54f69fb3ee64a2e57680b08f62a0bd3970bd36444477fa1f3a56729ad10b7c90` | 20 |
| `wake/examples/network_recovery_probe.rs` | 1 | `206924a4483758df81b397ab5428672877fc4e26ed72e1faa6138db0e1bd2dfe` | 24 |
| `wake/examples/process_activity_probe.rs` | 2 | `4cf535736af10324d0b352750045bf229f5abc2098b0d2057062408249ffec78` | 4 |
| `wake/examples/process_chain_probe.rs` | 2 | `48dde70b7d78b7b5a85b433ba998c8695deca74f93597d77f2a20a67be722ab5` | 4 |
| `wake/examples/process_cpu_sample.rs` | 1 | `c3b8aaaefab64fb00ef8979917b5345d3d829d9dff3b89d1066e6ffbf3e3beca` | 7 |
| `wake/examples/process_descendants.rs` | 4 | `b44ffe2ae8d1cd134cde43f0ca84cf582412b3088a339edd0c5be4769441c332` | 13 |
| `wake/examples/process_probe.rs` | 2 | `b30af99fd1475e82ea10f101631535b15085dacf1fba82c2a3685e238e007903` | 8 |
| `wake/examples/production_wait_probe.rs` | 5 | `27658a96f07481e51ae9a48c9af62706d43434005e664378330921e03cc311a1` | 17 |
| `wake/examples/protected_link_env_probe.rs` | 1 | `8475069a6ed7c1d9470b74750373e5da3992db8ab4a69f527212cb53e27aa646` | 22 |
| `wake/examples/publish_current_reviewed_package.rs` | 3 | `d05b7aeade1626fedfe48130946a2480248514c10db4d13facb75403b8ba8ad2` | 38 |
| `wake/examples/python_bridge_selected_runner.rs` | 3 | `77b0d3a7074683ca1e447b9c1cf82782a3a71992217a923130b35d8ecd021f89` | 17 |
| `wake/examples/queue_readback.rs` | 3 | `90c67f33912b3547e9015a1426fec9eccd42c7ec7c418020e986a7dc6e029c42` | 10 |
| `wake/examples/reviewed_candidate_inventory.rs` | 7 | `15dbf4f531a46342d2d1c275326f7a5613f298c0f2edf95a1931c5fb881c3969` | 33 |
| `wake/examples/selenium_source_probe.rs` | 2 | `731c1b21e6cc120546392151e9048104f5bf63f79a241f4d97ac7c41d8bbb4dd` | 11 |
| `wake/examples/stage_watch.rs` | 1 | `c06477a70bc79a508e09a914de6b99964b6a996c3194071043c7bcb63b9ef921` | 3 |
| `wake/examples/terminate_duplicate_v4_build.rs` | 3 | `4c0c687cbe1114e706d04b58967600e35aa6358fbe29936ceaf34520f4d342b8` | 11 |
| `wake/examples/terminate_exact_stale_installer.rs` | 1 | `8efabd4aca2f15ede03ee16b806136336d972545c2c0ab59029837d2e7c1b708` | 23 |
| `wake/examples/uc_route_probe.rs` | 1 | `24292e2c4fc3e770536b550a2bb2da9e88d1f16fe85e5e2a7538cd299a72eb46` | 14 |
| `wake/examples/uc_then_cdp_probe.rs` | 4 | `567f11254e0ece884246ae251af3c2e3bf4a1c5e8c8a1ada1d1559ce14b71b8a` | 13 |
| `wake/examples/uc_then_cdp_url_probe.rs` | 4 | `249070f54860192e5bc1ba660ac638d58747c2b0cec3ec115baf897b0b8ad68f` | 13 |
| `wake/examples/vs_env_probe.rs` | 1 | `a131ed9b31941fd0265129a7d34a8dd0517dbe4c0868b8789c8b39fb959b9ecb` | 13 |
| `wake/src/runtime.rs` | 23 | `c237a3dca05aa9afe749b41ec3548acb17a5740d7cb4c6878963d7d7214c234d` | 945 |
| `wake/src/store.rs` | 4 | `405a671513b704cd554726f49275f1e42d6c57e686110745fcfbc6764737510f` | 300 |
| `wake/tests/python_bridge_validation.rs` | 1 | `e1d61ec9ac239f2a6f9071f1e32cb9d33cc99e55ee3a445d98ed690aed3a7269` | 74 |

## Parent resolution: exact contract gate, eight files only

The contract executes `cargo fmt --check`, not the broader `cargo fmt --all -- --check` used by the initial inventory. A fresh exact-gate capture reported eight files. On 2026-09-26 UTC the parent ran `cargo fmt` for the root package only; the exact `cargo fmt --check` gate then passed. All 53 additional Wake paths from the broad audit remain byte-identical to the manifest above. This avoids broadening normalization beyond the actual prerequisite.

This normalization is separately attributed current-source maintenance under the operator's Wake-maturity goal, not new timer functionality or authorship of inherited examples. No example was executed. Baselines and the exact normalization patch are preserved in `.catdesk/logs/codex-verification-prerequisite-20260925/`. Each of the eight after-files was independently confirmed equal to rustfmt output from its saved before-file (logical newline normalization only). The reviewed-build before-file already contains the separately described stale-test repair, so that repair is not hidden in the formatting patch.

The concrete full-suite failure was `reviewed_build::tests::offline_worker_policy_never_inherits_ambient_cargo_home`: its fixed 6000-byte source slice missed `.env_clear()` after environment setup moved into `configure_exact_worker_cargo_command`. The repair bounds the worker and helper by their real markers, checks the isolated Cargo-home handoff, and behaviorally inspects a command configured with an ambient sentinel/Cargo-home to prove they are cleared/replaced. No production semantics changed. The original failing run recorded 960 passed / 1 failed / 22 ignored; this is a stale test, not evidence of cross-test concurrency failure. Focused repaired/helper tests passed.

| Normalized file | Before SHA-256 (after test repair) | After SHA-256 |
| --- | --- | --- |
| `examples/local_mcp_binagotchy_retire.rs` | `d495321d5cdbd5c58581f39f641607fd9d83ec3f2acb7bd87f7d9caea7477ec6` | `e32ff8cfe4859fc7569eb90204f9bcce3432b944a1f9fa0beb00703112e8e37a` |
| `examples/local_mcp_reviewed_build.rs` | `b562d909e2f9d27ba48a6a787a12fe1d42e743aefb958d2f657b3dd64788d76f` | `f1c995b014f55a46d7c1c53ae3a95686023b42de3cc5eaee22c9fb773ae413f6` |
| `src/binagotchy_cli.rs` | `67dc867114f814b9804dfd8bf0ab554c24a42d20a2a746734b972de806702094` | `f945daa1bc0921c53225a70c9c24359280dba046dff3cf746cb3030f87b1256e` |
| `src/delegated/autonomy_runtime.rs` | `53044ca0b963937ec829437d55e31d9266251b0b486591f83b4abddd2f915e60` | `68453681243714c5c77e73ecf4c56e952bd9b464b3399f67113e2bee8e62ec00` |
| `src/mcp.rs` | `a2466ea151d8dad505c8ed0ed13dc06b6ec3e03befce611390d993d33fb6e327` | `68d36ce6488b8a7879e4a071f1142ab85f9c8715b62e5b1f1b47b43cfbf236c7` |
| `src/reviewed_build.rs` | `77e48bf287ecd7e1091c343f6b112fc0188004b5cfb81ad0bc25be92b3a790cd` | `c555cfa11e15e5681fffa75e342526cc091a3ab01ea6c16359613a9c7c2d8ac0` |
| `src/supervisor_lifecycle.rs` | `642f06d62cfaa73a6f6e70380adf7fa7832a13b173405bb1e7228ed27a71ad80` | `e6884bdf198e285c5dc5b0f4330fd928b57f9c2db55cdd5f7a58ccb6b381cfef` |
| `tests/recovery_powershell.rs` | `e6f44285b767ae06721be8a52d4e9803d9f925bdbd2dae02f4f8049ea5d0163e` | `10e333f098158342cd034e518a304e0954bd07c82aa8c9cf4d8d3b7b7ac31d05` |
