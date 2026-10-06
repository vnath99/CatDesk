# T-0248 / T-0224-R1A-R6 — Target Integrity and Runtime Independence

## Independent review request

Independent final review is requested. Controller state is advisory; review the
exact durable authorities, negative cases, and bounded verification evidence.

## Exact target/config authority

The existing protected target authority remains exactly:

`<workspace>/.catdesk/wake-bridge/config.json` → `conversation_url`

The pre-existing protected reader in `src/mcp.rs` validates canonical workspace
components, rejects final/intermediate unsafe link/reparse paths, bounds config
bytes, rejects duplicate JSON fields, validates the configured ChatGPT URL, and
does not inspect browser, daemon, tunnel, release, manifest, promotion, or LKG
state.

T-0248 adds a read-only digest-only interface over that exact reader:

`read_protected_wake_target` → opaque SHA-256 identity

`verify_protected_wake_target` accepts only an explicitly supplied opaque
digest and fails closed on mismatch. It never creates, selects, infers,
migrates, rewrites, or substitutes a conversation. Stable readiness reads then
revalidates the same protected target, so a change between reads fails closed.
Status exposes only `targetAvailable`, never the target URL.

## Canonical inbox remains unchanged

The only stable event authority remains
`<workspace>/.catdesk/autonomy/review-inbox.json`. T-0247’s producer-compatible
camelCase schema, bounded JSON array, safe nested references, deterministic
pending/stale classification, duplicate/conflict behavior, and read-only
semantics are preserved. No `.catdesk/stable-wake/review-events` authority or
producer is reintroduced.

## Production call path

`server::post_mcp` → `AppState::transport_status_payload` →
`stable_wake_bootstrap::workspace_readiness` →

1. canonical inbox `discover`, and
2. fixed protected target read + same-target digest revalidation.

The two durable inputs are evaluated independently: a target config failure
sets `targetAvailable=false` without consuming or hiding canonical inbox work.
A missing/replaced CatDesk binary, daemon/MCP/tunnel outage, release/manifest,
promotion, or LKG condition is not queried by this code.

## Attributable files

- `src/mcp.rs` — read-only protected target identity and explicit-digest
  verification helpers layered on the existing protected config reader.
- `src/stable_wake_bootstrap.rs` — target availability/revalidation, runtime
  independence regression, and deterministic target/inbox tests.
- `src/state.rs` — bounded `stableWake.targetAvailable` status field.
- `src/server.rs` — real transport-status fixture now includes protected target
  config and verifies target readiness.
- `docs/orchestrator/review_bundles/T-0248_T0224_R1A_R6_TARGET_CONFIG_INTEGRITY_RUNTIME_VERSION_INDEPENDENCE_REVIEW_BUNDLE.md` — this artifact.

The worktree was dirty before the slice. Some listed tracked files contain
unrelated pre-existing changes; task attribution is limited to the described
target-identity/readiness/test hunks and this bundle.

## Fail-closed matrix

| Condition | Stable result |
| --- | --- |
| protected config missing | `targetAvailable=false`; inbox remains readable |
| malformed/corrupted URL | `targetAvailable=false`; no target substitution |
| duplicate `conversation_url` JSON field | rejected by strict parser |
| explicit expected digest malformed | rejected |
| configured target differs from explicit digest | `wake target binding drifted` |
| valid target changed between readiness reads | same-target revalidation fails closed |
| canonical inbox missing/malformed/conflicting | `discoveryAvailable=false` |
| valid inbox + invalid target | pending data remains available; target remains unavailable |

The tests preserve config and inbox bytes around reads. No live configuration
was read or changed by the provider.

## Runtime/version-independence evidence

The focused durable-state test repeats the same read-only readiness result for:
target-release binary missing/replaced, daemon unavailable, MCP/tunnel
unavailable, reviewed-release manifest missing/mismatched, promotion/LKG
missing/mismatched, ordinary version transition, and restart reload from the
same durable inputs. A production-source regression rejects references to
target release, reviewed release, promotion, LKG, OpenAI tunnel, or daemon
reload authority in stable discovery.

## Verification evidence

| Check | Result |
| --- | --- |
| `cargo test stable_wake_bootstrap` | passed: 10 tests |
| `cargo test transport_status_tool_returns_redacted_status` | passed |
| `cargo fmt --check` | passed |
| `cargo clippy --all-targets --all-features -- -D warnings` | passed |
| `cargo build` | passed |
| main binary suite | passed: 725 passed, 21 ignored |
| supervisor binary suite | passed: 15 passed |
| `recovery_powershell` | passed: 2 passed |
| `t0215_measure` | passed: 2 passed |
| `t0217_release_measure_tmp` | passed: 0 tests |
| `git diff --check` | passed |

The repository Rust verification plan is fmt, test, and build. Test binaries
were run separately to keep execution bounded; no test gate was skipped.

## Prohibited mutations and R1B boundary

No browser delivery/wake, durable claim or receipt, target update, installation
ownership, ProgramData/Scheduler/service change, daemon/release promotion,
externally owned Secure MCP/tunnel mutation, Git publication, signing, or
provenance work occurred. Browser delivery, durable claim/receipt, stable host
installation, and externally owned Secure MCP continuity remain R1B work.
