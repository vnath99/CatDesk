# T-0056 GUI Live Orchestration Observability — Review Bundle

## Architecture

T-0056 adds `AutonomyObservabilitySnapshotV1`, a bounded, read-only UI model.
It is built from CatDesk's durable autonomous session, contract, review inbox,
wake-policy, and T-0055 accounting stores. `draw_ui` only renders the cached
snapshot; it does not parse autonomy files, call MCP, inspect process tables,
or read browser state.

The TUI refreshes the snapshot about every 750 ms through a bounded blocking
local read. It first copies workspace and redacted transport state while the
shared `AppState` lock is held, releases that lock for all filesystem work,
and publishes the result afterwards. The daemon remains independent of this
cache when the TUI is absent. Read errors produce a stale `UNKNOWN` snapshot.

## Evidence and precedence

The snapshot distinguishes `WORKING`, `WAITING`, `ATTENTION`, `IDLE`, and
`UNKNOWN`, with deterministic priority:

1. operator input, blocked state, or unread session review → `ATTENTION /
   OPERATOR`;
2. positive wake dispatch, verifier, provider, CatDesk transition, then exact
   target ChatGPT-generation lifecycle evidence → corresponding `WORKING`
   actor;
3. durable rate-limit, pause, queue, and ChatGPT-wait states → `WAITING`;
4. terminal/inactive session → `IDLE`; otherwise insufficient live evidence →
   `UNKNOWN`.

The production reader uses only a CatDesk-owned live wake-dispatch marker, a
durable verifier lifecycle state, and a durable provider-handle ownership token
for the corresponding live actors. It intentionally supplies no ChatGPT-Web
positive evidence: remote connector traffic, Chrome, window presence, and
wall-clock gaps never produce `CHATGPT_WEB` working status.

## Timing, layout, and redaction

The panel consumes T-0055's bounded work-time report. It renders known active,
wall, waiting, and evidence quality separately; it does not derive active time
from the legacy task elapsed value. ChatGPT timing remains
`UNKNOWN_NOT_OBSERVABLE` absent a positive persisted observation.

The existing status panel now has compact lines for autonomy/actor,
project-ticket-session, model/reasoning, active-wall-waiting timing, last/next
action, and wake/review state. Values are bounded and no endpoint, route,
tunnel ID, wake target, profile path, cookie, credential, prompt, response,
command, or process diagnostic is read or rendered.

## Changed files

- `src/delegated/autonomy_observability.rs`: bounded snapshot reader,
  precedence, timing projection, and deterministic tests.
- `src/delegated/mod.rs`: exposes the internal observability module.
- `src/state.rs`: UI-only cached snapshot field.
- `src/main.rs`: periodic lock-safe refresh and compact `Autonomy` status
  section.

No W13 submit/receipt, T-0054 wake policy execution, or T-0055 accounting
semantics were changed. The panel has no mutation controls.

## Local verification

```text
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test delegated::autonomy_observability::tests -- --nocapture
cargo test
git diff --check
```

The local full suite completed with 492 passed, 18 ignored, and no failures.
Independent CatDesk verification and authoritative diff capture remain
required before closure.
