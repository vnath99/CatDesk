# T-0060F-R5 Official Tunnel Reattach / Recover Hardening Review Bundle

## Root cause

Native daemon replacement already persisted `REPLACEMENT_READY_PENDING_TRANSPORT_RECONNECT`: loopback listener handoff proves only local CatDesk MCP readiness. It does not prove that the externally owned official tunnel runtime remains attached to the new listener. During the observed reload, the old daemon/connector session ended while the official client process survived its recorded parent. The recovery facade consequently saw local MCP ready but a stale runtime and could only surface `ACTION_REQUIRED`; the later natural monitor reconnect showed that port handoff and transport reattachment were separate states.

## State model

The monitor now classifies only bounded, non-secret states:

| State | Meaning |
| --- | --- |
| `LocalMcpAndRuntimeHealthy` | local MCP and dynamically verified official runtime are healthy. |
| `LocalMcpRuntimeStale` | local MCP is ready and a uniquely verified runtime is alive but not attached/ready. |
| `OrphanedRuntime` | runtime evidence exists but local MCP is not ready after owner/handoff loss. |
| `RuntimeAbsent` | authoritative status reports no runtime process. |
| `AmbiguousOrMismatchedRuntime` | running status lacks the trusted identity required for reattach. |

`CONNECTED_VERIFIED` remains reserved for the local listener plus authoritative runtime status and dynamic health proof. A reload handoff remains explicitly transport-pending until the replacement monitor reaches that proof; an expected MCP disconnect is not success evidence.

## Changed behavior and ownership boundary

`src/openai_tunnel.rs` now validates a recovery identity before any reconnect of an already-running runtime: canonical trusted tunnel-client executable SHA-256, configured official-runtime alias/profile fingerprints, and status-provided PID plus tunnel fingerprints. Status aliases must match the configured alias. PID/name-only, absent identity, client replacement/path drift, alias mismatch, or malformed status refuse reattach. No process enumeration, signal, stop, remove, or kill action was added.

`src/ngrok.rs` captures the trusted client fingerprint after existing discovery/validation. For a stale runtime with local MCP ready, official mode enabled, auto-recover enabled, environment-reference presence, and complete identity, it issues one bounded `runtimes connect` command through the existing official control surface. This is a non-destructive alias-scoped reattach, not a process launch/duplicate. The monitor’s existing cooldown/window/attempt budget remains the follow-up recovery owner and rechecks status plus `/healthz` and `/readyz` before promotion. Any missing prerequisite records a fixed pending/blocked reason such as `OFFICIAL_RUNTIME_IDENTITY_UNAVAILABLE`, `OFFICIAL_RUNTIME_CREDENTIAL_REFERENCE_UNAVAILABLE`, or `OFFICIAL_RUNTIME_REATTACH_FAILED` without exposing credentials, routes, command output, or process paths.

The canonical `catdesk.ps1 recover` facade still delegates to the reviewed release-only stack engine. Its bounded full-stack wait can now converge through the deterministic Rust reattach path when the stated identity and environment-reference prerequisites exist; otherwise it keeps the fixed actionable result. CatDesk continues to preserve `keep_runtime_on_catdesk_exit` and never owns normal shutdown of the official runtime.

## Regression coverage

Deterministic local tests cover client fingerprint drift/replacement, missing PID/tunnel identity, alias mismatch, stale/healthy/orphaned/absent/mismatched state classification, cooldown and retry budgeting, failed/oversized/non-UTF-8 bounded command metadata, no stop/remove/create recovery command, existing alias handling, daemon transport-pending handoff semantics, and existing external-runtime shutdown isolation. All fixtures use temporary files or status seams; no live client, runtime, credential, browser, or external workspace is accessed.

## Verification

Run locally for this ticket:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- `git diff --check`

## Remaining risk and host acceptance

The official status API must provide its own alias-scoped PID and tunnel fingerprints. If it omits them, CatDesk deliberately does not reconnect a running runtime and reports the stable identity-unavailable state. No PID reuse or duplicate-runtime inference is attempted.

Host acceptance is separate and must use an isolated reviewed build/hash: perform one reviewed daemon reload, observe the durable transport-pending handoff state, then require bounded `CONNECTED_VERIFIED` status and a single alias-scoped runtime after reattach. Do not deliberately create or kill a stale live runtime for testing without a separate approval. Stop on identity/credential/cooldown failure; do not retry by manual process manipulation.
