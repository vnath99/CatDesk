# T-0364 Wake / CatDesk explicit local-Qwen initial route — R5

Date: 2026-09-19
Status: SOURCE-VERIFIED / NOT DEPLOYED

## Problem

A fresh CatDesk autonomous session always initializes as `CODEX_PREFERRED`. The local routine provider (`ollama` / `qwen3.8:27b`) is selectable only after a real Codex-credit-exhaustion transition or when recovering an already-Qwen session. That means a fresh review-only task cannot honor the operator's provider policy without either spending a speculative Codex turn or falsifying Codex exhaustion. The ChatGPT connector also currently omits the first-class direct-ChatGPT finalizer, so that no-provider completion route is unavailable.

## Narrow design

- Preserve historical mode `chatgpt_web_codex_autonomous` unchanged.
- Add one explicit mode: `chatgpt_web_qwen_autonomous`.
- Unknown modes still fail closed.
- The explicit Qwen mode reuses the already-existing local routine provider/model, provider adapter, tool definitions, workspace/path policy, Git policy, verification policy, lease, hard stops, review inbox, and accounting surfaces. It creates no new provider or trust domain.
- Contract creation persists the existing `QWEN_FALLBACK_ACTIVE` route before approval/start. The enum name is historical; deliberate Qwen start is distinguished by contract mode.
- For deliberate Qwen start, the controller emits an explicit routine-provider instruction and does not load or create a Codex-exhaustion handoff.
- Historical Codex-exhaustion fallback keeps its existing handoff behavior unchanged.
- No Codex eligibility timestamp or fake exhaustion evidence is generated for deliberate Qwen start.

## Files changed

- `src/delegated/autonomous_contract.rs` — accepts the explicit Qwen mode and exposes `starts_on_routine_provider()`; regression covers legacy/default/unknown modes.
- `src/delegated/autonomy_supervisor.rs` — persists initial Qwen route only for the explicit mode; regression proves default sessions remain `CODEX_PREFERRED`.
- `src/delegated/autonomous_controller.rs` — deliberate Qwen instruction no longer requires Codex exhaustion handoff; regression proves first provider launch is Ollama, no Codex launch occurs, provider turn accounting advances, and no synthetic handoff exists.

## Verification

- `cargo fmt --check` — PASS. Log `.catdesk/logs/1789866726-58477cbd-7e17-4b2b-9f14-88aabb1f860d.log`.
- Focused contract mode regression — PASS.
- Focused supervisor route-persistence regression — PASS.
- Focused controller initial-Qwen/no-handoff regression — PASS. Log `.catdesk/logs/1789866838-9562e4da-b2a8-4bf7-a81e-b11db9160dda.log`.
- Autonomous contract suite — 9/9 PASS. Log `.catdesk/logs/1789866851-bc0961ee-55a7-40d6-901e-97aec50695d9.log`.
- Autonomy supervisor suite — 26/26 PASS. Log `.catdesk/logs/1789866857-31156c1d-71e6-49fe-ab2b-0f6076443a82.log`.
- Autonomous controller suite — 52/52 PASS. Log `.catdesk/logs/1789866869-b38d0f52-01ab-4286-9420-444f1c071ec1.log`.
- `cargo clippy -q --bin catdesk -- -D warnings` — PASS. Log `.catdesk/logs/1789866897-f460405d-17c8-4b73-9669-1b880e858186.log`.
- `git diff --check` — PASS; normal Windows LF/CRLF warnings only. Log `.catdesk/logs/1789866902-a1ed2391-fedd-4158-8831-d7b8cef0e0be.log`.

## Provider / review history

A genuine dev.20 autonomous review was started before this source change. It selected Codex/Terra-high because every fresh autonomous session was hardwired `CODEX_PREFERRED`. Execution accounting showed Terra/high and `reachedLimit=false`, but did not capture reset/credit metadata, so it did not satisfy the standing `>2 resets` threshold. The session was cancelled after exactly one provider turn; no second Codex turn and no review-inbox record were produced.

The older delegated local-Qwen review path was also attempted. Its worker never started; the generated RUN_START approval was later recovered from the durable journal, but the run had already been safely moved to `CANCEL_REQUESTED`. It remains audit history and must not be resurrected. Restart reconciliation is expected to terminalize this stranded read-only cancellation.

## Deployment boundary

This R5 source change is NOT deployed. Wake dev.20 remains the live Wake package and is unaffected. Do not hot-reload a root binary built from the cumulative dirty worktree merely to activate this route. Prefer an isolated/review-bound serving candidate or another supported path that preserves reviewed source lineage. After the route is safely served, create a fresh genuine autonomous review with `mode=chatgpt_web_qwen_autonomous`; require local Qwen execution, normal independent final review, natural generation-10 Wake discovery, a persisted USER message in the canonical chat, and durable correlated `EXACT_USER_MESSAGE_APPENDED`.
