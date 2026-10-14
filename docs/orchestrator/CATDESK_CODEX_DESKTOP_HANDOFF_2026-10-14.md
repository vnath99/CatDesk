# CatDesk — Comprehensive handoff to Codex Desktop
**Date:** 2026-10-14  
**Authority:** Operator-requested transfer of day-to-day CatDesk implementation to Codex Desktop. This document is an engineering handoff, **not** authority to modify protected production installations or bypass security controls.  
**Source repository:** https://github.com/vnath99/CatDesk  
**Working branch:** `orchestrator/chatgpt-codex-autonomous-loop`  
**Verified source HEAD at handoff:** `bea190d66ff2f9e840ebffee832f6ce6354251b9`  
**Local Windows repo:** `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop`  
**Previous ChatGPT canonical conversation:** https://chatgpt.com/c/6ace36d3-8634-8333-a79c-1a4cba32caeb (Chat 53)  
**Operator scheduling decision:** ChatGPT hourly deadman **DISABLED** at handoff. Do not recreate it automatically.

## Current Codex Desktop start prompt (2026-10-14 update)

The latest unified **copy-ready implementation instruction**, incorporating the **post-multi-project T-0484 token/context compaction requirement**, is at `docs/orchestrator/CATDESK_CODEX_DESKTOP_START_PROMPT_2026-10-14.md`. Use it as the entry point, then this longer handoff, active tickets and independently verified repo/CI. This historical handoff's original `bea190d` checkpoint is **not the latest HEAD**: at prompt generation, HEAD was `b0a664e` and CI run38641559086 was still IN_PROGRESS. The user has explicitly disabled the ChatGPT hourly deadman; do not recreate it.

## 0. Read this first — what to do in Codex Desktop

You are the primary engineering implementer for CatDesk. Open the **existing local repository** above in Codex Desktop and work from the checked-out feature branch. Read the files in §2, inspect live source and GitHub state rather than treating dated entries as timeless, then make concrete changes, tests, reviewable commits, and feature-branch pushes. The operator prefers `gpt-6.1-sol` at **medium reasoning effort**, if available; verify the **effective** model rather than silently switching to an older pinned model. A different Codex Desktop model should require operator direction if the named model is unavailable.

**First outcomes wanted, in order:**
1. Independently review and complete the **dev.85 WakeHost release/deployment and fresh Chat53 wake acceptance** without resending an ambiguous old event; make the independent WakeHost/adapter reliably recover from transient Chrome error pages. This is source CI-green, **not installed**.
2. Repair or cleanly bypass, by supported opt-in architecture, the CatDesk-managed **Codex Sol/medium execution route** so a real Codex task can produce a concrete deliverable and an authentic `review_ready` record; never fake this with a manual debug wake. See T-0483.
3. Continue the already started **T0478 isolated development worker** from B3B2B2B (owner/identity-bound safe cleanup), then T0478-C manifest-gated MCP read/search and T0478-D bounded worker lifecycle; move to T0479 stable versioned worker/LKG and unfinished production recovery.
4. Finish later milestones: usable Binagotchy CLI UX and **multi-project wake queue/fairness/worker orchestration**, while preserving completed GitHub mirroring.
5. **AFTER the multi-project milestone is independently accepted**, implement core T-0484: per-conversation token/context-pressure estimation, conservative configurable auto-compaction, structured Chat A handoff and safely verified new-chat continuation via **the existing SeleniumBase browser**. See `docs/orchestrator/tickets/T-0484_POST_MULTI_PROJECT_CONTEXT_METER_AUTO_COMPACTION.md`. This is core planned functionality but not immediate work; do not prioritize it ahead of multi-project acceptance.

Choose a single small, testable increment at a time, record what actually passed, and continue rather than restarting CatDesk architecture from scratch.

## 1. What CatDesk is and how it is supposed to work

CatDesk is a local ChatGPT/Codex coding and agent-orchestration harness intended to let ChatGPT Web act as architect/reviewer and Codex/Ollama act as implementers, with controlled local filesystem/terminal/Git/MCP access, persistent plans/tasks/reviews and browser-based wake continuation. Intended components:

- **CatDesk Rust daemon / MCP gateway:** tools for workspace read, edit, command execution, Git, task/session state and protected workflows. The CatDesk v4 secure tunnel was `CONNECTED_VERIFIED` at handoff.
- **Codex CLI / app-server:** coding worker and project-thread continuity. Local `codex-cli 0.160.0`, `codex login status` → `Logged in using ChatGPT`; `codex exec --help` and a read-only `gpt-6.1-sol` / medium model probe worked. Larger `codex exec` / `codex queue` calls invoked *through CatDesk run_command* intermittently returned `INVALID_ARGUMENT` or timed out, with no genuine milestone-deliverable receipt. This is a **CatDesk gateway/managed routing issue**, not evidence that Codex is absent.
- **Qwen/Ollama:** optional cheaper local workers; do not silently substitute one for the requested Codex implementation.
- **Review-inbox / autonomous session / Git evidence:** a worker produces a bounded task result and review bundle; ChatGPT or independent reviewer checks actual files, tests and CI.
- **Independent WakeHost + SeleniumBase Python adapter + Binagotchy:** durable review events target one ChatGPT conversation; browser sends/observes one event, verifies exact persisted USER receipt and assistant completion; manual test events are **not** natural review acceptance.
- **Secure MCP tunnel:** owned by its separate official runtime. Do **not** start a second copy or replace profiles, tunnel credentials, authorization, or browser-auth identity.
- **Future stable supervisor and versioned worker:** audited health/recovery, one command per layer, signed reviewed installations and last-known-good rollback, not yet fully accepted.

CatDesk's current scope has **three different security/deployment lanes**: (A) ordinary isolated source edits + feature branch + GitHub CI; (B) separately installed independent WakeHost with reviewed immutable package/hashes; (C) specially protected signed CatDesk main image/supervisor promotion. Passing tests in lane A does **not** activate lanes B or C.

## 2. Authoritative files to read (and precedence)

1. **This handoff** and `docs/orchestrator/tickets/T-0483_CODEX_SOL_MEDIUM_WAKE_CYCLE.md`.
2. `.catdesk/session.md`, `.catdesk/current_plan.md`, `.catdesk/todo.md`, `CATDESK_MILESTONES.md`, `CATDESK_NEW_CHAT_NOTES.txt`, `docs/orchestrator/CATDESK_IMPORTANT_COMMANDS.md`.
3. `docs/orchestrator/review_bundles/T-0478_R3B4B3B2_OWNER_ACL_HANDLE_CLEANUP_CONTRACT.md`, `T-0478_R3B4B3B2B1_PINNED_OWNER_DACL_READBACK.md`, `T-0478_R3B4B3B2B2A_RETIREMENT_OBJECT_IDENTITY.md`, and `T-0478_R3B4_SOURCE_SNAPSHOT_PROVENANCE_DESIGN.md` in that folder.
4. Wake source and tests: `wake/Cargo.toml`, `wake/src/runtime.rs`, `wake/adapter.py`, `wake/install.ps1`, `scripts/wake_bridge.py`, `tests/test_wake_bridge.py`, `tests/test_stable_wake_browser_adapter.py`; relevant historical reports `docs/orchestrator/review_bundles/T-0419_WAKE_CHROME_ERROR_PREPROMOTION_REVIEW_20261008.md` and `T-0419_WAKE_R5_RESPONSE_REOPEN_QA_20261008.md`.
5. Codex management: `src/delegated/autonomous_contract.rs`, `src/delegated/codex_cli.rs`, Codex app-server integrations, `src/mcp.rs` and `docs/orchestrator/tickets/T-0483_CODEX_SOL_MEDIUM_WAKE_CYCLE.md`.
6. Snapshot / protected Windows FS: `src/isolated_fixture_snapshot.rs`, `src/isolated_fixture_manifest.rs`, `src/isolated_fixture_git_objects.rs`, `src/isolated_fixture_github_authority.rs`, `src/windows_protected_fs.rs`, `src/reviewed_source_snapshot.rs`, `tests/isolated_dev_fixture.rs`.

**Fresh-state rule:** several large append-only files contain obsolete headings like “LATEST,” and `.catdesk/current_plan.md` starts with a **stale Chat52 handoff**. Prefer date-sorted, *most recent independently verified* Git/CI/current installed readbacks over old headings. Repair/compact misleading top-level plan and command statuses as part of documentation maintenance—preserve forensic history instead of deleting it.

## 3. Exact known engineering state at transfer

### Git, CI and workspace
- Last inspected `HEAD`: `bea190d66ff2f9e840ebffee832f6ce6354251b9`, existing feature branch `orchestrator/chatgpt-codex-autonomous-loop` tracking `origin/orchestrator/chatgpt-codex-autonomous-loop`. Tracked files synchronized with GitHub; **GitHub mirroring is complete**. No need to redo mirror/initial setup.
- GitHub Actions latest source run for `bea190d`: **38550793848 SUCCESS**, and dev85 source run for `7df27ee`: **38443116318 SUCCESS**. Both have all three Windows jobs—Rust, Python Wake/advisor, Independent WakeHost Rust—terminally green. Check *new* commits against their own runs before claiming acceptance.
- Exactly seven unrelated historical untracked files were present; **do not stage/delete/overwrite them speculatively**:
  `docs/orchestrator/WAKE2_PROFILE_REAUTHENTICATION.md`;
  `docs/orchestrator/review_bundles/T-0418_GIT_CODEX_ALLOWLIST_DAEMON_RELOAD_REVIEW.md`;
  `docs/orchestrator/review_bundles/T-0461_R1_RECOVERY_DIAGNOSTICS_SOURCE_REVIEW.md`;
  `docs/orchestrator/review_bundles/T-0462_R1_RECOVERY_DIAGNOSE_GATEWAY_INDEPENDENT_REVIEW.md`;
  `pyproject.toml`;
  `run_current_v5_linker_diagnostic.cmd`;
  `run_v5_diagnostic_tmp.ps1`.

### Live WakeHost / canonical authority
- Independent WakeHost at handoff: installed `1.0.0-dev.84`, RUNNING; **dev85 is source-only and not active**.
- Both project registry and independent Wake target were previously verified for Chat53 URL above, generation **34**, SHA256 `b9c3c16a978e424ff4122f868450d4f7b8c540578dda2cc652cd3e04c557a3e3`.
- Latest Chat53 diagnostic `manual-wake-mcp-1791908511726` is `SUBMITTING`, attention `SUBMIT_TARGET_DRIFT_CHROME_ERROR`, no exact current event receipt. Queue depth 0, but queue 0 is **not** SENT proof. The older Chat52 gen33 events are invalid for current-generation acceptance and are forensic only. Do not resubmit/replay ambiguous USER events.
- The source fix adds `SUBMIT_TARGET_DRIFT_CHROME_ERROR` to bounded **read-only** SUBMITTING reconciliation and corrects same-CDP exact-target navigation retries, final receipt reproof and an immediately checked return from successful `cdp.get()`. Commits `09c0456` and `7df27ee` respectively. Source tests/CI pass; installed package still lacks these changes.
- A reviewed immutable install path exists: `wake/install.ps1` builds wake binary and Binagotchy GUI (optionally `-RebuildGui`), stages a manifest and SHA256 of `CatDeskWakeHost.exe`, `CatDeskBinagotchy.exe`, `adapter.py`, `wake_bridge.py`; Rust `publish-reviewed-install` + `activate-reviewed-install` take the exclusive install lease and perform pointer/host handoff. **Confirm review/installation authorization first**, obtain version/hash readback, avoid killing official tunnel or installing unsigned main image.
- Current ChatGPT hourly fallback: **disabled by operator request on 2026-10-14**; leave OFF. If handing back to ChatGPT later, user can opt in to a new schedule. Codex Desktop may work in foreground now.

### Codex model and delegation
- Local Codex CLI is **available**: `codex-cli 0.160.0`, logged in using ChatGPT; `codex --version`, `codex login status`, and `codex exec --help` all succeeded via CatDesk gateway at handoff.
- Earlier read-only `codex exec -m gpt-6.1-sol -c model_reasoning_effort=medium --json READY` produced a completed thread; this proves a **model probe** only. Full milestone worker tasks launched through CatDesk gateway have **not** reliably executed.
- Existing CatDesk managed autonomous contract in `src/delegated/autonomous_contract.rs` expects `gpt-5.6-terra`, while `src/delegated/codex_cli.rs` binds high reasoning effort; do not misrepresent that as successful Sol/medium work. Build a backward-compatible **opt-in** route for the operator-preferred `gpt-6.1-sol`/medium, with authoritative effective-model telemetry and rejection on silent downgrade. Codex Desktop's native CLI/workspace access may make this development straightforward without going through CatDesk MCP.
- T-0483 defines a specific **first real Codex deliverable**: `docs/orchestrator/review_bundles/T-0483_R1_CODEX_DEV85_RELEASE_REVIEW.md` analyzing the dev85 package, code/tests/CI, signed/immutable boundaries, install preflight, rollback and exact fresh canary. This file is not yet accepted as produced by Codex. A manufactured manual Wake debug test is **not** a `review_ready` event.

### T0478 current state
- **ACCEPTED SOURCE ONLY:** isolated authenticated read/search fixture, Git-object source with witnessed GitHub branch+CI provenance, manifest canonicalization/size/SHA, pinned OS-temp snapshot roots and retained file handles. Pinned owner-only Windows ACL/SID semantic readback B3B2B1 fixed and passed Windows CI through `f908314`/later runs. B3B2B2A gathers non-destructive pinned volume/file identity evidence.
- **NOT IMPLEMENTED:** B3B2B2B native exact-handle owned cleanup; T0478-C actual producer-owned manifest-gated isolated MCP server; T0478-D bounded owner PID/port/shutdown/crash lifecycle.
- **NOT COMPLETE:** T0479 versioned development worker release/LKG, stable production supervisor recovery, restored Binagotchy CLI UX and multi-project fair wake queue.

### Production supervisor
A fresh read-only stable-supervisor status at transfer reports `SUPERVISOR_ROOT_UNAVAILABLE`, `SUPERVISOR_STARTUP_POLICY_UNPROVEN`, `SUPERVISOR_STARTUP_DEFINITION_READ_FAILED`, `SUPERVISOR_3201_NOT_PROBED`. Image/startup may separately say `SUPERVISOR_LIFECYCLE_READY`, **not** sufficient for full recovery. Signed installed T0215 epoch1 and pending T0366 epoch2 remain under separate review/promotion authority. No direct protected-file editing, ad-hoc task/service rewiring, signer handling, or forced replacement.

## 4. Codex Desktop implementation phases and acceptance gates

### Phase A — Inventory and repository hygiene (first action)
- Open local repo, confirm branch/tracked upstream/HEAD/dirty state, check CI for current SHA. Confirm no existing Codex worker or independent Wake browser actor is executing a conflicting write. Read §2. Capture a brief current-state review in the handoff or new dated review bundle.
- Keep the seven historical untracked files outside new commits; use `git add -- <explicit scoped paths>`, never `git add -A`; no force pushes; no changes to main/master. Inspect existing Codex/Desktop project policies.
- Fix only factual, current-facing stale documentation (especially `.catdesk/current_plan.md` and misleading old “LATEST” blocks). Preserve historical evidence.

### Phase B — Wake dev85 reviewed installation and canary (highest operational priority)
1. Review `7df27ee` plus predecessor `09c0456`, exact current immutable package inputs, all related Python/Rust tests and CI. Confirm dev.85 package ID is aligned in `wake/Cargo.toml`, `wake/Cargo.lock`, and root `Cargo.lock`.
2. Document independently reviewed source hashes, candidate/test/build status, installed dev84 vs candidate dev85, the proper `wake/install.ps1` path, expected `current.json` and artifact SHA readbacks, rollback if interrupted, and browser/target authority safety; record a real review bundle.
3. With suitable approved local permissions and a passing review, use only the existing **reviewed** Wake package installer/activation mechanism. Do not bypass the exclusive Rust installer or copy binaries over an active installation. A `-BuildOnly` call via CatDesk MCP gateway previously returned `INVALID_ARGUMENT`; direct Codex Desktop approved local execution may work without that gateway limit, but it is **not permission to bypass CatDesk's protected policy**.
4. Verify host version genuinely `dev.85`, SHA-matched Python adapter+bridge and executable, canonical target still Chat53 gen34, no competing submission. Only then produce **one NEW** diagnostic event, never replay `manual-wake-mcp-1791908511726`. Accept manual Wake only on exact latest event `EXACT_USER_MESSAGE_APPENDED`, terminal `SENT`, timer `COMPLETE`, no duplicates, browser closes and auth remains ready.
5. Test intermittent `chrome-error://` navigation in a bounded controlled way; test read-only reconciliation of already-SUBMITTING without second USER Send. Review wrong-host and different-chat fail-closed behavior. Only a real task's independent review event can prove **natural** wake acceptance.

### Phase C — Real Codex task → review → Wake cycle (T-0483)
1. Select `gpt-6.1-sol` with `medium` effort using Codex Desktop/native CLI if available; capture effective identity and effort. Preserve Terra/high legacy sessions, do not silently substitute or globally overwrite the managed model policy.
2. Implement a narrow opt-in model policy/adapter, or carefully document why desktop-native delegation makes a CatDesk API change unnecessary. Validate single-writer ownership, queue state, model mismatch failure, provider exhaustion, idempotent resume, and back-compat tests.
3. Run a **real bounded Codex review task** on dev85 release-readiness; produce the named T-0483 R1 review bundle and exact verification artifacts. Record task/session/thread identity, source SHA and diff.
4. Have the actual CatDesk review-inbox/session state publish one `review_ready` bound to that deliverable, not a fake test event. The independent installed WakeHost must deliver exactly that event to current paired target.
5. ChatGPT reviewer (when re-engaged) checks the exact bundle, source diff and CI, ACKs or rejects it and creates the next scoped task. Codex Desktop can continue implementing directly during the operator's pause; do not assume a background ChatGPT task is active merely because an event was enqueued.

### Phase D — T0478 B3B2B2B safe snapshot retirement
- Implement only after reviewing the owner ACL + pinned identity contracts. Root and all manifest leaves must be owner-verified through real Windows token/handle authority and exact root/file volume+index+SHA+size; retain pinned parent/root capability and reject reparse/junction/symlink/hardlink, foreign/missing/modified files, races and stale generations.
- Release only owned read leases as needed; use audited **native exact-open-handle** delete disposition and exact root-empty proof. Never use `std::fs::remove_dir_all`, `Remove-Item -Recurse`, wildcard/path-prefix deletion, caller-chosen arbitrary DELETE path, or adoption of an orphan based solely on a generated directory name. On ambiguity fail closed, preserve outside data and surface operator attention.
- Add adversarial Windows tests: good two-file cleanup, missing/injected/foreign children, replacement race, hardlink, modified content, crash-partial stage, double cleanup, simultaneous owners, orphan lacking durable authority. Pass local focused and independent Windows CI.

### Phase E — T0478-C/D, T0479 and full production recovery
- **C:** Bind only a *producer-owned immutable in-memory* `StagedCandidateSnapshot` and trusted Git source witness to the live isolated read/search MCP fixture; per-request exact manifest membership/SHA/identity, no caller-submitted manifest or unrestricted workspace capability.
- **D:** Owner-only bounded disposable child PID and loopback port, readiness checks, exact shutdown, crash handling/restart and no conflict with official Secure MCP tunnel; verify no orphan workers.
- **T0479:** reviewed immutable versioned dev-worker/LKG release pipeline, activation/rollback health gates, stable adapter contracts and source-to-release traceability.
- **Production recovery:** inspect fixed nine-layer diagnostics, supervisor root/startup-policy/Task Scheduler definition and port3201, existing signed reviewed lifecycle. Do not infer recovery success from healthy MCP alone or rotate protected images without explicit approval. Then resume Binagotchy CLI polish and multi-project queue fairness after lower layers are reliable.

## 5. Verification, evidence, and commit discipline

Normal repository verification commands from the project:
```powershell
cargo fmt --check
cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings
cargo test --locked --offline --bin catdesk
cargo test --locked --offline --test isolated_dev_fixture
pytest tests/test_wake_bridge.py -q
git diff --check
```

Run standalone Wake crate tests with its appropriate manifest/workspace context; do not call a rejected gateway invocation a passing test. Use smaller focused suites when timing out, but never mark full suite PASSED unless run to completion. Windows GitHub acceptance needs **Rust + Python Wake/advisor + Independent WakeHost Rust** terminal SUCCESS for the exact commit. Link the run IDs in review notes. Existing `cargo build --locked --offline --bins` checks the cross-binary Windows linkage problem previously seen with Advapi32.

Each scoped increment should have: problem and boundary, diff/changed files, explicit tests and exact results, security/negative tests, independent CI URL, rollback/remaining limitations, and a Git commit on the feature branch pushed to GitHub. A source commit is not a reviewed Wake package or signed production installation.

## 6. Non-negotiable safety and handoff rules

- Do not access/export credential material, private signer keys, browser profile secrets, secure tunnel tokens or raw protected-state files. Use named fixed-policy interfaces only.
- The official OpenAI Secure MCP tunnel remains separately owned; don't launch a second tunnel or tear it down to deploy Wake.
- Preserve signed installed T0215/pending T0366 images and protected supervisor. Distinguish source, build, publication and live acceptance.
- Treat historical events by immutable target generation and exact event ID; old ATTENTION does not block unrelated new work, but ambiguous current event must not be retransmitted. Manual events are diagnostics, not evidence of natural review wake.
- Use least privilege and scoped project paths. No destructive sweep, `git clean`, implicit extra file staging, main branch rewrite, forced Git push, or unreviewed Windows Task Scheduler/Service changes.
- ChatGPT hourly deadman is **OFF** by operator request; do not reenable by default. Do not assume ChatGPT Web will review a future deliverable unless its review event is actually delivered or the operator resumes the conversation.
- If an operator-only step is truly necessary, specify one exact safe command/action, why it is needed, and what readback proves completion; do not request obsolete status data again.

## 7. Definition of complete and the first Codex Desktop response expected

For initial Codex Desktop handoff acceptance, report:
- Reconfirmed checkout and branch/CI; exactly which historical files were preserved.
- Current installed vs source Wake package identity and actual release gate.
- Codex effective model/effort and a **real** task plan (not a model probe).
- The next concrete source/doc deliverable and tests, with clear owner and rollback.
- Whether you can carry out the reviewed dev85 installation yourself; if not, the precise operator-only blocker.
- The plan for native T0478 B3B2B2B cleanup afterward, without letting Wake repairs indefinitely consume all project time.

A reliable autonomous loop is complete only when a real Codex task produces a reviewed artifact, an exact `review_ready` reaches ChatGPT through the **installed** WakeHost, the event is terminal `SENT` with verified receipt and timer `COMPLETE`, ChatGPT reviews/ACKs the right artifact, and the subsequent Codex task proceeds without duplicate writer/submission.

## 8. Competitor research — future browser companion and automatic new-chat continuity

The operator also asked Codex Desktop to consider the prior *CatDesk competitors* discussion, especially Chat On Steroids' existing-chat wake, **automatic context-threshold compaction**, and **new ChatGPT conversation creation**. The operator explicitly prefers **retaining CatDesk's SeleniumBase + Rust WakeHost**, not migrating to a browser extension. The source-grounded design analysis is `docs/orchestrator/CATDESK_COMPETITOR_BROWSER_CONTINUITY_2026-10-14.md`. It explains CoS's authenticated WebSocket wake notifications, persistent browser extension, durable input election/receipt ledger, native Chrome tab/new-chat UI creation, delayed `/c/<id>` assignment, and transactional **Compact & Resume** local session rebinding. It compares Desktop Commander, ChatGPT Local Coder, ChatCMD, Codex with ChatGPT, 1Code and OpenMausBot and proposes distinct future CatDesk R1–R4 milestones and security/acceptance gates. The highest-value concept is **separate durable CatDesk session identity from individual ChatGPT conversation IDs**, then safely and atomically transition A→B without losing Codex/worker/task history. Treat this as a future optional architecture after the current dev85/T0483 work, **not** immediate permission to install extensions or automate arbitrary new chats. Keep the hourly deadman OFF.

**Token/multi-project follow-up (2026-10-14):** Existing CatDesk `o200k_base` MCP token counters measure serialized tool arguments/results and aggregate traffic, not exact per-conversation ChatGPT context. Chat On Steroids' context meter counts visible messages and tool events by ~4 chars/token with 10k cap per MCP result; 1Code adds per-task Git worktree isolation. CatDesk should combine its tokenizer with a durable per-project/per-conversation ledger only **after multi-project queue/fairness**, keeping SeleniumBase. Source-backed next design: `docs/orchestrator/tickets/T-0484_POST_MULTI_PROJECT_CONTEXT_METER_AUTO_COMPACTION.md`; source-level comparison: `docs/orchestrator/CATDESK_COMPETITOR_BROWSER_CONTINUITY_2026-10-14.md`.

**Start by reading this handoff, T-0483, the latest CI, and the current Git state. Do the first concrete task instead of writing another broad architecture plan.**
