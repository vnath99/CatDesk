# T-0297 Current Chat Wake Authority Handoff Reconciliation Review Bundle

## Scope and result

T-0297 reconciles the operator-authorized current CatDesk control chat through
read-only repository and protected-state inspection. It did not invoke a target
setter, guarded CAS, wake owner, browser/profile, supervisor, tunnel, or host
lifecycle surface.

## Read-only authority map

| Authority | Read-only evidence | Result |
| --- | --- | --- |
| CatDesk project registry | The sole `catdesk` row in `.catdesk/projects/projects.json` names `https://chatgpt.com/c/6a976557-1054-83ea-b4c3-b10bb51b4800` with SHA-256 `c16f6d1e834c3d1d530e3cc842be09021df2b029b3e94c44b272e80b85f5e911`. | Current project authority. |
| Protected effective wake configuration | `.catdesk/wake-bridge/config.json` has the same canonical `conversation_url`. It stores no separate target digest. | Effective wake URL converges with the registry. |
| Canonical effective digest | Read-only SHA-256 of the canonical effective URL is `c16f6d1e834c3d1d530e3cc842be09021df2b029b3e94c44b272e80b85f5e911`. | Matches the registry digest exactly. |
| Designated-target/preflight source | `operator_read_designated_chat_target` requires registry/effective-wake equality; `read_fixed_core_host_acceptance_preflight` supplies that current digest to receipt validation. | No hardcoded historical conversation URL selects acceptance authority. |

**Pair status: CONVERGED.** The reviewed guarded operator target-set/CAS action
is not needed for the current pair. If a future read-only check finds an actual
registry/effective-wake mismatch, that exact reviewed operator surface is the
narrow boundary; direct files, source tests, or browser actions are not repair
authority.

## Historical and stale-state handling

The T-0296 `6a932...` target and older T-0295 target/digest are historical
records only. They cannot override the newer operator-authorized current
readback. Bounded source search found no historical URL in production
target-selection logic. The durable milestone and plan now add T-0297's current
authority state, while T-0295/T-0296 records remain preserved as superseded
history. `CATDESK_NEW_CHAT_NOTES.txt` now explicitly instructs future chats
to use current coherent registry/effective-wake URL and digest, park a genuine
mismatch for the reviewed CAS, and retain existing local Git/gh authorization
guidance without re-asking for a separate ChatGPT connector.

## Deterministic evidence

No source defect was found, so no production repair was made.

| Existing bounded regression | Preserved evidence |
| --- | --- |
| `mcp::tests::designated_chat_target_readback_and_guarded_update_keep_project_and_wake_coherent` | An authorized fixture update changes both authorities and readback returns only the current value. |
| `mcp::tests::designated_chat_target_rejects_stale_or_preexisting_divergence_before_wake_mutation` | A stale request or existing mismatch refuses before wake mutation. |
| `core_host_acceptance_preflight::tests::structurally_sent_receipt_must_match_the_current_authoritative_target` | Receipt binding is to the evaluator's current authority input, not a permanent historical target. |
| `core_host_acceptance_preflight::tests::project_and_wake_target_disagreement_stays_a_deterministic_failure` | Real disagreement remains `FAILED_DETERMINISTIC_PREREQUISITE` before receipt acceptance. |

The tests use isolated fixtures. They do not modify the current project registry
or protected wake configuration.

## Acceptance impact and residual operator boundary

T-0224 remains open. The current target pair is already converged, so there is
no target-reconciliation action to take. The remaining operator boundary is
only the existing restoration of an authenticated ChatGPT session in the
configured CatDesk wake browser/profile. Afterwards, a **fresh ordinary**
CatDesk final-review event must naturally produce schema-4 `SENT`, positive
`browser_sent_at_unix`, receipt schema 1, and exact fresh
record/message/current-target binding. No source test, provider completion, or
historical receipt closes that gate.

The critical order remains:

```text
T-0224 -> T-0223 -> T-0222/T-0139 -> T-0152 -> T-0155
```

## Next safe overnight queue action

T-0298 is the next bounded, read-only T-0224 natural-delivery evidence
reconciliation. It must inspect only the fresh ordinary final-review record and
its durable schema-4 receipt after T-0297 completion. It must not retry or
invoke wake. If the receipt is absent or ends in login/CAPTCHA/security/
post-submit ambiguity, it records the exact operator boundary and parks it;
if exact current-target `SENT` evidence exists, it records the result without
fabricating later T-0223/T-0222/T-0152 acceptance.

## Verification

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --workspace --all-targets --all-features` | Passed: 882 unit tests plus all workspace integration targets. |
| `cargo build --workspace --all-targets --all-features` / `rust_full` | Passed; the all-target/all-feature profile satisfies configured `rust_full`. |
| `git diff --check` | Passed; accumulated dirty-tree CRLF warnings were non-fatal. |

## Attributable files and prohibited-action audit

T-0297 attribution is limited to:

- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `.catdesk/todo.md` status additions
- `CATDESK_NEW_CHAT_NOTES.txt`
- this exact bundle

The intentionally dirty worktree was preserved. No live target/config/registry,
browser/profile/wake, host/supervisor, Secure MCP/tunnel, external-project,
signing/provenance/dedicated-producer, branch, commit, push, or publication
action occurred.

**Status: READY_FOR_INDEPENDENT_REVIEW - CURRENT CHAT AUTHORITY CONVERGED;
T-0224 REMAINS OPEN FOR FRESH NATURAL CURRENT-TARGET DELIVERY EVIDENCE.**
