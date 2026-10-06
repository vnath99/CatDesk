# T-0320 — Recovery runtime-verification convergence

## Objective

Make `./catdesk.ps1 recover` converge without operator tunnel, PID, hash, or
sidecar intervention when the local canonical daemon is healthy and the
externally owned official runtime is briefly difficult to observe.

## Root cause being repaired

`Test-OfficialRuntimeVerified` caught all failures and returned `$false`. That
lost the distinction between a negative runtime state, a bounded command
timeout, oversized output, changed status schema, invalid dynamic health
reference, and HTTP readiness failure. Bootstrap subsequently had no reliable
reason to decide whether any local daemon action was appropriate.

## Safety contract

- Canonical binary plus sidecar remains the only local identity pair.
- LKG is written only after canonical pair, local MCP, and external runtime are
  positively verified; no current bytes are blessed after a failed probe.
- The recovery engine never owns, kills, or recreates tunnel-client/the existing
  external tunnel.
- A local CatDesk process remains stop authority only when the existing exact
  canonical-process rules prove it stale or when existing verified listener
  replacement rules apply.
- Public output contains fixed gate vocabulary only; no endpoint, path, alias,
  command output, credential, or exception detail is emitted.

## Planned changes

1. Add an internal runtime-verification result and fixed gate mapper.
2. Keep the existing `Test-OfficialRuntimeVerified` boolean compatibility
   surface, backed by that result.
3. Propagate the final failed gate through `Wait-FullStackReadiness`, engine
   status JSON, and the public facade parser.
4. Remove the branch that restarts a healthy canonical daemon merely because an
   external runtime probe is not yet verified; wait only within the fixed
   readiness deadline.
5. Add fixtures for current runtime-status field variants, dynamic health URL
   and file forms, timeout, oversized output, transient status failure,
   `healthz`, `readyz`, and repeated healthy recovery.

## Acceptance evidence required

- PowerShell recovery/lifecycle/promotion fixtures and Rust integration test.
- Static review that every wait/native command remains bounded and that no new
  tunnel ownership or arbitrary process authority was introduced.
- A fresh `./catdesk.ps1 recover` result of `CONNECTED_VERIFIED` on the host,
  followed by a repeated invocation with no duplicate canonical daemon or
  external tunnel.
- The later destructive daemon-loss drill remains T-0319 acceptance; T-0320
  does not fabricate that live proof.
