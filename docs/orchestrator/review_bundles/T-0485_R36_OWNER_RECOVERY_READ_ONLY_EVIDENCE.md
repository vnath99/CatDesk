# T-0485 R36 — Actual CatDesk claim recovery evidence map

Date: 2026-10-16. Status: `TESTED_LOCAL` for read-only diagnosis; live claim reconciliation remains `BLOCKED`.

## Exact observation

The control checkout was `orchestrator/chatgpt-codex-autonomous-loop` at `06f347d3e3bf9a87842f6c4d124ca4a9b65f00c4` (tree `9a11122271792b741b607175b2784c7b613ea114`). Its current `target/debug/catdesk.exe` SHA-256 was `201F306E4B795B2D466BFF14F6A7AAB0997873CC4DA3C604627F8F75982152EC`. With normal host Git identity access, both exact read-only commands exited zero:

```powershell
$root = 'C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop'
$cli = Join-Path $root 'target\debug\catdesk.exe'
& $cli codex recovery status  --project-id catdesk --workspace $root
& $cli codex recovery prepare --project-id catdesk --workspace $root
```

The full JSON was parsed locally and only aggregate values are reported here. Both commands returned `NEEDS_OWNER_RECOVERY`, `readOnly:true`, `launchAuthorized:false`, and diagnostic `evidenceSha256` `60b3ce930bcfcffbb7873fe3beaa83301c2c65099dc1f8d11a625a8aeb436d70`. The snapshot covered 594 sessions and 558 accounting records. There were zero session-state blockers, 35 unique in-flight queue claims, 85 accounting review rows, and no pending writer marker. The digest is diagnostic, not a compare-and-swap token; no OS writer lock was checked.

| Evidence group | Read-only count |
| --- | ---: |
| Queue claims | 30 `WORKER_RUNNING`, 5 `VERIFYING` |
| Claim-bearing session states | 29 `CANCELLED`, 5 `LEASE_EXPIRED`, 1 `PAUSED`; none `QUEUED` |
| Claim-linked accounting | 35 rows, all 35 unended; 13 with open spans |
| Other accounting review rows | 50 rows; 49 unended, 44 with open spans |
| Missing owner process identity / source-bound child exit | 35 / 35 |
| Incomplete Codex turn profile | 27 |
| Missing / mismatched Codex thread binding | 3 / 3 |
| Ineligible checkpoint session state / missing checkpoint Codex profile | 2 / 1 |

`prepare` classified the 35 candidate native diagnostics as 20 `ABSENT`, 6 `NO_CURRENT_HANDLE`, 3 `NO_EXACT_THREAD_BINDING`, 2 `NO_CODEX_HANDLE`, and 4 `EXACT_CURRENT_TURN_DIAGNOSTIC`. All four exact candidates recorded direct parent exit code zero; three included a thread-compatible `turn.completed`. The report marks `ownerIdentityProven:false` and `descendantExitProven:false` for every exact candidate. A parent exit or model event does not establish the original owner or absence of surviving descendants. No raw claim/session IDs, diagnostic path, arguments, stderr or content are published here.

## Source limits and decision

`src/operator_codex_cli/recovery.rs` is intentionally read-only. It bounds sessions at 1,024, report rows at 4,096, each candidate diagnostic at 256 KiB and all candidate diagnostics at 4 MiB; it checks files without following redirects. Its candidate digest is separate from the durable-state `evidenceSha256`, and the report warns that same-user parent replacement can race this nontransactional read. `CodexCliExecutionDiagnosticV1` records an exit code and events but no owner PID with creation identity or process-tree lifetime evidence. `AutonomousSessionSnapshotV1`, `AutonomousQueueTaskV1`, and the accounting record likewise do not bind a historical owner process identity.

`src/delegated/autonomous_controller.rs::reconcile_restart_task_state` only considers a `QUEUED` session with its current `WORKER_RUNNING` task and a unique unended accounting record showing an interrupted provider lifecycle without open or ambiguous reviewer ownership, tool-call effects, or thread drift. None of the 35 actual claim-bearing sessions is `QUEUED`; five tasks are `VERIFYING`. Reusing this helper would not cover the live claims. `src/operator_codex_cli.rs::ensure_no_interrupted_managed_writer` also checks every session/queue and then requires the full recovery report to have no blockers, so closing only 35 queue claims while leaving 50 other accounting rows would still deny direct Codex.

A source-guarded reconciliation interface could be implemented as a dormant, fail-closed path, but **no historical claim is presently eligible for a live clearance transaction** on recorded evidence. Treating terminal session labels, absent pending marker, four parent exits, `turn.completed`, elapsed time or an operator assertion as descendant/owner proof would weaken the writer boundary. The current report cannot authorize a batch reset, claim replay, accounting closure, or new Desktop turn.

**Smallest safe next step:** select one private claim hash from the four exact diagnostic candidates and obtain original-owner evidence for that exact session/task/turn: owner process identity including creation time, authenticated handoff, child and descendant quiescence, source/output disposition, and a supported accounting-span outcome. If that proof exists, independently review and fixture-test a one-claim, exclusive-lock compare-and-swap transaction with durable intent/commit and full readback. Preserve all other claims and accounting rows; if proof does not exist, keep the claim quarantined and seek an explicit scoped abandonment decision that retains its output and history. No such decision or mutation occurred in this audit.
