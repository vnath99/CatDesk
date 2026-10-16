# T-0485 R33 — Queue review and candidate health source integration

Date: 2026-10-16. Status: `TESTED_LOCAL`; overall October CatDesk MVP remains `PARTIAL`.

## Exact source and behavior

The `orchestrator/chatgpt-codex-autonomous-loop` branch integrated three independently source-reviewed commits on base `f098b34a6f263337149d2fb9b9b4b03363a1a7a3`:

| Integrated commit | Source commit | Change |
| --- | --- | --- |
| `1121b2fba5790861907ce2362fcccb252df9b4eb` | `75fe6754d8b8c913ebc5a1d2e3e02ae5e0015b1e` | Candidate parent probes the authenticated isolated MCP route every 15 seconds with a six-second bound, alongside its one-second listener PID check. |
| `52031093f08b13cc34c312cc0bf06855b7692dba` | `e762176d0b43e9aa3ce1a34382b98ef4ad0c1af9` | Queue refuses dirty or unborn Git baselines after validating the exact attempt marker and before writing a writer marker or spawning Codex. A child-absent receipt moves that job to `ATTENTION`, releases its exact scheduler lease, and leaves the next clean project eligible. |
| `911dc61575540b2f83ce636d93aa61911ceaef76` | `34295ee428305dcc98c022aceb43d5690a68863b` | Local interactive `review-disclosure-renew` opens another exact-package, one-hour shared-MCP read window. It keeps create-once hash-linked history, validates current registration and immutable package under receipt, queue and registration locks, bounds the receipt-directory scan, and cannot override a revocation tombstone. |

The integrated source tree is `714dac1f9df25f9e13319895feda2ccc38c72178`. Candidate owner failure exits through its existing kill-on-close Job and leaves the prior serving journal for read-only diagnosis; this change does not write a new runtime-attention record or prove live orphan classification. The queue guard does not reconcile historical claims or run an actual project job. A renewal discloses evidence to a shared principal; it does not record independent review, ACK a chat, authorize Wake, or start another task. An MCP read already in progress may finish after a revocation is published; expiry uses the local wall clock.

## Verification on the integrated tree

- `cargo test --locked --workspace --all-features --quiet -- --test-threads=1` exited 0 with 1,383 main-binary tests passed, zero failed and 28 ignored. Every enabled integration target passed. The nested AppContainer denial printed an expected child-process failure while its enclosing test passed.
- `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`, `git diff f098b34 HEAD --check`, and `cargo build --locked --all-features --bin catdesk` exited 0. The built debug image SHA-256 is `4AB3F948B2AE2862D331A8DFCEB25545C489B673E2EF25609C36079F1FD4C664`.
- Before integration, the candidate focused suite passed 14/14; the corrected queue suite passed 49/49, including marker precedence and dirty-project failure isolation; renewal reader tests passed 8/8. Independent reviewers passed each final source commit. A default-sandbox isolated suite had temporary registry access-denied failures; the exact integrated full run above used normal host filesystem access and passed.
- With the exact built image and normal Git identity access, read-only actual Desktop `codex status` reported authenticated `codex-cli 0.160.0`, `NEEDS_OWNER_RECOVERY`, 35 historical queue blockers, 85 accounting review rows, no pending writer marker and `launchAuthorized:false`. `codex queue status` reported `jobs:[]`. A restricted sandbox read of those commands returned `CODEX_SCOPE`/`CODEX_QUEUE_SCOPE` Git-identity errors; the normal-access readback is the applicable observation.

The main worktree's nine preexisting untracked files were preserved. No actual project queue job, live candidate, installed daemon, protected Wake package, Wake event, target, external tunnel or GitHub Actions setting was changed. Exact-source remote CI, installed recovery, independent ChatGPT review and real three-project acceptance remain open. The CI workflow has push and pull-request triggers but no `workflow_dispatch`; the historical HTTP 422 disabled-Actions response is a separate observation. No dispatch retry was made.
