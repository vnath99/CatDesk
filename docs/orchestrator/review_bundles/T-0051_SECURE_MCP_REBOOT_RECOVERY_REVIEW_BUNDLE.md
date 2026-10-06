# T-0051 Secure MCP Reboot-Recovery Review Bundle

## Scope and ownership

This change repairs the pre-T-0040 secure MCP reboot-recovery gate. It does
not start, stop, reconnect, reconfigure, inspect, or otherwise disturb a live
CatDesk daemon or an externally owned official tunnel runtime. No credential,
route, tunnel ID, token, profile value, path to a sensitive runtime file, or
complete local MCP URL is recorded in this bundle.

The official runtime remains externally owned. `keep_runtime_on_catdesk_exit`
is unchanged, CatDesk does not call the official runtime stop/remove commands
from its monitor, and bounded recovery only uses the existing operator-local
environment references after local MCP readiness is available.

## Source review

- `scripts/setup-secure-mcp.ps1` retains `route_id` only in process memory for
  local target construction. Its JSON presentation continues to expose only
  `<redacted-local-mcp-url>`.
- The script validates the route before it checks credentials or invokes the
  client. Empty values, placeholders, literal redaction markers, and up to
  four layers of percent encoding are rejected. The accepted route alphabet is
  conservative and requires a 24--96 character route.
- `src/openai_tunnel.rs` obtains the authoritative health base from official
  runtime status JSON, with a bounded alias-scoped health URL file fallback.
  It accepts only credential-free loopback HTTP URLs, rejects query/fragment/
  userinfo and non-loopback values, and rejects the legacy fixed port.
- Dynamic readiness now requires successful `/healthz` and `/readyz`, official
  runtime running/healthy/ready flags, and local CatDesk MCP readiness before
  the monitor reports `CONNECTED_VERIFIED`.
- `src/ngrok.rs` no longer promotes an official runtime to verified from status
  flags alone, never falls back to configured admin UI URLs for official
  runtime health, waits for local MCP readiness before initial connect, and
  keeps recovery bounded by the existing cooldown/window/attempt limits.
- Legacy external-foreground configuration is left configured but unverified;
  it no longer probes a configured stale admin URL as authoritative runtime
  health.
- The official runtime file fallback accepts the observed alias-scoped state
  filename shape `health/catdesk-local.url` without hard-coding a user profile
  path. The file is bounded, cannot disclose its path or contents in a
  diagnostic, and still must contain a validated dynamic loopback HTTP base.
- `Get-LocalMcpRuntimeUrl` brackets `::1` so an IPv6 loopback target remains a
  valid URI.
- `-MigrateOfficialRuntime` is an explicit, idempotent operator action. It
  only changes a single validated `external_foreground` process mode under an
  existing unambiguous `openai_secure_tunnel` configuration, uses an atomic
  replacement with a recoverable backup, and refuses malformed or ambiguous
  configuration. It does not run during ordinary startup or this review.

## Deterministic verification

The following commands were run without a live tunnel mode or browser:

- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-secure-mcp-route-validation.ps1`
  — passed. The test extracts only validation/migration helpers; it does not
  read an operator config, discover a client, or invoke a tunnel command. It
  covers IPv6 bracket construction, the observed alias `.url` filename,
  migration/idempotence, preservation, backup creation, and malformed or
  ambiguous config refusal.
- `cargo test openai_tunnel::tests --no-fail-fast` — 26 passed.
- `cargo test openai_external_mode_does_not_infer_readiness_from_configured_admin_url --no-fail-fast` — passed.
- `cargo test openai_official_runtime_reuses_ready_alias_without_duplicate_process --no-fail-fast` — passed.
- `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- Worker-sandbox `cargo test --no-fail-fast` — 437 passed, 7 failed, 11
  ignored. The seven environment-specific failures require an unavailable
  advisor executable or Windows process-tree termination permission; the
  secure MCP targeted tests passed.
- CatDesk independent host verification — PASSED: 455 tests, no failures.
- `git diff --check` — passed.

The Rust coverage includes a local loopback health server on an ephemeral port,
healthy dynamic-port readiness, health-live/ready-false behavior, malformed and
non-loopback URL rejection, the stale fixed port rejection, an approved
per-alias health URL file, no duplicate attach behavior, bounded recovery
limits, and redacted plus repeated-percent-encoded route rejection.

## Pre-fix and post-fix behavior

Before the fix, the setup script overwrote the in-memory route with a
presentation marker before constructing `--mcp-server-url`; official status
flags and a configured admin URL could also be treated as readiness without
authoritative dynamic `/healthz` and `/readyz` confirmation. This was
reproduced by source inspection only, with no live process interaction.

The redacted pre-fix operational observation supplied for this review was:
`localMcp=READY` with 65 tools, `transportHealth=CONNECTING` solely because of
the stale `http://127.0.0.1:3220/readyz` check, and loaded
`processMode=external_foreground`, while the official runtime was independently
known ready. No route, credential, tunnel ID, complete MCP endpoint, or
official-runtime health URL is included here.

After the fix, a presentation marker cannot become a runtime target, and an
official runtime stays unverified unless the dynamic authoritative URL and all
three readiness layers succeed. Runtime/status command output is not persisted
or returned; diagnostics use redacted reasons and fingerprints only.

## Migration and controlled acceptance checklist

New setup configuration converges to `process_mode = "official_runtime"`.
Existing `[openai_tunnel]` sections are intentionally not rewritten by the
setup script. Operators migrating an older external-foreground entry should
review it and select official-runtime configuration explicitly; no automatic
config rewrite is performed.

After CatDesk accepts this source bundle, controlled live acceptance should:

1. Confirm local CatDesk MCP readiness.
2. Obtain official runtime state for the existing alias and verify its dynamic
   loopback `/healthz` and `/readyz` endpoints.
3. Confirm `catdesk_transport_status` is `CONNECTED_VERIFIED` without exposing
   the underlying URL.
4. Confirm no duplicate runtime was created and external ownership persisted.

No live CatDesk or tunnel restart occurred while preparing this bundle.

## T-0051-R2 release-build evidence

The current dirty source completed an isolated workspace release build with
Cargo. The SHA-256 fingerprint of the resulting release executable is
`0ed4aa78fb003ef75aa13a04b958146b5bdc042d8f278183637d977edeb615c4`.

The ordinary workspace release-output location is held open by the live
CatDesk daemon, so the in-place `cargo build --release` replacement was not
forced. The isolated target build provides current-source compilation evidence
without replacing that live executable. No migration, restart, daemon action,
tunnel action, or configuration change occurred for this evidence collection.
