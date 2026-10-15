# T-0483 — Opt-in Codex Sol/medium deliverable-to-Wake cycle

Date: 2026-10-14. Canonical CatDesk Chat53: https://chatgpt.com/c/6ace36d3-8634-8333-a79c-1a4cba32caeb

## Request and acceptance

The operator requests **Codex GPT-6.1 Sol with medium reasoning effort** to work on a bounded CatDesk milestone, leave a concrete reviewable deliverable, have CatDesk emit an exact review event to Chat53, and have ChatGPT verify that deliverable before delegating the next ticket. The earlier hourly-fallback request was later superseded by the operator's decision to disable that automation.

**STATUS: NOT END-TO-END ACCEPTED.** An authenticated Codex CLI model probe succeeded, but no verified Sol/medium Codex-produced milestone deliverable, corresponding natural review_ready Wake, or latest-generation exact delivery receipt has been proven. The hourly ChatGPT fallback was **disabled at the operator's request on 2026-10-14**. Do not mislabel any synthetic manual debug event as a natural review wake.

**2026-10-14 source increment:** `docs/orchestrator/review_bundles/T-0483_R2_SOL_MEDIUM_OPT_IN_SOURCE.md` documents an opt-in exact Sol/medium contract/CLI/app-server/controller path. This is source-only; a matching Sol thread, actual worker deliverable, authentic review event, installed dev.85 and exact live Wake receipt remain required.

**R8 source increment:** Autonomous review same-ID replay now rejects changed project/state/reference while preserving first-write time and acknowledgment across crash retry. The controller test covers an inbox write surviving before the terminal transition. See `docs/orchestrator/review_bundles/T-0483_R8_REVIEW_INBOX_REPLAY_BINDING.md`. Immutable task output hashes and authoritative effective model/effort readback before review/Wake remain open; no live cycle is accepted.

**Subsequent source increments:** `T-0483_R4_SOL_REGISTRATION_PROFILE.md` adds explicit Sol/medium registration; `T-0483_R5_SOL_THREAD_ROTATION_SOURCE.md` adds a guarded, short-lived Terra-to-Sol canonical-thread rotation for a quiescent registered CatDesk workspace. Neither has been installed or exercised on the live project. The existing CatDesk binding remains Terra. The exact new commit has no independent Windows CI while GitHub Actions dispatch reports HTTP 422 (`Actions has been disabled for this repository`).

**P0 authority hold:** `docs/orchestrator/review_bundles/T-0483_R3_WAKE_STORE_AUTHORITY_PROVENANCE.md` records the connected-vs-direct Wake store mismatch. `T-0483_R6_WAKE_ROOT_OBJECT_PROVENANCE.md` adds a source-only hash of the opened Windows root directory object, plus a rebuilt inert dev.85 candidate. Resolve the live authority discrepancy and exact Windows CI outage before protected activation or a fresh canary. Earlier R1/R3 binary hashes are superseded by the R6 candidate; none is installed.

## Grounded state

- Official CatDesk secure tunnel: CONNECTED_VERIFIED. Local user Codex CLI 0.160.0 reports login via ChatGPT.
- CLI accepts `codex exec -m gpt-6.1-sol -c model_reasoning_effort=medium` and completed one non-mutating probe: thread `01a139f3-ccac-7d00-bca6-8ec83668f752`, structured `thread.started / turn.completed`. Catalog refresh emitted a timeout warning, but the probe turn completed.
- Later direct CLI attempts to initiate a substantive scoped task and `codex queue` through CatDesk run_command produced `INVALID_ARGUMENT` or timed out without a reliable task or deliverable receipt. **Do not claim these task launches succeeded.** The CLI help/login and model probe prove only those precise capabilities.
- The current source CatDesk autonomous contract in `src/delegated/autonomous_contract.rs` requires `gpt-5.6-terra`; `src/delegated/codex_cli.rs` only sets `model_reasoning_effort=high` for that model. App-server routing also references Terra/high. Do **not** quietly run a managed task under Terra/high to satisfy this Sol/medium request, and do not globally replace the old pinned model without scoped compatibility tests.
- Latest Wake source commit `7df27ee` passed independent Windows CI run `38443116318` (Rust, independent WakeHost Rust, Python Wake/advisor). Installed independent WakeHost still reports `1.0.0-dev.84`, target generation 34, exact Chat53 target and historical unresolved `SUBMIT_TARGET_DRIFT_CHROME_ERROR` for manual event `manual-wake-mcp-1791908511726`. The source candidate `dev.85` is not an installed reviewed package. Do not replay the ambiguous event or claim natural Wake acceptance.
- Tracked feature branch `orchestrator/chatgpt-codex-autonomous-loop` synchronized with GitHub at the current checked HEAD. Preserve the seven unrelated historical untracked files.
- The ChatGPT hourly automation `CatDesk Hourly Deadman Chat53` is **disabled by operator request**. Do not reenable it as part of this ticket.

## Scoped Codex task to delegate once the Sol/medium policy gate is satisfied

**Objective:** Independently review `dev.85` source/package readiness and prepare a concrete, non-destructive release-readiness review bundle, including source hashes, test results, the immutable package publication path in `wake/install.ps1`, current-vs-candidate divergence, the exact conditions for guarded installation, a duplicate-safe Chat53 canary, and rollback/recovery limits.

**Expected deliverable:** `docs/orchestrator/review_bundles/T-0483_R1_CODEX_DEV85_RELEASE_REVIEW.md` plus a bounded verification report. The worker may edit this review bundle only until reviewed. It must not modify the signed main image, private signer, Windows services, scheduled tasks, browser profile, independent WakeHost installed state, or official Secure MCP tunnel. Do not commit or push unrelated untracked files. ChatGPT should review the exact diff and tests before staging/pushing a scoped commit.

**Requested model:** `gpt-6.1-sol`, effort `medium`, provider `codex-cli`, with exact effective model/effort readback. Abort and report unavailable rather than silently changing the model.

## End-to-end contract for the recurring loop

1. Single-writer preflight: no OBSERVING manual turn, no active Codex session writing in this workspace, no current-generation in-flight browser submission and no contradictory Wake/project authority.
2. Validate the exact Codex model and effort, execute one bounded task, persist a unique task/session ID and artifacts, run scoped checks, and terminally record work as reviewed or blocked.
3. Have CatDesk produce one `review_ready` record *for that exact session/deliverable*. Persist its ID, source SHA, file list and verification outcome. Avoid faking a review-ready record with `catdesk_binagotchy_command(test)`; that is only a manual debug canary.
4. Have independent installed WakeHost deliver the event to the paired canonical Chat53 target with matching generation and SHA. Require exact current event `EXACT_USER_MESSAGE_APPENDED`, terminal `SENT`, turn timer `COMPLETE`, and no duplicate USER message. After the answer completes, browser should close cleanly.
5. Upon receiving the wake, ChatGPT reads the exact session/review bundle and Git state, reviews changes for correctness and scope, checks independent CI where required, ACKs/finalizes only supported records and delegates the next scoped task. If review fails, continue a bounded fix cycle.
6. The hourly fallback is disabled. If the operator separately enables it in the future, it must remain observation/continuation-only and never become a second Codex writer during another turn, review, or browser Wake.

## Implementation and validation gates

- Add **opt-in**, not global, Sol/medium model+effort handling to the CatDesk autonomous contract and Codex CLI/app-server telemetry and routing. Existing Terra/high sessions must remain valid unless deliberately migrated. Model selection must be read back from authoritative CLI turn metadata, not inferred from a command flag alone.
- Add regression tests for accepted Sol/medium, rejected silent downgrade, Terra/high backward compatibility, no unrelated project model changes, exact one-writer ownership, task-to-review ID binding, and no duplicate current-generation Wake.
- Verify actual Codex work in a safe branch and review the exact output. Do not assert completion merely because `codex --help` or a one-word probe succeeded.
- After independent Windows CI and reviewed package approval, deploy the immutable WakeHost through `wake/install.ps1` with version/hash-verified publication and atomic activation, then run exactly one fresh current-generation canary. Installation is **not** authorized by source CI alone.

## Operator and next continuation

Operator action currently: no credentials or GitHub install needed; direct Codex substantive launch through CatDesk is unresolved. If a protected WakeHost install or account-specific Codex approval is rejected by supported tools, surface the exact command and permission required rather than bypassing the gateway.

**Next autonomous step:** Check the current first-class timer, Codex process/session and source policy, then resolve the narrow Sol/medium opt-in route. Once available, delegate the above review task; do not generate a purported Codex deliverable wake before a real task/deliverable and review record exist.
