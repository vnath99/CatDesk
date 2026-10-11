# T-0478 R2 — Bounded fixture-only developer MCP surface

Date: 2026-10-11 UTC. Source-only, not activated as a second CatDesk daemon.

## Scope and invariant

Source-only T0478 R1 `scripts/catdesk-dev-lane.ps1 status|build|verify` passed all three CI jobs (commit 5f70329, run 38111868683). The R2 aim is a strict negative capability boundary for a **disposable fixture** server before attempting a developer worker with agents or live project access. Production CatDesk is still served by the separately owned Secure MCP tunnel; its signed image and independent WakeHost are untouched.

## Source changes

- `src/main.rs` adds explicit `--isolated-dev-fixture` ONLY within `--headless-mcp`. It requires exact loopback host 127.0.0.1, OS-assigned port0, computer/read-only mode, explicit `--workspace`, `--config-path`, `--mcp-path /dev/mcp`, and a process-supplied bearer auth token. The workspace must resolve to one direct `catdesk-dev-fixture-<ascii>` child of the canonical OS Temp directory, and config must be that fixture's regular `config.toml` (or not yet created), not the normal user `~/.catdesk/config.toml`. No symlink config leaf.
- `src/server.rs::isolated_fixture_router` is a **distinct router**, not a filtered view of the normal mutating HTTP application. Its only route is bearer-authenticated `POST /dev/mcp`. It intentionally excludes normal partner/profile/layout/archive mutation routes, devtools, OAuth advertisement, websocket/UI operations and external tunnel ownership.
- `src/mcp.rs::handle_isolated_fixture_request` is a separate dispatcher. Supported requests: MCP initialize (advertising tools only, NOT a resources capability), ping, notifications, `tools/list` exposing exactly `read` and `search`, and `tools/call` for those two relative-to-fixture operations. Everything else is explicitly rejected before the general MCP dispatcher, plan guards, global Wake store, supervisor, signing, arbitrary command, provider, profile or browser code executes. No main production dispatcher or unrelated global status is called.
- Deterministic Rust tests in main/mcp/server prove parser refusal of non-ephemeral port/nonfixture paths or tool mode, token-required HTTP rejection, exactly two tool list results, and rejection of every representative global/mutating tool and MCP resources method. The fixture HTTP test does not require any external account, tunnel or browser.
- *This is still not a security sandbox* or a full autonomous developer agent. No untrusted code execution, process isolation from the current OS account, or full worker `run/stop` command has been authorized. File read/search can inspect only properly confined fixture files; future fixture lifecycle should copy explicit audited source inputs into the temporary root and separately enforce symlink/hardlink and stale-PID policy.

## Review and acceptance

- Local source formatting passed, but old serving CatDesk shell currently rejects cargo test/Clippy as `INVALID_ARGUMENT`; GitHub Windows CI is the authoritative independent confirmation, not assumed success. No runtime was started by ChatGPT.
- Requirement to proceed: Windows Rust build/fmt/Clippy/tests all pass including new tests, Python and independent WakeHost jobs pass; inspect exact failures without disabling existing tests. An additional reviewed isolated launcher would need controlled fixture preparation, confined application config initialization, exact PID/port process ownership, bounded shutdown and denial of real project tools before claiming runnable worker.
- Production Recovery remains partial: stable supervisor startup policy unproven/root unavailable; old serving CatDesk gateway typed `catdesk.ps1 diagnose` refused `INVALID_ARGUMENT`. The separate T0481 independent-Wake recovery source correction **did pass** all three Windows jobs in run38112167722; docs cleanups run38112359878 also pass. No protected signed T0215/T0366 rotation, no external tunnel mutation.
- Chat51 manual Wake source+target restored and gen32 exact `EXACT_USER_MESSAGE_APPENDED` exists for preceding canary `manual-wake-mcp-1791691944140`; newer manual continuation event `manual-wake-mcp-1791693864808` appeared in Chat51, final durable receipt must be checked separately after the responding turn settles. Manual diagnostic is never natural acceptance.
