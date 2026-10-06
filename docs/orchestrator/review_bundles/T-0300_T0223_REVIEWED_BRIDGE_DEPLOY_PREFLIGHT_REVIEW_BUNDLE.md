# T-0300 — reviewed bridge deploy / read-only preflight boundary

## Decision

T-0299 is accepted at its source boundary. T-0300 did not deploy the bridge or
call stable-supervisor lifecycle operations because this execution context has
no CatDesk MCP tool with which to invoke the only accepted deployment/reload
authority. No alternative shell, direct executable, PowerShell, SCM, Task
Scheduler, or ProgramData route was used.

## Before-runtime evidence

- The T-0300 contract supplies the bounded live discovery fact: the attached
  transport is the pre-T-0299 runtime with 77 tools; all three
  `catdesk_stable_supervisor_*` bridge tools are absent.
- No endpoint, token, credential, or tunnel detail was read or recorded.
- The current provider tool inventory contains no CatDesk MCP transport tool.
  This is why the runtime could not be queried, prepared, confirmed, reloaded,
  or rediscovered in this ticket.

## Accepted deployment authority

The reviewed path is intentionally two fixed first-class control-plane
operations, not a raw executable launch:

| Stage | Existing surface | Fixed proof |
| --- | --- | --- |
| Reviewed deployment | `catdesk_reviewed_build` PREPARE/CONFIRM | accepted acknowledged review authority, source snapshot/attestation, and fixed worker policy |
| Daemon handoff | `catdesk_daemon_reload` dry-run/confirm | canonical workspace candidate, exact SHA-256, short confirmation token, current daemon identity and fixed MCP port |
| Reconnect | existing daemon/tunnel relationship | daemon replacement reconnects to the already-running official tunnel; no tunnel action is requested |

The source requires candidate-hash revalidation at confirmation. It does not
authorize a caller-selected ProgramData destination, direct
`target\\release\\catdesk.exe` execution, generic shell copy/launch, SCM,
Task Scheduler, or a replacement lifecycle implementation.

## Candidate preparation

`cargo build --release --bin catdesk` was attempted under the approved Cargo
profile. The workspace's existing release `catdesk.exe` measured
`a264c8dc85fa55311e74114158f4f4cd598f6e9e9d0a6ca535e14d18aa649939`, but its
timestamp predates T-0299 and the bounded build command did not produce a
fresh certifiable replacement artifact in this context. It was therefore not
submitted to a deployment surface and cannot be treated as the reviewed T-0299
runtime.

## After-runtime and lifecycle evidence

There is no after-runtime evidence: no first-class deployment request could be
made, so no reconnect/discovery occurred. The following calls were deliberately
not made:

- `catdesk_stable_supervisor_status` `{}`
- `catdesk_stable_supervisor_preflight` `{}`
- `catdesk_stable_supervisor_activate` (prohibited in T-0300)

Consequently, T-0300 has no lifecycle result from which to classify
`READY_NON_ELEVATED`, `ELEVATION_REQUIRED`, or another lifecycle reason. The
exact bounded current blocker is **CatDesk MCP deployment transport unavailable
to this execution context**. This is not a product lifecycle category and must
not be misreported as `ELEVATION_REQUIRED`.

## Secure-MCP and prohibited-action audit

The external official Secure-MCP/tunnel runtime was neither stopped, restarted,
configured, nor inspected for credentials. No browser, wake, target/profile,
supervisor activation, protected receipt, ProgramData, Task Scheduler, SCM,
external-project, signing/provenance/dedicated-producer, or Git action occurred.

## Changed files and attribution

- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- this bundle

No product source changed. The intentionally dirty accumulated worktree was
preserved; this ticket's documentation attribution is limited to the files
above.

## Verification

- Read-only source inspection verified the closed reviewed-build and reload
  policy and the T-0299 bridge names/schema routing.
- `cargo build --release --bin catdesk` was attempted; it did not yield a
  fresh certifiable T-0299 replacement artifact within this execution context.
- `cargo test stable_supervisor_lifecycle --all-targets --all-features` —
  passed: 4 focused bridge tests.
- `cargo test daemon_reload --all-targets --all-features` — passed: 12 focused
  reload-policy tests.
- `cargo fmt --all -- --check` and `git diff --check` — passed. The worktree
  remains intentionally accumulated and dirty.

## Exact next host action

Use a CatDesk execution context that exposes its first-class MCP transport.
There, run the accepted reviewed-build PREPARE/CONFIRM and daemon-reload
preflight/confirmation flow, rediscover the three exact bridge tools and their
closed schemas, and call only status/preflight with `{}`. Park
`ELEVATION_REQUIRED` exactly if returned; otherwise carry the exact
fail-closed result into a separately authorized activation ticket. Do not call
activation in this T-0300 boundary.
