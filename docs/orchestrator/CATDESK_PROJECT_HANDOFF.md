# CatDesk Project Handoff and Continuity Guide

This is the starting point for a new ChatGPT conversation or a newly assigned
maintainer. It records the current durable project state without granting new
authority. Read it before selecting a ticket or operating a local lifecycle
surface.

## Current checkpoint — 2026-10-07

This section supersedes older operating-picture text below when the two disagree.

- Canonical CatDesk conversation: `https://chatgpt.com/c/6ac63f72-a2ac-83e9-bf61-db8ab9d97224`.
- The paired CatDesk project + independent Wake authority is generation 30, digest `bd55d92e8e4e29b0fcb722f7826b0d61c215b93b539b8ec782119fa1c1b8b9f3`. Canonical rollovers use the established CatDesk-only `DESIGNATED_CHAT_TARGET_URL=` CAS transaction, not registry-only or standalone Wake setters.
- The GitHub recovery mirror milestone is complete. Current branch `orchestrator/chatgpt-codex-autonomous-loop` local HEAD and remote HEAD were freshly revalidated equal at `b9753da74b5245c5f01b2b9748a20b634dc1c86a`; a direct push dry-run reports `Everything up-to-date`.
- Git and Codex are functionally live through CatDesk: direct `git` commands execute and `codex --version` reports `codex-cli 0.160.0`. The remaining daemon `identity.gitCommit=unknown` / reviewed-reload `INVALID_ARGUMENT` issue is provenance/control-plane hardening, not evidence that the Git/Codex deployment failed.
- Five local untracked diagnostic/review artifacts remain intentionally preserved; consult `.catdesk/current_plan.md` before deleting or publishing anything.
- Immediate gate: before recovery implementation, the operator connects the ChatGPT GitHub connector and ChatGPT verifies private off-host repo access. Then proceed in order: runtime/recovery convergence -> current Wake selector/reconciliation attention -> Binagotchy CLI/TUI + one-command diagnostics -> multi-project scheduling/wake fairness.
- Every substantive turn uses the first-class Wake TurnTimer; checkpoint by 19 minutes. Exactly one hourly deadman is enabled for the generation-30 canonical chat; predecessor CatDesk deadmen are disabled. Future delegated engineering uses Codex 6 Astra MEDIUM/LOW only with more than two resets remaining, otherwise GPT-5.6 Terra HIGH; when no resets/cloud usage remain, try Qwen and fall back to direct ChatGPT/CatDesk work if Qwen is inadequate.

## Canonical sources and precedence

| Purpose | Canonical source | How to use it |
| --- | --- | --- |
| Milestone status and acceptance boundary | [`CATDESK_MILESTONES.md`](../../CATDESK_MILESTONES.md) | The single current milestone/status source. |
| Active work and live-control state | [`.catdesk/current_plan.md`](../../.catdesk/current_plan.md) | Read before resuming work; it identifies the bound control state and next safe work. |
| Durable ticket queue and historical intent | [`.catdesk/todo.md`](../../.catdesk/todo.md) | Select only an eligible, scoped ticket. |
| Current lifecycle/security architecture | [`CANONICAL_CURRENT_ARCHITECTURE.md`](CANONICAL_CURRENT_ARCHITECTURE.md) | Current operational design and ownership model. |
| Implementation evidence | [`review_bundles/`](review_bundles/) | Evidence for a named ticket only; never use a historical bundle to override a current source. |

If these sources disagree, stop and reconcile the discrepancy against the
milestone tracker and current plan before mutating source or host state.

## Current operating picture

- The stable CatDesk control transport is `CONNECTED_VERIFIED` at the current
  reconciliation boundary, and the already-running official Secure MCP runtime
  remains externally owned.
- This is the successor control conversation after the prior CatDesk chat filled
  its context. Exactly one new ChatGPT hourly CatDesk deadman is enabled for this
  successor chat at the top of every hour; predecessor deadmen remain disabled.
- The exact successor control chat is now known and operator-authorized:
  `https://chatgpt.com/c/6aa2d008-3b78-83e9-b4ce-9dddabfdfa2b`, SHA-256
  `4e0b61904268640353ad891a9b7b6e58c29c914a3adece36c72806e78dc90da7`.
  Fresh 2026-09-10 readback still shows the live CatDesk project registry on
  predecessor `https://chatgpt.com/c/6a976557-1054-83ea-b4c3-b10bb51b4800`,
  SHA-256 `c16f6d1e834c3d1d530e3cc842be09021df2b029b3e94c44b272e80b85f5e911`.
  Current source now provides a cached-schema CatDesk-only
  `DESIGNATED_CHAT_TARGET_URL=` bridge that delegates to the already-reviewed
  paired project-target + wake-target CAS/readback transaction; focused paired
  and refusal tests pass. The serving 77-tool legacy generation cannot consume
  this new form yet, so event-driven wake remains parked until reviewed serving
  parity deploys it. Do not use the older registry-only `CHAT_TARGET_URL=` form,
  direct-edit protected wake state, or
  manually fire Selenium/browser wake.
- The cumulative T-0319 recovery chain is independently **ACCEPTED** through
  the separate Codex/Terra-high T-0363 durable verdict. That reviewer explicitly
  rechecked the T-0359 restart-worker identity repair and the T-0362 stale-daemon
  production-enumeration repair; focused recovery acceptance passed 3/3 and the
  reviewed candidate verification passed 897 tests.
- Do not start another recovery reviewer or hardening loop absent a new concrete
  reproducible defect. T-0324 reviewed deployment/source parity is now the
  highest-priority core blocker.
- A legacy `CatDesk_Local` route may return HTTP 404. It is not authority to
  replace, restart, or mutate the healthy stable tunnel.
- The official OpenAI Secure MCP tunnel/client is externally and operator
  owned. CatDesk must not create, duplicate, configure, stop, or remove it.

The [current architecture document](CANONICAL_CURRENT_ARCHITECTURE.md) covers
the ordinary lifecycle façade, redacted status model, review inbox, and legacy
recovery constraints in more depth. This handoff adds the milestone-specific
continuation boundary.

## Verified architecture and ownership boundaries

```text
Operator-designated ChatGPT conversation
  -> externally owned official Secure MCP runtime
  -> CatDesk loopback control transport
  -> canonical Codex app-server / exact provider thread
  -> durable task, accounting, review, and continuity state
  -> bounded review-event wake path (when separately eligible)
```

### Provider and thread continuity

Codex is preferred while eligible. A confirmed Codex allowance/credit
exhaustion is a provider-route transition, not a new logical task:

1. Persist the provider-attested reset boundary and preserve the exact Codex
   continuity/thread metadata.
2. Hand the same logical session/task to healthy contract-approved local Qwen.
3. Keep Qwen sticky and make no Codex probe before that boundary.
4. At or after the boundary, restore only the preserved exact Codex thread at a
   safe provider/task boundary.

If Codex remains exhausted, persist the refreshed boundary and remain on (or
return to) Qwen. A transient Codex 429 remains bounded Codex backoff; an
unavailable Qwen fails closed to ChatGPT. No cloud, paid, browser, or remote
fallback is permitted. The accepted evidence is [T-0284's reset restoration
bundle](review_bundles/T-0284_T0030_R1_CODEX_RESET_TIMESTAMP_RESTORATION_REVIEW_BUNDLE.md).

The current Codex GUI/CLI revalidation uses the app-server's supported
metadata/turn APIs and durable `providerThreadId`, `expectedCodexThreadId`, and
continuity metadata. It does not use browser scraping, browser wake, or a
ChatGPT conversation URL as its thread identity. T-0285 reached deterministic
`COMPLETED_VERIFIED`; its bundle still requests independent T-0142 review
before treating operator-visible GUI/CLI interoperability as accepted. See the
[T-0285 bundle](review_bundles/T-0285_T0142_CODEX_GUI_CLI_CURRENT_THREAD_REVALIDATION_REVIEW_BUNDLE.md).

### Stable control plane

T-0223's accepted source chain preserves a version-independent local control
plane: a fixed local `127.0.0.1:3201` supervisor front door, a fixed private
Windows control pipe, and an ordinary versioned worker at exact
`127.0.0.1:3200`. The pipe admission model derives the supervisor's expected
principal from OS token evidence and requires exact `TokenUser` plus
`TokenSessionId` equality before registration decoding; executable/image,
reviewed-manifest, generation-CAS, and old-backend-preservation checks remain
downstream requirements.

The protected supervisor install/state and native startup transaction work are
accepted through T-0281. That does **not** mean host-live activation is
accepted: the remaining T-0223 work is separately reviewed, operator/live-host
acceptance through the closed lifecycle path only. No ad-hoc ProgramData,
service, Scheduler, pipe, port, shell, or direct-state operation is a valid
substitute. Secure MCP/tunnel ownership remains external.

### Wake and deadman boundary

The durable review inbox is the canonical wake-event authority. A wake is a
bounded, idempotent response to the newest unread actionable event for the
exact session; it is not a general-purpose browser control channel. Do not
manually invoke browser wake to manufacture evidence.

Historical T-0224 natural-delivery acceptance remains evidence for the reviewed
wake design, but the current control-chat replacement creates a newer target
coherence boundary. The current hourly deadman is safely bound to the new chat.
The event-driven browser path must remain parked until the serving generation can
atomically reconcile the protected project target and wake target through the
reviewed paired CAS/readback authority. Direct selector/target editing,
fabricated review events, reuse of prior records, manual browser/Selenium
commands, and normal manual wake are not acceptance evidence. The deadman
fallback must never become a second normal browser-submit owner.

## Accepted state versus open acceptance

| Area | Durable state | Continuation rule |
| --- | --- | --- |
| T-0223 stable supervisor | Source chain accepted through T-0281; host-live acceptance parked. | Use only an accepted lifecycle ticket and reviewed closed operator surface for a future live step. |
| T-0224 wake | Historical natural-delivery acceptance closed by T-0298, but this 2026-09-10 successor chat is not yet coherently bound in protected project+wake target state. | Exactly one hourly deadman is active on this successor chat. Make event-driven rebinding the immediate continuity priority once the exact current URL is available; use only the reviewed paired target CAS/readback surface and never hand-edit target state or manufacture browser evidence. |
| Provider routing | Reset-aware Codex/Qwen routing is deterministically accepted through T-0284, but the serving generation still predates the corrected Qwen 3.8 tool-result continuation behavior; T-0324 is the deployment-parity gate. | Prefer actually usable Codex/Terra-high. Until T-0324 deploys accepted current source and reconciles T-0322, do not rely on Qwen for substantive implementation; bounded direct ChatGPT work remains permitted. |
| Completion attribution | T-0134 accepted. | Do not recapture post-turn baselines for a previously launched task whose baseline authority is missing/corrupt. |
| Lifecycle façade | T-0137 accepted. | Keep public parameter identity scoped through helper loading; use supported public lifecycle forms only. |
| Codex GUI ↔ CLI | T-0285 deterministic revalidation complete; independent T-0142 review pending. | Do not create/select a GUI conversation or invoke browser wake as proof. |
| Windows GUI | Implemented in current source; live host/deployed-release visibility/taskbar acceptance remains. | Keep behind T-0223. T-0224 is already accepted. The next T-0222 proof must capture the exact canonical release/mode that launched because the 2026-09-07 operator screenshot showed the older terminal UI despite newer GUI source. |
| Multi-project operation | Plumbing exists; live concurrency/isolation acceptance remains. | Preserve one writer per workspace and exact workspace/thread separation. |
| Routine release/audit governance | T-0324 still proves deployment-generation skew, and T-0372 preserves the exact protected-image readback. The operator has now rejected recurring product-root/UAC ceremonies as the ordinary release path; T-0373 is preserved but parked as fallback. | ChatGPT selects exact reviewed candidates. CatDesk should automatically bind immutable source/build/review hashes and purpose-separated per-user audit attestations/signatures where useful, then atomically route versioned workers behind a stable user-scope control host with rollback. Preserve the external product root/private-key only for rare bootstrap/trust-root/stable-host authority; routine updates must not make the operator a signing clerk. |

## Safe continuation procedure

1. Read the four canonical sources in the precedence table and inspect a narrow
   Git status/diff for attribution only. Preserve the intentionally dirty
   worktree; never reset, clean, or absorb unrelated changes.
2. Confirm there is no active autonomous/delegated worker and that the selected
   task is the next eligible milestone item. Do not create a replacement
   session merely because a provider is exhausted.
3. Read the exact accepted/rejected review bundles named by that task. Treat
   controller verification as evidence, not independent acceptance, when the
   milestone says a boundary is still pending.
4. Keep all normal work inside the canonical workspace/project and existing
   provider-thread binding. Use durable app-server/CLI continuity metadata, not
   browser navigation, to resume Codex.
5. Make only the mutations authorized by the active task contract. If a live
   host, operator, browser, scheduler, tunnel, or identity action is not
   explicitly authorized, park it and document the precise blocker instead of
   inventing a workaround.
6. Run the task's required verification, record only attributable changes, and
   produce the exact requested review bundle. Request independent review before
   changing an acceptance state.

## Operator-only and prohibited boundaries

The routine public Windows lifecycle façade is `catdesk.ps1`; it exposes fixed
consumer operations and redacted status. Internal scripts, release promotion,
and production-acceptance tooling are not generic operator substitutes. When a
future reviewed ticket specifically permits a host action, use its one closed
surface only and preserve its elevation/operator boundary.

The following are not normal continuation tools:

- direct ProgramData, state, selector, Scheduler/service, pipe, listener, or
  task mutation;
- arbitrary shell/PowerShell substitutions for a reviewed lifecycle action;
- browser wake, ad-hoc Selenium, browser-profile, or ChatGPT-target changes;
- external Secure MCP/tunnel mutation or duplicate runtime creation;
- Git publication, external product-root/private-key signing, speculative
  provenance/dedicated-producer expansion, credential access, or fallback-provider
  expansion. Purpose-separated CatDesk-owned routine audit attestations are allowed
  only through the reviewed self-service release design.

## Remaining eligible core work

After this documentation reconciliation, keep the milestone-closing sequence
narrow and evidence driven:

1. T-0319 is independently accepted through T-0363. Preserve that closure and
   do not invent another recovery-hardening/re-review ticket absent a new concrete
   reproducible defect.
2. Treat exact-current-chat event-driven wake restoration as the immediate continuity priority. Obtain the successor chat's exact `/c/<id>` URL, then use only the reviewed paired project-target + wake-target CAS/readback authority and prove one fresh ordinary natural delivery. The hourly deadman remains fallback only.
3. T-0324 reviewed deployment/source parity remains the active runtime gate, but T-0373's Program Files/UAC rotation is no longer the normal path. Build the self-service release model around the accepted version-independent supervisor/wake foundations: stable per-user control host, user-owned versioned worker images, immutable source/build/review manifests, optional purpose-separated CatDesk audit signature, atomic version switch, readiness proof, and rollback. Preserve the existing product-root private-key isolation and do not bless mutable `target/release`, manually copy binaries, use raw legacy reload, or fabricate trust authority.
4. On the new serving generation, verify T-0322 safely reconciles its stranded
   `CANCEL_REQUESTED` owner, then run the bounded Qwen 3.8 continuation canary.
5. Finish T-0223 stable-supervisor host-live acceptance through the revised
   low-friction user-scope lifecycle, preserving the exact principal/runtime and
   rollback invariants already accepted in source.
6. Complete T-0222/T-0139 visible native GUI acceptance, including exact
   deployed canonical release/mode and literal visible-window/taskbar evidence.
7. Run T-0152 unified final acceptance, followed by T-0155 resilience soak.
   Resume broader Driver/BYOVD or Bug Bounty use only after the corresponding
   core stability threshold is satisfied.

T-0143 itself closes the documentation/handoff milestone only after this
document and the README navigation are independently reviewed. It does not
close any of the live acceptance rows above.

## Evidence map

- [T-0360 stale-daemon bounded inventory enumeration repair](review_bundles/T-0360_STALE_DAEMON_INVENTORY_ENUMERATION_REPAIR_REVIEW_BUNDLE.md)
- [T-0319 cumulative recovery acceptance bundle](review_bundles/T-0319_WATCHDOG_AND_STALE_DAEMON_RECOVERY_ACCEPTANCE_REVIEW_BUNDLE.md)
- [T-0281 native startup ABI and atomic rollback](review_bundles/T-0281_T0223_R4E_R2_NATIVE_STARTUP_EXACT_ABI_ATOMIC_ROLLBACK_COMPLETION_REVIEW_BUNDLE.md)
- [T-0284 Codex reset timestamp and thread restoration](review_bundles/T-0284_T0030_R1_CODEX_RESET_TIMESTAMP_RESTORATION_REVIEW_BUNDLE.md)
- [T-0285 current Codex GUI/CLI thread revalidation](review_bundles/T-0285_T0142_CODEX_GUI_CLI_CURRENT_THREAD_REVALIDATION_REVIEW_BUNDLE.md)
- [T-0267 single-writer wake source/preflight](review_bundles/T-0267_T0224_R1B_R3H_FINAL_HOST_LIVE_SINGLE_WRITER_W13_CANARY_RESTART_ACCEPTANCE_REVIEW_BUNDLE.md)
- [T-0134 attribution continuity and dangling-link hardening](review_bundles/T-0123_R2_ATTRIBUTION_CONTINUITY_DANGLING_LINK_HARDENING_REVIEW_BUNDLE.md)
- [T-0137 lifecycle parameter scope hardening](review_bundles/T-0137_LIFECYCLE_PARAMETER_SCOPE_HARDENING_REVIEW_BUNDLE.md)

Use the milestone tracker for the current status of this evidence; the bundles
are not a license to execute their host-live procedures outside a currently
approved task.

## T-0362 recovery-inventory checkpoint

The T-0361 stale-daemon fixture rejection was traced to user-level WMI access:
both CIM and legacy WMI process reads for the fixture PID returned `Access
denied`, which the old helper suppressed as an empty result. T-0362 replaces
only that read-only observation with fixed limited-information process handles
and native loopback TCP-table rows. The parent remains the sole mutation
authority and still requires exact daemon token, canonical path/SHA-256,
creation, pinned process handle, listener continuity, and all ambiguity
refusals. The relevant daemon set keeps its one-extra 65-row refusal and the
raw same-image source has a 257-row fail-closed ceiling. A fresh separate
T-0319 verdict is required; no recovery, tunnel, wake, Scheduler, or target
action was performed.
