# T-0403R2 Launch, Recovery, and Tunnel Consolidation

## Classification

`CONSOLIDATION_REPAIR_READY_FOR_INDEPENDENT_REVIEW`

This is a source-and-fixture result only. It does not establish a live tunnel
session, reload a daemon, or establish reviewed promotion or LKG authority.

## Routine-path map

- `catdesk.ps1` is the public lifecycle facade. It late-binds its workspace and
  fingerprint defaults, validates the canonical `start-catdesk-stack.ps1`
  identity, and explicitly preserves caller-bound values across dot-sourcing.
- `scripts/start-catdesk-stack.ps1` is the normal start/recover engine. It
  validates the canonical daemon/listener, calls the external official-runtime
  status surface through a bounded child, and requires both local MCP readiness
  and runtime `healthz`/`readyz` before reporting `CONNECTED_VERIFIED`.
- `scripts/catdesk-release-recovery.ps1` and
  `scripts/promote-reviewed-catdesk-build.ps1` are the reviewed canonical
  recovery path. Historical/manual helpers remain internal or diagnostic; they
  are not ordinary facade authority.

The start engine observes the external official runtime but does not own its
lifetime. A daemon replacement can therefore leave the runtime running; the
post-replacement status, health, and ready checks decide whether the pair has
reconverged. Fixed redacted gates distinguish client unavailable, command
failure, timeout, oversized/invalid status, not-ready, and endpoint failures.

## Tunnel-client evidence and bounded correction

The durable pre-upgrade status record showed tunnel-client `0.0.10` exposed the
required `runtimes connect/status/stop/rm` capability surface but did not prove
why the observed connector session terminated. The later `0.0.14` observation
is consequently correlation, not a version-causality finding. No version floor
was added: runtime capability and status validation remain the acceptance
boundary.

`scripts/install-openai-tunnel-client.ps1` selects the exact version-tagged
official archive, verifies its published checksum, and checks the runtime
command surface before installation. `scripts/setup-secure-mcp.ps1` now keeps
all direct tunnel-client lifecycle commands inside the existing bounded native
invocation boundary:

- explicit `stop` now uses `Invoke-BoundedTunnelClient` rather than an
  unbounded native invocation;
- a failed status command is represented only as
  `<redacted-status-command-failed>`, not as misleading "status output
  present";
- command output and native errors remain unreported, while the fixed failure
  state preserves actionable connector-session observability.

The route remains validated from the configuration in memory before `connect`;
redacted or encoded placeholder routes cannot be used as runtime targets.

## Recovery and bootstrap authority boundary

Reviewed promotion writes `CANONICAL_HANDOFF_PROVEN` only after it has verified
the canonical pair and listener handback. The recovery helper accepts that
record only when its candidate and canonical SHA-256 values exactly equal the
canonical file evidence, then may create/reuse a `reviewed_promotion` LKG.
`operational_verified`, daemon reload receipts, health, isolated build outputs,
and review prose cannot mint or advance LKG.

The independently verified temporary controller bootstrap described by
T-0395 remains execution-only evidence. It is deliberately distinct from a
reviewed-promotion candidate and cannot substitute for canonical LKG recovery.

## Attributable changes

- `scripts/setup-secure-mcp.ps1`: bounded stop invocation and fixed-vocabulary
  status-command outcome classification.
- `scripts/test-secure-mcp-route-validation.ps1`: validates the unavailable
  client classification and rejects unbounded `connect` or `stop` execution.
- `scripts/test-start-catdesk-stack.ps1`: makes the inherited-pipe regression
  deterministic by creating its descendant with `UseShellExecute = false`, so
  the bounded output-drain behavior is actually exercised on this host.
- this review bundle.

## Verification

- `powershell -NoProfile -ExecutionPolicy Bypass -File
  scripts/test-secure-mcp-route-validation.ps1`: passed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File
  scripts/test-start-catdesk-stack.ps1`: passed after the deterministic fixture
  correction.
- `scripts/test-catdesk-lifecycle.ps1`: passed.
- `scripts/test-catdesk-autostart-supervisor.ps1`: passed.
- `scripts/test-promote-reviewed-catdesk-build.ps1`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed (the pre-existing `could not canonicalize path C:\\Users\\Volap`
  warning was non-fatal).
- `git diff --check`: to be captured after this bundle.

## Residual risk and follow-up

The observed 0.10-to-0.14 session recovery needs a separately authorized live
operator observation to establish causality. That observation must use the
official external runtime and compare the bounded readiness gates before and
after a reviewed daemon action; it must not be replaced by a version guess or
by CatDesk-owned tunnel management. Historical/internal helpers should remain
documented as diagnostic-only until a separately reviewed deprecation/migration
removes them.

## Prohibited-action audit

No CatDesk start, recover, stop, daemon reload, tunnel install/connect/stop,
credential access, Wake mutation, reviewed-build/promotion action, canonical
release or LKG mutation, Git publication/history change, or external-project
mutation occurred. The PowerShell tests used source/temporary fixtures only.
