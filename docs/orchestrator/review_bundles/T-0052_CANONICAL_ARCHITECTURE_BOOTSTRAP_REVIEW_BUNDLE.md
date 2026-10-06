# T-0052 Canonical Architecture and Bootstrap Review Bundle

## Delivered scope

`CANONICAL_CURRENT_ARCHITECTURE.md` is the current authority for ownership,
on-demand Codex lifecycle, review/wake semantics, and reboot recovery.
`scripts/start-catdesk-stack.ps1` is the canonical plan-first recovery entry
point. It does not run during this review.

## Startup state machine

1. Run the one-time `scripts/provision-catdesk-release.ps1` release procedure
   when provisioning a new build. It writes only an adjacent non-secret SHA-256
   manifest. Reboot recovery validates the provisioned
   `target/release/catdesk.exe` against that manifest (or an explicit expected
   SHA-256), plus config shape, a single loopback listener, and current-user Codex CLI
   availability without reading credentials. Reboot recovery never compiles a
   replacement; absent or unverifiable release identity fails closed.
2. In plan mode, report only redacted state and never launch a worker.
3. If the listener path and fingerprint equal the canonical release in
   official-runtime mode, attach without a duplicate process. A mismatched
   CatDesk daemon is stopped exactly before canonical restart, even if disk
   configuration is already official-runtime.
4. If legacy external-foreground mode is present, explicit recovery stops only
   the verified CatDesk listener before migration, preserves the original
   recoverable T-0051 backup, proves durable official-runtime mode, then starts
   the canonical native daemon with `--catdesk-daemon`.
5. Wait for route-scoped local MCP JSON-RPC `initialize` plus `tools/list`
   (requiring `catdesk_instruction` and `delegated_run_list`), authoritative
   official runtime status flags, and dynamic loopback `/healthz` and `/readyz`.
   Transient status and network failures remain pollable during the bounded
   recovery window. Only this full
   conjunction returns `CONNECTED_VERIFIED`. CatDesk's Rust monitor retains
   bounded existing-alias recovery ownership; bootstrap never creates,
   reconnects, stops, or removes the runtime.

When recovery is needed and the canonical daemon predates the current bootstrap
environment, bootstrap first requires only the presence of the approved
environment references, then performs one exact CatDesk relaunch to inherit
them. A newly launched daemon is not relaunched again, and a verified runtime
does not cause any restart. Runtime status and approved health files use the
validated configured runtime alias, which is not emitted in diagnostics.

## Boundaries and deprecated paths

The external OpenAI runtime remains external. The bootstrap never stops or
creates a second tunnel runtime. It does not expose route IDs, endpoints,
health URLs, credentials, tokens, cookies, or sensitive user paths. Historical
relaunch/restart scripts and ticket documents are retained as evidence; the
canonical architecture document supersedes them for normal recovery.

## Deterministic coverage

- `scripts/test-start-catdesk-stack.ps1`: config mode parsing, ambiguity and
  malformed refusal, route-scoped MCP JSON-RPC success/failure, required-tool
  gating, no route leakage, manifest validation, transient runtime polling,
  dynamic health URL rejection, runtime flag gating, `/healthz` + `/readyz`
  success conjunction, lifecycle safety markers, on-demand Codex marker, and
  refusal of the auto-start TUI path.
- `scripts/test-secure-mcp-route-validation.ps1`: redacted route rejection,
  IPv6 target construction, migration idempotence, stale-writer recovery, and
  existing-backup preservation.
- `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings` passed.
- Worker-sandbox `cargo test --no-fail-fast` recorded 437 passed, 7 failed,
  and 11 ignored. The failures are the established unavailable-advisor and
  Windows process-tree permission limitations, not bootstrap coverage.
- `git diff --check` passed.

No live CatDesk, official runtime, migration, reboot, or browser action was
performed while preparing this bundle. Clean reboot acceptance is operator-only:
first provision the reviewed release binary with
`scripts/provision-catdesk-release.ps1`; after login the one-command recovery
is `scripts/start-catdesk-stack.ps1 -Mode recover -Execute`. The optional
`-Mode plan` dry-run is redacted and non-mutating; then confirm
`CONNECTED_VERIFIED` from canonical local MCP plus authoritative official-runtime
health before performing a fresh actionable review wake through the normal
CatDesk flow.
