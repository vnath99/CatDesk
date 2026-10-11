# T-0477 R1 — Recover partial Wake-first Chat51 target binding; consolidate commands

Date: 2026-10-10 EDT; canonical Chat51 https://chatgpt.com/c/6acaae13-4b18-83e9-b6ca-3c5e00cf47a8.

## T0477 R2 — CI-discovered regression repair (2026-10-10; source-only)

- First source commit `ef7ce6b` Windows Actions run `38107639800` TERMINAL FAILURE on Rust job. Python Wake and independent WakeHost Rust jobs PASS. Rust formatting/Clippy/PowerShell helper PASS, 1022 of 1023 main binary tests PASS and 26 ignored. New `designated_chat_update_completes_exact_interrupted_wake_first_commit` test PASS; older `designated_chat_update_repairs_only_corrupt_digest_matching_independent_wake` test FAILED `ProtectedStateMismatch`.
- Precisely identified cause: new partial-repair branch had called the **fully validating** `store.load_registry()?` before the legacy digest-only repair. A registry with exactly one corrupted digest fails full validation, so the older approved narrow reconciliation method never executed.
- R2 repair: evaluate Wake-first partial completion **only if** the multi-project registry passes full `load_registry()`. If it does not, do *not* silently normalize or accept corruption: enter the PREEXISTING `reconcile_catdesk_digest_with_wake()` closed method, which is allowed to repair only one old CatDesk digest when old URL == independently witnessed Wake URL, the digest is syntactically 64hex but wrong, and the complete corrected registry validates. Stale caller/different URL/other field corruption still reject.
- The same failed CI also triggered independent known intermittent test-only `reviewed_build::tests::appcontainer_fixed_helper_writes_only_fixed_output_child`: Windows runner Win32 5 `Access is denied` during isolated lowbox child create-new. This is not evidence of a failed user-host signed image transaction, and it is NOT caused by the new target-binding fix. Preserve visibility; do not hide/ignore tests or attribute it to product activation without evidence.
- Local source `cargo fmt --all`, strict workspace Clippy and `git diff --check` PASS. Old installed MCP `run_command` refuses `cargo test ...` via `INVALID_ARGUMENT`; full regression must pass new GitHub Actions job before any live source-local recovery attempt. No protected registry or independent Wake state modified by this source edit.

## Operator symptom and independent readback

Operator source-local Binagotchy CLI printed `TARGET_PROTECTED_STATE_MISMATCH` for `target set` and `target` but showed independent WakeHost v1.0.0-dev.84, target Chat51 URL, **generation32**, STOPPED PID0, current queue0. No `Designated Chat URL updated` receipt. The assistant's read-only `autonomy_project_registry_read` reports the CatDesk project still at old Chat48 URL and correct old digest `3a1d4cc69dfa3d05cb2bc33ef1197ac5a7433a120cfb4214a6996edb5a9d29e9`. This is an interrupted Wake-first **partial target advance**. Connected old serving `catdesk_transport_status` displayed a stale independent Wake snapshot Chat48 generation31; user source-local independent Wake status is the fresh later observation. Never treat older cached status as proof to revert target generation32. No browser event, WakeHost start or manual test is warranted before registry agreement.

Historical successful assistant-side setter from past Chat sessions is exact `autonomy_project_registry_bind` with `projectId=catdesk`, `decision=DESIGNATED_CHAT_TARGET_URL=<new>`, and a compare-and-swap digest of the **currently coherent** old pair. Assistant attempted it once this turn with expected old Chat48 digest and requested Chat51; the installed connector returned `INVALID_ARGUMENT` before any success receipt. No successful mutation through that path and no registry-only or Wake-only setter substituted.

## Source gap and scoped recovery implementation

Current-source `src/mcp.rs::operator_update_designated_chat_target` accepted exact guarded coherent-pair rollover and one old *same URL / damaged SHA* registry reconciliation, but rejected the distinct state where the independent Wake target had already advanced to exactly the desired canonical URL while the registry still contained a self-consistent old URL/hash. This is the observed failure of the previous operator CLI attempt; retrying the same T0476 CLI without repair would fail again.

Add a **single narrow completion branch** before same-URL SHA-only recovery:
1. Re-read the currently configured effective Wake target (the independently installed WakeHost when enabled). Require its normalized URL has exactly the caller requested URL and its computed digest equals caller CAS; a stale CAS fails.
2. Load and validate entire persistent multi-project registry. Require exactly one `projectId=catdesk` record at the canonical CatDesk workspace. Require its **old** registered URL is canonical and SHA matches that old URL. Reject missing/corrupt/different project state.
3. Use existing `bind_project_chat_target_after` under the registry registration CAS lock, passing the **actual stored old registry digest**, and recheck the independent Wake target remains exactly the requested URL before commit. Do not call Wake setter or increment generation again.
4. Require full final `designated_chat_target_readback_locked` equals requested URL+digest; otherwise fail closed. No independent Wake control state, profile/browser, external Secure MCP tunnel, installed Program Files exe or signing key touched.
5. Existing *same-URL but invalid SHA* repair path and ordinary coherent-pair CAS unchanged.

Regression fixtures in `src/mcp.rs` cover successful interrupted Wake-first completion with **wake config byte-for-byte unchanged**, and rejection of stale CAS, unrelated desired URL, and damaged old registry digest with both stores unchanged.

This source repair is NOT yet live in the installed old controller. An operator-run current-source local Binagotchy CLI can carry out the guarded completion after source verification/Windows CI; but by design ChatGPT should normally use the first-class paired CatDesk tool once the serving connector is current. Do not expect another user-driven registry change on every chat.

## Existing commands reference reorganized instead of duplicated

`docs/orchestrator/CATDESK_IMPORTANT_COMMANDS.md` already existed; added an authoritative `START HERE` quick-reference to the top with guarded paired target CAS, WakeHost controls, manual canary, 19m timer, nine-layer `catdesk.ps1 diagnose`/guarded recover, Git status/commit/push, `codex resume --all` then `/goal resume`, and source-vs-production distinctions. Operator PowerShell examples are **all one-line**. The existing dated sections remain historical. Documented that `autonomy_project_registry_chat_target_bind` `PREFLIGHT` may itself mutate registry and is NOT safe as a dry-run; a sole registry-only setter is not an acceptable alternative to the paired transaction without the above witnessed recovery.

## Verification and limitations

Local `cargo fmt --all`, `cargo fmt --check`, strict workspace all-features/all-targets Clippy, `git diff --check`, and `cargo build --locked --offline --bin catdesk` all returned exit0. Focused and full `cargo test` executions via currently installed CatDesk `run_command` returned **INVALID_ARGUMENT** (before producing test output) on this turn, despite earlier commands working in prior turns; do not claim those tests passed. GitHub CI after scoped source commit is required to establish tested status, including new regression tests, before any further operator invocation. No independent source reviewer approval yet; don't claim production serving deployment.

The old source-current installed main image is T0215 signed epoch1 with T0366 signed epoch2 pending; its legacy installer is separately failed. None of that is required for this narrow target-state recovery and no protected signed-main-image mutation occurred. Independent WakeHost must remain STOPPED until project and independent Wake targets are independently verified at Chat51 generation32+ and manual canary is separately authorized.

## Next steps

1. Verify new scoped commit on GitHub, complete Windows CI, independent review if production serving gate requires it.
2. After the local rebuilt CLI is approved, use the source-current guarded `target set` **only as a rare recovery fallback** to commit the pending registry side, leaving Wake at Chat51 generation32. Do not ask user to operate separate target setters or touch protected JSON.
3. Independently read both authoritative target stores and exact digest, require STOPPED and no current queue, then separately start/manual Wake diagnostic; manual ≠ natural acceptance.
4. Long-term: make the assistant's first-class paired tool available independently of signing/production rollout and add one *read-only direct* independent WakeHost target-generation observation not backed by old serving cached status. Continue T0477 isolated developer loop and T0478 simple versioned stable production release, without coupling to T0366 pending rotation.
