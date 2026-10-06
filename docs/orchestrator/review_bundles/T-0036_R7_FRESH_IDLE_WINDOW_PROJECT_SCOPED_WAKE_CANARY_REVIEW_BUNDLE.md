# T-0036-R7 Fresh Idle-Window Project-Scoped Wake Canary Review Bundle

## Bounded reviewed evidence

The reviewed R5 candidate remains `target/t0036-r5-candidate/release/catdesk.exe` with SHA-256 `3b10b52c5cb47e91733f9c621855bce4c5685b4b7f14029d0b9d43a5acd0f76f`.

Bounded central-registry inspection confirms that the canonical `catdesk` project already carries the exact project-scoped ChatGPT target and target SHA-256 `acbac6dc7101db8e3dfff84d0d043e497f89cb31964c1e1d7f071eef4e5737d4`.

## R5 migration and T-0095 control outcome

R5 successfully enabled CatDesk-only migration for the validated legacy target when the dedicated profile is an absolute safe canonical directory contained by the exact `.catdesk/wake-bridge` root. The migration remains durable, does not overwrite an existing target, does not migrate external projects, and provides no global fallback.

T-0095 ended at the preserved pre-submit `CHATGPT_NOT_IDLE` control: no submit boundary was crossed. That is not a project-routing regression. Another `CHATGPT_NOT_IDLE` after the established bounded readiness budget remains a fail-closed pre-submit control result.

## CatDesk-owned fresh proof

The fresh R7 record must be routed solely by its durable `projectId` to the exact CatDesk target; there is no global target fallback. Final live proof belongs only to CatDesk's normal automatic dispatcher after `COMPLETED_VERIFIED`, when this chat is idle.

CatDesk must then evaluate W13/R7 durable receipt requirements: schema 4, `SENT`, positive `browser_sent_at_unix`, receipt schema 1, exact record/message/target hashes, the exact CatDesk project target, and no duplicate submit. T-0095's record must not be retried or reused.

This worker performed no manual wake, browser action, wake-state/config/profile inspection, app-server action, external registration, daemon/release operation, tunnel/Scheduler action, or Qwen action.
