# T-0036-R6 Fresh Project-Scoped Wake Canary Review Bundle

## Reviewed live candidate

The reviewed live R5 candidate is `target/t0036-r5-candidate/release/catdesk.exe` with SHA-256:

`3b10b52c5cb47e91733f9c621855bce4c5685b4b7f14029d0b9d43a5acd0f76f`

This worker made no source, wake-config/state/profile, registry, daemon, release, tunnel, Scheduler, external-project, or Git change.

## Reviewed R5 contract

R5 fixes the CatDesk-only migration compatibility defect by permitting the existing absolute dedicated profile path only when its safe canonical directory is contained within the exact canonical `.catdesk/wake-bridge` root. Relative in-root profiles remain supported. Outside-root paths, parent traversal, symlink/reparse ambiguity, malformed or oversized configuration, and other unsafe forms fail closed.

The migration remains CatDesk-only and durable: it persists the validated legacy local target into the canonical CatDesk project record only when unbound, never migrates external projects, never overwrites an existing project target, and supplies no global fallback. Project-scoped routing therefore selects only the durable target associated with the actionable review's project.

## CatDesk-owned automatic proof

Final live proof is owned solely by CatDesk's normal automatic project-scoped dispatcher after `COMPLETED_VERIFIED`. CatDesk must establish the durable canonical CatDesk target binding, then evaluate the fresh record's W13/R7 outcome: wake schema 4, `SENT`, positive `browser_sent_at_unix`, receipt schema 1, exact record/message/target hashes, target SHA-256 `acbac6dc7101db8e3dfff84d0d043e497f89cb31964c1e1d7f071eef4e5737d4`, and no duplicate submit.

The failed T-0093 record is excluded: it must not be retried or reused. No manual wake, browser action, app-server invocation, external registration, daemon/promotion, tunnel/Scheduler operation, or Qwen action was performed by this worker.
