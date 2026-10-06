# T-0365 / T-0324 — Executable Source Eligibility Manifest

Date: 2026-09-09
Status: SOURCE ELIGIBILITY CONVERGED — eligible for immutable reviewed-source freeze/build; not yet deployment authority

## Purpose

This record narrows the current intentionally dirty CatDesk workspace into the exact source domain that ChatGPT designates as eligible to become the next T-0299+ reviewed main-image candidate. Eligibility here means the bytes may enter the already-reviewed immutable source-snapshot and reviewed-build pipeline. It does **not** make mutable workspace output, `target/release`, a generic verification build, or a caller-selected hash/path into deployment authority.

## Designated executable-source scope

The existing reviewed source-snapshot model is the correct freeze primitive. The designated source domain is exactly:

- `Cargo.toml`
- `Cargo.lock`
- recursive `src/**`
- `build.rs` only if a regular safe file exists when the immutable snapshot is created; no current `build.rs` appears in the workspace status inventory

Documentation, review bundles, `.catdesk/**`, logs, diagnostics, ad-hoc release directories, `target/**`, scripts, tests, Git metadata, and other workspace files are not selected as executable source authority by this manifest. A production source file that intentionally embeds another artifact would still have to satisfy the reviewed snapshot/build policy and its own reviewed source semantics; this record does not create a path-based exception.

The broad dirty-worktree state is intentionally preserved. No clean/reset/stage/commit operation is needed or authorized to make this source selection.

## Review-lineage reconciliation

The current compile domain contains both tracked modifications and substantial untracked `src/**` modules. Git HEAD alone is therefore not a meaningful release cutoff. ChatGPT reconciled the current source by implementation surface and accepted review lineage instead.

### Product-root, reviewed source/build, promotion, and recovery authority

The current `src/reviewed_build.rs`, `src/reviewed_source_snapshot.rs`, `src/daemon_reload.rs`, `src/operator_facade.rs`, `src/windows_protected_fs.rs`, `src/main.rs`, and associated integration surfaces are covered by the reviewed T-0154/T-016x through T-0217 build/provenance chain, the protected-filesystem extraction chain through T-0269, the later supervisor/recovery integration, and the independently accepted cumulative T-0319/T-0363 recovery boundary. T-0306/T-0307 explicitly re-audit the current reviewed-source/reviewed-build/main-image authority graph. The historical signed T-0215 epoch-1 and unsigned T-0217 epoch-2 payload are evidence only; neither is the new candidate.

`Cargo.toml` / `Cargo.lock` changes are attributable to accepted reviewed requirements: the Reqwest `stream` feature is recorded by the T-0223 stable-supervisor transport work, and `ed25519-dalek = "2"` plus its lockfile closure is explicitly reviewed by T-0215 product-root main-image authority.

### Version-independent supervisor / protected host control plane

`src/control_plane_supervisor.rs`, `src/windows_supervisor_control_pipe.rs`, `src/windows_supervisor_startup.rs`, `src/supervisor_lifecycle.rs`, and their shared protected-filesystem dependencies have review lineage through T-0223/T-0226..T-0283. The accepted source contains the fixed 3201/3200 supervisor architecture, principal/session-bound pipe admission, reviewed-image role binding, exact lifecycle grammar, and corrected native startup/rollback semantics. Host-live acceptance remains downstream; source eligibility is not a claim that T-0223 live acceptance has already occurred.

### Stable wake / current-target control surfaces

`src/stable_wake_adapter_runtime.rs`, `src/stable_wake_bootstrap.rs`, `src/stable_wake_core.rs`, `src/stable_wake_delivery.rs`, `src/stable_wake_owner.rs`, and `src/stable_wake_owner_mode.rs` have the bounded review chain T-0242 through the later T-029x wake/target work. Historical natural wake acceptance is already recorded separately. Current-chat paired target rebinding remains intentionally parked until this newer serving generation is active; inclusion of these source bytes does not authorize a browser submit or protected target edit.

### Native GUI and integrated acceptance surfaces

`src/windows_gui.rs`, `src/core_host_acceptance_preflight.rs`, `src/core_host_gate_approval.rs`, and `src/core_host_gate_evidence.rs` are attributable to the T-0139/T-0292..T-0312 implementation/review lineages. Literal deployed GUI/host acceptance remains downstream and is not manufactured by source inclusion.

### Provider/orchestration surfaces

The current delegated/autonomy source has review lineage across T-0030/T-0031/T-0035, Qwen compatibility T-0129/T-0138, task-output/cancellation/recovery work, reset-aware Codex/Qwen policy T-0282/T-0284, current-thread work T-0285, runtime-capability T-0316, and the current T-0324 continuation investigation. In particular:

- `src/delegated/contracts.rs` exact `.` workspace-root scope is explicitly covered by the T-0030 Phase-1 corrective review; `./src` remains rejected.
- `src/delegated/integrated.rs` contains the Qwen 3.8 post-tool non-authority user continuation whose focused current-source regression passed and whose absence from the old serving image is the T-0324 deployment-skew trigger.
- `src/delegated/autonomous_qwen_tools.rs`, `autonomy_accounting.rs`, `autonomy_observability.rs`, `autonomy_projects.rs`, `codex_app_server.rs`, `github_bootstrap.rs`, and `github_publication.rs` each have explicit earlier review-bundle attribution.

### Direct ChatGPT residual-diff review

A focused residual audit identified five tracked source files whose current diff was not usefully attributable from filename search alone:

- `src/command.rs`: production change is a closed `LifecycleFacadeOperation` vocabulary and exact root-relative `catdesk.ps1` interception. It accepts only fixed status/start/recover/stop/autostart shapes and rejects arbitrary paths, flags, quoting, command chaining, redirection, expansion, assignments, and extra tokens. ChatGPT reviewed this diff directly. `cargo test detect_lifecycle_facade_intercept -- --nocapture` passed 2/2 on 2026-09-09. This bounded surface is accepted into the candidate eligibility set.
- `src/delegated/contracts.rs`: exact `.` root-scope correction is already explicitly described and accepted in T-0030 Phase 1.
- `src/delegated/advisor.rs`: current diff is Windows-only test-ignore annotation; no production behavior change.
- `src/delegated/fault_injection.rs`: current diff is a Windows-only test-ignore annotation; no production behavior change.
- `src/delegated/job_manager.rs`: current diff is a Windows-only test-ignore annotation; no production behavior change.

No unresolved production hunk was identified in that residual group.

## Candidate designation decision

ChatGPT designates the **exact current eligible Cargo/source domain described above** as the source set to freeze for the next T-0299+ image. This is deliberately the whole current compile domain, not a cherry-pick of only a few headline features, because the present `main.rs` generation composes the reviewed build, recovery, supervisor, wake, GUI, provider, and acceptance modules together and the current source has already passed the repository verification gates recorded in the T-0360 governance record.

This designation is still one trust step before candidate bytes. The mutable files named above are not themselves a signed image. The next legitimate operation is to capture those exact bytes through the reviewed immutable source-snapshot authority and run the reviewed locked build from that committed snapshot. Only that resulting exact output may be measured and considered for a canonical signed envelope.

## Next trust steps

1. Create/validate the immutable reviewed source snapshot for the designated Cargo/source domain without cleaning or rewriting unrelated workspace state.
2. Run the reviewed locked release worker against that immutable snapshot using its bounded build timeout rather than the legacy MCP shell's 120-second ceiling.
3. Validate producer attestation and exact candidate identity; measure payload SHA-256 and length.
4. Determine the next monotonic main-image epoch and fixed bootstrap/rotation purpose from trusted accepted state, then write the canonical unsigned signing payload.
5. Perform only the unavoidable detached signing operation through a non-exporting product-root signer, verify the signature against the compiled public root, and consume through the fixed reviewed-main-image bootstrap/rotation boundary.
6. Prove serving-generation parity, reconcile T-0322, run the Qwen continuation canary, then continue T-0223, guarded current-chat wake rebinding, GUI acceptance, T-0152, and T-0155.

## Safety / non-actions

No raw reload, manual binary copy, mutable `target/release` blessing, direct promotion-script execution, protected-lock deletion, private-key access, signature fabrication, product-root replacement, browser wake, protected wake-target edit, Scheduler/service mutation, tunnel replacement, Git publication, or dirty-worktree cleanup was performed while producing this manifest.

T-0366 froze this designated set into an isolated non-reparse copy, proved all
90 copied files matched before and after a locked release build, and measured
an offline candidate. It did not create a signing payload because the current
protected accepted epoch and bootstrap-versus-rotation state remains
`UNOBSERVABLE_FROM_CURRENT_SAFE_SURFACE`; history alone cannot choose a safe
next epoch or purpose. See
`T-0366_T0324_OFFLINE_REVIEWED_IMAGE_CANDIDATE_MATERIALIZATION.md`.
