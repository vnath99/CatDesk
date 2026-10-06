# T-0060F-W3 Live Wake Reconciliation Review Bundle

## Scope and conclusion

This repair does not launch a browser, reload or promote CatDesk, alter the
Secure MCP runtime, inspect browser storage, or modify the durable live wake
state. It repairs the two fail-closed gaps revealed by the W2 canary.

The W2 durable entry is contradictory: it is `CLAIMED` with
`SUBMIT_RECEIPT_UNPROVEN`. `CLAIMED` is a pre-submit retryable state, whereas
the latter is an unknown post-submit result. The same contradiction is present
for the preceding W1 review entry.

The current `scripts/wake_bridge.py` file has a modification time after the W1
and W2 durable attempt timestamps. Before this repair neither Rust launch path
recorded or supplied a bridge revision hash. Therefore the exact historical
script bytes cannot be proven retrospectively; no source writer other than the
bridge was found, but historical script identity was not an auditable property.
The former malformed-receipt branch in the current bridge also independently
allowed this exact invalid combination if a sink returned an invalid receipt
without first invoking `before_submit`.

## Changes

- `scripts/wake_bridge.py`
  - Requires `--bridge-sha256`, verifies it against the exact
    workspace-local `scripts/wake_bridge.py` path and bounded script bytes, and
    rejects copied, stale, or altered bridge invocations before state or browser
    access.
  - Reconciles any pre-existing `CLAIMED` record that has submit attention or
    receipt fields to `OPERATOR_ATTENTION` with
    `INVALID_CLAIMED_POST_SUBMIT_STATE`; it does not launch a retry.
  - Makes a malformed/missing return receipt become durable `SUBMITTING` with
    `SUBMIT_RECEIPT_UNPROVEN`, so it can never be treated as a safe claim.
  - Accepts the bounded nested CDP `result/value` reply shape used by current
    CDP adapters while retaining the exact user-author selector and bounded
    message count/size checks. This addresses the receipt query shape without
    broad DOM interaction.
- `src/delegated/autonomy_runtime.rs`
  - The automatic dispatcher hashes the bounded project-local bridge and passes
    that exact hash to Python. It refuses dispatch when the script cannot be
    bound.
- `src/mcp.rs`
  - The dedicated `catdesk_wake_bridge_run_once` wrapper uses the same bounded
    project-local hash binding, including dry-run invocation evidence.
- `tests/test_wake_bridge.py`
  - Covers copied/revision-mismatched bridge rejection, nested CDP results,
    malformed receipt reconciliation, and pre-existing
    `CLAIMED + SUBMIT_RECEIPT_UNPROVEN` rejection.

## Durable state-machine invariant

```text
CLAIMED       only before the submission boundary
SUBMITTING    written immediately before click; preserved on all uncertainty
SENT          only with schema-4 exact record/message/target/browser receipt
OPERATOR_ATTENTION
              for invalid historic state or pre-submit attention
```

Rust treats a bridge exit status as provisional and confirms it only from the
matching complete receipt. It does not write wake-bridge state. The two Rust
launchers now select the same path and provide the bridge byte digest that
Python verifies, eliminating undetectable copied/stale-script selection.

## Evidence and verification

- `cargo fmt -- --check`: passed after formatting.
- `cargo clippy --all-targets -- -D warnings`: passed.
- Focused Rust tests passed:
  - `automatic_dispatch_binds_a_bounded_exact_bridge_revision`
  - `dedicated_wake_wrapper_hashes_only_a_bounded_regular_bridge_file`
  - `dedicated_wake_dry_run_binds_the_project_local_bridge_hash`
- `git diff --check`: passed.
- Project-local Python wake tests could not run in this environment because
  `.catdesk/wake-bridge/venv/Scripts/python.exe` resolves to a missing base
  interpreter. The repair deliberately does not rebuild or replace that
  operator-owned environment.

## Remaining operator-only live sequence

After independent review and loading the reviewed candidate, use the normal
automatic autonomous review flow once. Confirm that the invocation carries the
current bridge hash, that the exact configured conversation is used, and that
the resulting record is either a complete `SENT` receipt or durable
`SUBMITTING`/`OPERATOR_ATTENTION`; no `CLAIMED` record may carry submit
attention or receipt metadata. Do not manually retry an uncertain submission.
