# CatDesk — Updated Codex Desktop execution handoff / master prompt
**Updated 2026-10-14. Copy the "BEGIN CODEX PROMPT" section into Codex Desktop.**
**Source of truth:** live checkout + latest CI and installed readbacks; prior dated statuses are evidence, not permanent facts.
**Repository:** https://github.com/vnath99/CatDesk
**Local Windows checkout:** `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop`
**Expected branch at last verification:** `orchestrator/chatgpt-codex-autonomous-loop`
**Verified HEAD before this prompt:** `b0a664e` (tracked upstream synchronized; seven unrelated historical untracked files protected)
**Last verified CI:** `b0a664e` Actions 38641559086 was still running (Independent WakeHost Rust SUCCESS; Rust and Python in progress). Source Wake `7df27ee` Actions 38443116318 three of three SUCCESS, and `dfaf317` Actions 38639126269 three of three SUCCESS. Do not claim an in-progress run succeeded.

---

## BEGIN CODEX PROMPT

You are assuming **primary engineering implementation ownership** of the existing **CatDesk** project. The user wants **working, verified software**, not another architecture-only discussion. Work directly in the EXISTING repository and continue the already-implemented components and documented milestones. Use the **GPT-6.1 Sol model at medium reasoning effort** if available, and check actual model/effort; do not silently substitute a different model or claim the CatDesk-managed old Terra/high route is Sol/medium. If the requested model is unavailable, report that explicitly rather than claiming compliance.

### Product goal

CatDesk is a local ChatGPT Web + MCP + Codex/Ollama orchestration harness. The long-term target is reliable, sustained multi-project engineering in which ChatGPT can review deliverables and Codex/local agents can edit/test code, with durable tasks, CI-linked review records, event-driven wakeups, auto-recovery, auditability, and easy GUI/CLI control. A longer-term milestone will estimate per-conversation context usage, compact an oversized conversation, create a new authenticated ChatGPT conversation using the EXISTING SeleniumBase browser, and safely resume the same durable project/session in that new chat.

Retain the current **Rust MCP gateway + independent Rust WakeHost + Python SeleniumBase** architecture. Do **not** replace Selenium with a browser extension. Study CoS, 1Code and other competitors for their architectural patterns only, especially durable project identity, queues, safe chat migration and isolated worktrees.

### 1. Rehydrate and independently verify before changing anything

Read these files, then inspect their cited source, CI and review bundles:

- `docs/orchestrator/CATDESK_CODEX_DESKTOP_HANDOFF_2026-10-14.md` — comprehensive overall handoff and source map.
- `docs/orchestrator/CATDESK_COMPETITOR_BROWSER_CONTINUITY_2026-10-14.md` — researched CoS and other competitors, plus decision to **keep Selenium**.
- `docs/orchestrator/tickets/T-0483_CODEX_SOL_MEDIUM_WAKE_CYCLE.md` — real Codex task → deliverable → review_ready → Wake cycle; currently **not end-to-end accepted**.
- `docs/orchestrator/tickets/T-0484_POST_MULTI_PROJECT_CONTEXT_METER_AUTO_COMPACTION.md` — **CORE milestone strictly AFTER multi-project scheduling/fairness is accepted**. Ticket number does not imply immediate priority.
- `CATDESK_MILESTONES.md`, `docs/orchestrator/CATDESK_IMPORTANT_COMMANDS.md`, `CATDESK_NEW_CHAT_NOTES.txt`.
- `.catdesk/session.md`, `.catdesk/current_plan.md`, `.catdesk/todo.md` (beware that the opening of current_plan can contain an obsolete Chat52 “LATEST” heading).
- `docs/orchestrator/review_bundles/T-0478_R3B4B3B2_OWNER_ACL_HANDLE_CLEANUP_CONTRACT.md` and subsequent B3B2B1/B3B2B2A reports, relevant T0478/79 architecture and recovery reports.
- Current source and tests in `wake/`, `scripts/wake_bridge.py`, `tests/test_wake_bridge.py`, `src/delegated/`, `src/mcp.rs`, `src/windows_protected_fs.rs`, `src/isolated_fixture_*.rs`, `src/reviewed_source_snapshot.rs`, `tests/isolated_dev_fixture.rs`.

At the start, verify: HEAD/branch/upstream and uncommitted status; CI for exact HEAD and latest relevant earlier builds; active CatDesk/Codex writers; installed WakeHost `current.json` and hashes; paired project/Wake target; signed production image state; supervisor layers. Do not trust an old status heading over current evidence.

Last known live state, **recheck**:
- Local CatDesk tunnel CONNECTED_VERIFIED; `codex-cli 0.160.0` installed and authenticated via ChatGPT; ordinary CLI help and an earlier `gpt-6.1-sol`/medium read-only probe worked. Substantive CLI exec/queue **through CatDesk's command gateway** failed or timed out. Codex Desktop should use its normal permitted repository/CLI access rather than assume Codex is absent, but still respect protected release boundaries.
- CatDesk managed autonomy contract `src/delegated/autonomous_contract.rs` pins `gpt-5.6-terra` and `src/delegated/codex_cli.rs` binds high effort. Add **opt-in** Sol/medium compatibility only with tests, preserve legacy sessions, verify effective selected model and never silent-fallback.
- Independent WakeHost installed **dev.84**. Reviewed **dev.85 source** passed all three Windows CI jobs in run 38443116318 but had **not been installed**. Chat53 target URL `https://chatgpt.com/c/6ace36d3-8634-8333-a79c-1a4cba32caeb`, paired project/Wake generation 34, digest `b9c3c16a978e424ff4122f868450d4f7b8c540578dda2cc652cd3e04c557a3e3`. Last current-gen manual event `manual-wake-mcp-1791908511726` was `SUBMITTING`, `SUBMIT_TARGET_DRIFT_CHROME_ERROR`, without exact receipt. **Do not replay or resend that ambiguous USER event**. Old Chat52 gen33 events are forensic, not current authority. Source alone does not equal installed Wake acceptance.
- Stable supervisor recovery incomplete: `SUPERVISOR_ROOT_UNAVAILABLE`, `SUPERVISOR_STARTUP_POLICY_UNPROVEN`, `SUPERVISOR_STARTUP_DEFINITION_READ_FAILED`, `SUPERVISOR_3201_NOT_PROBED`. “Supervisor lifecycle ready” is not production recovery proof.
- Project registry already includes `catdesk`, `BUG_BOUNTY_RECON_PLATFORM`, `BYOVD_DRIVER_PIPELINE`. Registration is not proof that multi-project queues/fairness and cross-project review/Wake isolation work.
- ChatGPT hourly deadman is **DISABLED by operator request**. Do not enable or create any hourly automation; a new Codex Desktop task is not permission to restart it.

**Preserve these seven historical untracked paths** unless the operator explicitly asks for their disposition:
`docs/orchestrator/WAKE2_PROFILE_REAUTHENTICATION.md`;
`docs/orchestrator/review_bundles/T-0418_GIT_CODEX_ALLOWLIST_DAEMON_RELOAD_REVIEW.md`;
`docs/orchestrator/review_bundles/T-0461_R1_RECOVERY_DIAGNOSTICS_SOURCE_REVIEW.md`;
`docs/orchestrator/review_bundles/T-0462_R1_RECOVERY_DIAGNOSE_GATEWAY_INDEPENDENT_REVIEW.md`;
`pyproject.toml`; `run_current_v5_linker_diagnostic.cmd`; `run_v5_diagnostic_tmp.ps1`.
Do not `git clean`, `git add -A`, force-push or overwrite them blindly.

### 2. Milestone execution priority: do not skip verification

**P0 — Make WakeHost dev.85 operational.** Review source `09c0456` + `7df27ee`, the bounded same-tab Chrome-error retry/reconciliation and exact receipt tests; create or independently complete the source-bound release-readiness review `docs/orchestrator/review_bundles/T-0483_R1_CODEX_DEV85_RELEASE_REVIEW.md`. Confirm immutable package version in `wake/Cargo.toml`, `wake/Cargo.lock` and root `Cargo.lock`. Review `wake/install.ps1` and existing Rust `publish-reviewed-install` / `activate-reviewed-install` handoff, hash-verified installed `CatDeskWakeHost.exe`, `CatDeskBinagotchy.exe`, `adapter.py`, `wake_bridge.py`, and safe rollback/failed-install status. Do not bypass protected installer policy or the installation lease; ask for an exact operator-only action if required. **After reviewed approval and verified dev.85 install**, send exactly **one fresh** Chat53 diagnostic event and require current-generation `EXACT_USER_MESSAGE_APPENDED`, `SENT`, timer `COMPLETE`, queue/ownership convergence and browser cleanup. Manual debug tests do **not** count as natural review_ready Wake acceptance.

**P1 — Genuine Codex/ChatGPT review cycle (T0483).** Implement the backward-compatible opt-in Sol/medium Codex CLI/app-server path, actual effective-model/effort readback, one-writer process/session lease and durable task-to-review identity. Execute one **real** bounded Codex task that produces a reviewed artifact and exact CI/diff. Have the **real** CatDesk review process issue one source-bound `review_ready`, rather than faking it with `binagotchy test`; require authenticated current-generation event delivery before claiming automated ChatGPT review. Since ChatGPT hourly deadman is OFF, do not assume ChatGPT will answer until a new event actually reaches it or the operator reopens the chat.

**P2 — Isolated worker and reliable recovery (T0478, T0479).** Continue from the existing source-only fixture and accepted B3B2B1 Windows owner/SID/DACL readback and B3B2B2A pinned read-only identity evidence. Next implement **B3B2B2B exact owner-authorized opened-handle native cleanup** under reviewed security authority; reject foreign/reparse/hardlink/collision/replacement/stale objects, no blanket path recursive delete or user-provided arbitrary DELETE path. Then T0478-C real immutable manifest-gated isolated MCP read/search, T0478-D bounded owned worker PID/port lifecycle/crash recovery, T0479 versioned worker release/LKG rollback. Separately repair the production supervisor's missing root/startup definition/policy/port3201 only through reviewed fixed-policy controls. Do not rotate signed production artifacts without explicit approved signing/installation authority.

**P3 — Binagotchy CLI/TUI and multi-project ownership/fairness.** Restore a reliable readable CLI/TUI with health for every layer, safe targeted recovery controls, task/session/review status and Codex usage. Then implement durable **per-project** queues, local workspace bindings, independent authorized ChatGPT targets/generations, worker leases, fairness/starvation protection, crash/restart persistence and correct review/Wake routing. Use testable scenarios with CatDesk + Bug Bounty + BYOVD concurrent tasks; one project's error/migration must not block or redirect another. Study CoS `src/main/projects.ts` for stable local project/session/workspace identity and optional ChatGPT Project linking, and 1Code for worktree-isolated concurrent agents. Their project registries are inspiration; they do not prove CatDesk queue fairness.

**P4 — CORE, explicitly after accepted P3: T0484 context estimator and safe automatic compaction.** **Keep SeleniumBase**, no browser-extension migration. Reuse CatDesk's `o200k_base` estimator in `src/mcp.rs` for *observed* serialized MCP calls/results but understand its ↓/↑/Σ/$/ƒ widget counts are **tool traffic, not actual ChatGPT context occupancy**. Build a durable, deduplicated ledger keyed by project + conversation + generation + turn/message/tool-call IDs, combining visible user/assistant text, model-visible tool result and argument estimates, and clear unknowns for native PDFs/images/system prompts/hidden reasoning/provider truncation. Never use lifetime `inputTokens+outputTokens` as remaining-context measure or add the same result twice. CoS uses rough ~4 chars/token and caps MCP result estimates at 10k per call; its 400k setting is configurable, **not a provider context guarantee**. Add conservative, model/profile-aware warning and compaction thresholds with uncertainty. When over threshold and at a verified safe point, produce a structured Chat A handoff, use the existing authenticated Selenium browser to create Chat B (id-less until ChatGPT assigns `/c/<id>` after initial user message), verify exact unique bootstrap receipt, and then **atomically rebind only that project's** session/worker/project/Wake target. Do not mistake a provider limit/refusal for permission to create new chats.

This ordering is explicit. **Do NOT begin T0484 before the multi-project scheduler P3 is accepted** unless the operator reprioritizes it.

### 3. Rules for implementation

- Prefer successive **working, end-to-end verified increments** over accumulating architecture-only plans. Before implementing a ticket, inspect existing code/tests; preserve good components.
- Maintain a single active workspace writer/lease; separate Git worktrees or review branches for parallel independent changes only if their merge/ownership rules are clear. Avoid collisions with another Codex Desktop task.
- Use source-control review on `orchestrator/chatgpt-codex-autonomous-loop`; commit and push scoped paths on the same branch as appropriate, never rewrite main/master or force-push. Show exact diff and link the CI run for the commit.
- Core checks usually include `cargo fmt --check`, strict offline workspace Clippy, full/targeted `cargo test --locked --offline`, relevant Python `pytest`, `git diff --check`, independent GitHub Windows **Rust / Python Wake and advisor / Independent WakeHost Rust** all terminal SUCCESS for the exact reviewed SHA. Short, failed or gateway-rejected commands are NOT passing tests.
- Keep `CATDESK_MILESTONES.md`, `docs/orchestrator/CATDESK_IMPORTANT_COMMANDS.md`, active ticket/review documentation and `.catdesk/session.md` current as work progresses. Old `.catdesk/current_plan.md` starts with stale Chat52 “LATEST”; repair current-first presentation without deleting historical forensic records.
- Source dev builds ≠ WakeHost installed packages ≠ signed CatDesk main image ≠ live production acceptance. Keep installed T0215/pending T0366 and private signing/rotation workflows untouched unless explicit, independently accepted protected operator action is granted. Never read/export credentials, browser profiles, private keys or tunnel tokens; the **official Secure MCP tunnel is externally owned** and must not be restarted as a side effect of WakeHost debugging.
- Restrict browser automation to approved, authenticated current ChatGPT UI actions; stop on CAPTCHA, auth/account prompts, policy refusal or uncertain foreign route. Never re-Send an ambiguous USER message; old ATTENTION records are preserved but must not become a new-generation blocker.
- No project quota/usage/safety restriction circumvention through model substitutions, extra chats or new tabs. A new chat is for authorized session continuity only.

### 4. How to work autonomously and what to report

**Do not stop at this handoff and ask what to work on.** First verify live state, choose **P0's smallest substantive actionable task**, implement or independently review it, and produce the concrete next artifact in the repository. If P0 is truly gated by operator-only approval, document the exact evidence and one action requested, then continue a safe independent P1/P2 source-only increment without touching the protected installation.

For each finished increment, report:
1. Milestone/ticket and what changed in source, with exact file paths.
2. Tests run and observed PASS/FAIL/SKIPPED (no invented CI).
3. Git branch, commit SHA, push status and independent Actions run/result.
4. Security impact, protected assets untouched, rollback readiness.
5. Current blocker, if any, and one precise operator action only when necessary.
6. The specific next implementation task and expected deliverable. Persist this in a durable handoff/checkpoint so a future Codex session resumes without rereading entire conversation.

Definition of success for the desired sustainable cycle: **a real Codex task has a specific delivered artifact and terminal review, an authentic CatDesk review_ready wakes the correct ChatGPT conversation with current-generation EXACT_USER_MESSAGE_APPENDED/SENT/COMPLETE and no duplicate, ChatGPT reviews that artifact, and the next distinct Codex task begins without conflicting writers.** This is not yet proven at handoff. The hourly deadman stays OFF.

**BEGIN NOW: verify existing workspace/CI, then execute the next bounded P0 deliverable.**

## END CODEX PROMPT
