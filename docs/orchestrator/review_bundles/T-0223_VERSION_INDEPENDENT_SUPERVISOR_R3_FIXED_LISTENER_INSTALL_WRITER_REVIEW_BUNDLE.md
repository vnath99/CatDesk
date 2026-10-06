# T-0223 R3 — fixed listener and installer writer

## Scope and preserved boundary

R3 extends the accepted R1/R2 supervisor foundation without activating it. The
separately buildable `catdesk-control-plane-supervisor` remains distinct from
ordinary worker release artifacts. Worker promotion/reload has no authority
over `C:\ProgramData\CatDesk\ControlPlaneSupervisor`; the existing static
regression remains in `control_plane_supervisor.rs`.

No provider command installed a supervisor, bound the compiled production
listener, wrote ProgramData, reloaded a daemon, or created/stopped/configured
the externally owned Secure MCP runtime.

## Fixed front-door protocol

The compiled supervisor listener is `127.0.0.1:3201`, deliberately distinct
from the compiled worker endpoint `http://127.0.0.1:3200/mcp`. It accepts no
arguments. The only proxied path is `/mcp`; `/status` is the bounded local
readiness surface. There is no public registration, executable, URL, route,
manifest, shell, tunnel, credential, or header persistence API.

For every MCP request, the supervisor loads and validates the active route
once, including its registration generation, before sending bytes. A later
N→N+1 handoff or rollback therefore applies only to a later request. The
captured request either finishes against its selected backend or returns a
bounded non-secret transport failure; it is never half-routed.

POST, GET, and DELETE are forwarded with their status, body, and end-to-end
headers. Hop-by-hop headers (including `Connection`-nominated headers) are
filtered in both directions. Reqwest redirects are disabled. The front door
limits request bytes to 2 MiB, response bytes to 8 MiB, request lifetime and
connect time to 20 seconds, and concurrent proxy work to 16. It never logs or
persists request bodies, Authorization/security headers, credentials, or
tunnel secrets. Missing/crashed/invalid backend state returns bounded 503 or
502 responses while `/status` remains local.

The fixed worker URL and distinct supervisor port make a self-loop impossible
in production. A cfg(test)-only loopback endpoint seam exists solely for
ephemeral listener integration tests; it admits only `http://127.0.0.1:<non-
3201>/mcp`, is not present in production route validation, and carries no
caller-facing surface.

`REMOTE_ROUTE_DETACHED` is still independently derived from observed remote
404. A local backend can be ready while overall control-plane readiness is
detached. The supervisor observes/persists only a bounded non-secret tunnel
identity and never creates, stops, removes, reconfigures, or authenticates the
OpenAI Secure MCP runtime.

## Fixed-purpose install/update writer

`SupervisorInstallerWriterV1` has a fixed production constructor rooted at
`C:\ProgramData\CatDesk\ControlPlaneSupervisor`. It has no CLI or MCP
surface. Its crate-private future integration accepts only a reviewed-image
capability's already selected bytes plus its expected SHA-256; source path,
destination, executable name, service settings, and policy are not caller
inputs.

The writer validates the exact bytes, takes a create-new single-writer lock,
stages a fixed image under a digest-bound side-by-side directory, validates
the staged object and digest, then writes bounded current/LKG receipts. An
identical digest is idempotent; N→N+1 retains the previous receipt as LKG.
Temporary staging collision, malformed receipt, digest drift, non-regular
object, root replacement, or unsafe relative component fails closed. Tests
use an injected temporary root only. Startup registration remains a later
dry-run/R4 host operation: this R3 code does not create a task or service.

## Deterministic evidence

`cargo test control_plane_supervisor --no-fail-fast` passed: 13 tests in both
the main and separately compiled supervisor harnesses. Coverage includes
missing/crashed workers, manifest and process mismatch refusal, idempotent
registration, N→N+1/rollback/reload, remote 404, persisted tunnel identity,
state/root bounds, worker-reload non-ownership, actual ephemeral MCP
POST/GET/DELETE traversal, header filtering, missing backend errors, test
endpoint rejection, and digest-bound temporary-root install/idempotency/LKG.

## Attributable files

- `Cargo.toml` and `Cargo.lock` (Reqwest stream transport feature resolution)
- `src/control_plane_supervisor.rs`
- `src/bin/catdesk-control-plane-supervisor.rs`
- `docs/orchestrator/review_bundles/T-0223_VERSION_INDEPENDENT_SUPERVISOR_R3_FIXED_LISTENER_INSTALL_WRITER_REVIEW_BUNDLE.md`

Repository-wide dirty changes from prior tickets remain outside this R3
attribution boundary.

## R4 live-host acceptance

After independent review, an operator must: build and independently review
the supervisor image; invoke the administrator-owned fixed writer against the
fixed root; register the fixed startup plan; start the supervisor on 3201;
register a real attested worker; then prove missing/crash/handoff/rollback and
remote-404/reattachment continuity without changing the externally owned
Secure MCP runtime. That host activation is intentionally not performed here.
