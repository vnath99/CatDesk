# T-0036-R4 CatDesk Project-Scoped Wake Canary Review Bundle

## Reviewed candidate

The reviewed live candidate is `target/t0036-r3-candidate/release/catdesk.exe` with SHA-256:

`5e8d4b2bfee1104994e15129b47c85ac3c0960b8aefb56e42f9545d3499b1b50`

This worker made no product, runtime, wake-config, registry, browser-profile, tunnel, Scheduler, external-repository, daemon, release, or Git change.

## Reviewed migration contract

For the canonical `catdesk` project only, the reviewed R3 implementation persists the validated legacy fixed local wake target into the canonical central-project record when that project target is unbound. The migration is durable and one-time: it does not overwrite an existing project target, does not migrate external projects, does not supply a runtime global fallback, and fails closed for malformed or unsafe local configuration.

Project-scoped dispatch continues to use the selected canonical project target. W13/R7 submission and schema-4 receipt checks retain exact review-record, message, and target binding; post-submit ambiguity is not retryable.

## CatDesk-owned live proof

Final live proof belongs exclusively to CatDesk's normal automatic project-scoped dispatcher after `COMPLETED_VERIFIED`. CatDesk must establish the durable CatDesk target migration and then evaluate the resulting schema-4 `SENT` receipt with a positive `browser_sent_at_unix`, receipt schema 1, exact record/message/target binding, and target SHA-256 `acbac6dc7101db8e3dfff84d0d043e497f89cb31964c1e1d7f071eef4e5737d4`.

No manual wake bridge invocation, browser action, registration/adoption control, daemon/release action, or retry was performed by this worker.
