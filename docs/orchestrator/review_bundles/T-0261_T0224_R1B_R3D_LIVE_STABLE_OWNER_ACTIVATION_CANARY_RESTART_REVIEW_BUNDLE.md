# T-0261 / T-0224-R1B-R3D — stable-owner activation/canary preparation

## Status

`READY_FOR_HOST_ACCEPTANCE` is **not** claimed by this provider run because the
fixed project wake-venv Python fixture cannot start in this checkout.  The
configured venv reports that its base interpreter is missing:
`C:\\Users\\Volap\\AppData\\Local\\Programs\\Python\\Python312\\python.exe`.
No alternative interpreter, venv repair, real `.catdesk` change, or browser
operation was attempted.  This bundle is source/preflight evidence only; R1B
is not closed.

T-0260-R1 is the accepted behavioral base.  This slice makes the selection
gates explicit at both production dispatch boundaries and retains its reviewed
artifact / expected-old-owner CAS authority.

## Ownership and authority map

| Surface | Legacy selection | Rust selection |
| --- | --- | --- |
| Canonical event authority | `.catdesk/autonomy/review-inbox.json`; read-only | Same sole authority; read-only |
| Delivery authority | `.catdesk/wake-bridge/state.json`, schema 4 | Same sole authority, owned by `StableWakeDelivery` |
| Legacy entry | `mcp::handle_wake_bridge_run_once` starts fixed `scripts/wake_bridge.py` only after `selected_owner == legacy_python` | Returns `WAKE_OWNER_RUST_SELECTED` before resolving the record or subprocess |
| Direct legacy-script guard | Missing selector permits documented legacy default | Exact `rust`, malformed, or unknown selector returns before `actionable`, state, or browser-sink work |
| Rust entry | `catdesk-stable-wake-owner` calls `dispatch_if_rust_selected`, which rejects without a claim | `dispatch_if_rust_selected` opens the stable owner only after exact Rust selection |
| Atomic seam | fixed `.catdesk/wake-bridge/owner.json` schema 1, owner `legacy_python` | `activate_reviewed_rust_owner(workspace, LegacyPython)` holds the delivery kernel mutex, completes fixed preflight, CAS-writes `rust`, then parses the persisted readback |

There is no accepted selector state for a third owner or for both owners.  A
missing selector maps only to `legacy_python`; malformed selector authority
fails closed.  The Rust binary cannot claim while legacy is selected, and both
the MCP legacy launcher and the legacy script reject Rust selection before
state/browser work.

## Preserved preflight and delivery rules

- The activation API accepts only the expected current owner.  Owner executable,
  adapter, Python, target, profile, inbox, state, selector, and root are fixed
  product paths; no caller selects one.
- `stable_wake_owner_mode` retains T-0260-R1's no-follow reviewed-artifact
  evidence: `PinnedDirectory` plus `bind_reviewed_regular_artifact` binds the
  fixed owner executable and adapter by stable regular-file identity and bounded
  SHA-256 evidence.  Replacement, reparse/symlink, type, digest, and parent
  identity drift fail closed.
- Before selector persistence it parses the exact protected target/profile,
  canonical inbox, and schema-4 delivery state.  Conflicts, malformed records,
  unresolved `SUBMITTING`, and operator attention block activation.  Activation
  does not acknowledge, rewrite, or otherwise mutate inbox/config/state.
- The Rust owner still revalidates target/config/profile and unique canonical
  actionability immediately before its one stateless-adapter attempt.  It
  preserves W13 receipt schema 1 (record ID, normalized message digest, target
  digest, positive sent time), `CLAIMED -> SUBMITTING -> SENT`, permanent
  non-retry of unresolved `SUBMITTING`, and T-0252 canonical-only terminal
  retirement.

## Executable/source proof

Focused tests executed against isolated temporary workspaces:

| Evidence | Result |
| --- | --- |
| `cargo test stable_wake_owner -- --nocapture` | PASS — 14 tests, including legacy-mode Rust dispatch refusal, terminal SENT replay suppression, stale target/actionability refusal, unresolved/ambiguous no-retry, and concurrent adapter-at-most-once behavior. |
| `cargo test stable_wake_owner_mode -- --nocapture` | PASS — 7 tests: expected-old CAS/readback, stale/replay/opposite-mode refusal, idempotence after preflight, malformed selector, concurrent one-winner transition, old-or-new fault seam, reviewed artifact replacement. |
| `cargo test stable_wake_delivery -- --nocapture` | PASS — 12 tests: schema-4/receipt safety, immutable inbox/config, submitting ambiguity under 129 claims, kernel locking, and T-0252 retirement. |
| `cargo test --test stable_wake_delivery_lock -- --nocapture` | PASS — hard-kill lock release, two-process lock ownership, and two-process claim serialization. |
| Source scans | `mcp.rs` selects owner before fixed legacy launch; legacy script's `legacy_owner_selected` check precedes `actionable`; owner binary calls `dispatch_if_rust_selected`; exactly canonical inbox/state paths are referenced. |

The direct legacy script regression in `tests/test_wake_bridge.py` writes an
isolated Rust selector and then malformed selector, verifies return code 2,
zero browser-sink calls, and no state creation.  The Rust regression writes an
isolated exact Rust selector only after first proving missing/legacy selector
causes zero calls; SENT restart returns `AlreadySent` and retains one adapter
call.

## Verification truth

| Command | Result |
| --- | --- |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo build --bins` | PASS |
| `git diff --check` | PASS (the repository emits existing LF/CRLF advisories) |
| `cargo test` | BLOCKED at `stable_wake_adapter_python`: fixed venv base interpreter missing; all preceding Rust unit/integration suites completed, including 773 main-unit tests, 35 stable owner/delivery tests, and 2 recovery tests. |
| `cargo test --test stable_wake_adapter_python` | BLOCKED with the same fixed-venv interpreter error. |
| project `rust_full` | No separately runnable project command is defined; it is not represented as a fabricated pass. |

## Narrow attribution

The working tree was already broadly dirty (131 `git status --short` entries).
This slice's narrow source ownership is:

- `src/stable_wake_owner.rs`: selector-gated production dispatch and isolated
  terminal-SENT/mode regression.
- `src/bin/catdesk-stable-wake-owner.rs`: uses only the selector-gated dispatch
  entrypoint.
- `scripts/wake_bridge.py` and `tests/test_wake_bridge.py`: fixed-selector
  legacy eligibility guard before any state/browser work and isolated proof.
- This exact review bundle.

The preserved T-0260-R1 shared authority remains in
`src/stable_wake_owner_mode.rs`, `src/stable_wake_delivery.rs`, and
`src/windows_protected_fs.rs`; it is listed for audit context, not claimed as a
new replacement trust model.  `git diff --no-index` against `NUL` reports the
pre-existing untracked module sizes rather than a meaningful incremental diff,
so no misleading repository-wide line count is claimed.

## Prohibited actions not performed

No real `.catdesk` owner/config/state/inbox/profile mutation; no live Rust
activation; no legacy retirement on the host; no browser launch/type/click/send;
no daemon/release/tunnel/Scheduler/service operation; no target/profile change;
no Git publication, signing, or provenance mutation.

## Host acceptance procedure — explicitly NOT PERFORMED BY CODEX

After independent source review and repair of the fixed project wake venv only
by the authorized host workflow:

1. Re-run the fixed adapter fixture and the full verification profile.
2. Use the closed `catdesk-stable-wake-activate` surface with expected
   `legacy_python`; do not manually edit `owner.json` and do not supply paths.
3. Read back exact `rust`; verify the legacy MCP path refuses and no second
   writer is eligible.
4. Deliver one fresh ordinary W13 canary to the already protected exact target,
   then restart/reopen and prove the SENT receipt suppresses replay.
5. Preserve any post-submit ambiguity as unresolved `SUBMITTING`; do not retry.

Only that separate host procedure can advance this work beyond
`READY_FOR_HOST_ACCEPTANCE`.
