# T-0047B Canonical Codex Continuity + Auth Review Bundle

Status: `SOURCE_REPAIR_COMPLETE__HOST_ACCEPTANCE_PENDING`.

## Repair Applied

Canonical thread selection now follows the required durable trust hierarchy exactly:

1. a persisted CatDesk project binding for the exact canonical workspace;
2. otherwise, a recent exact-CWD `exec` thread whose ID is already recorded by a different `COMPLETED_VERIFIED` session for the same project and required Terra model.

The current session's `providerThreadId` is no longer accepted as bootstrap evidence. It can be stale or come from an interrupted task. If it conflicts with the selected canonical identity, binding still fails closed.

The host-side preflight continues to require metadata-only resume on that exact thread, direct-input/idle eligibility, exact CWD, and authoritative `gpt-5.6-terra` / `high` metadata. Account/rate-limit reads remain advisory: unavailable or malformed telemetry is persisted as bounded `UNKNOWN` state and does not itself indicate exhaustion or block a valid Terra/High thread.

Normal current-user Codex context remains the default. Optional executable and `CATDESK_CODEX_HOME` recovery overrides are path-only; their contents are not read or serialized.

## Focused Evidence

- New regression: `canonical_bootstrap_uses_only_project_or_completed_session_evidence`.
- Host-only `turn/start` continuity support uses fixed `gpt-5.6-terra` / `high`, `approvalPolicy=never`, and a read-only, no-network sandbox. It does not accept a caller-supplied prompt, model, effort, or sandbox.
- Step two is blocked until `thread/turns/list` finds the exact first turn completed on the same canonical thread. Bounded turn IDs and completion evidence persist separately from substantive provider-turn budgets.
- Existing regressions cover trusted `exec` pagination, exact-CWD matching, unloaded versus busy direct-input state, metadata-only resume, Terra/High rejection, and unavailable rate-limit telemetry.
- No nested `codex exec` or `codex app-server` probe was launched by this coding worker.

## Host Acceptance Still Required

CatDesk host must perform the approved live sequence after loading the verified daemon: resolve/bind and read back the exact project mapping, run the two harmless bounded same-thread continuity turns, capture the shared thread ID and turn counts before/after, and record supported pre/post rate-limit snapshots. This worker did not inspect credentials or launch those live operations, so no live thread ID, account data, or continuity-turn result is claimed here.

## Verification

- `cargo fmt --check`: PASS.
- `cargo test autonomy_runtime`: 2 passed.
- `cargo test codex_app_server`: 15 passed, 1 ignored operator-local live probe.
- `cargo test autonomy_state`: 15 passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --no-fail-fast`: 426 passed, 7 failed, 10 ignored. The failures are the pre-existing environment-sensitive advisor fixture and Windows process-tree cancellation cases; no canonical-continuity regression failed.
- `git diff --check`: PASS (only existing CRLF warnings on the dirty worktree).

CatDesk independently performs full verification and host acceptance.
