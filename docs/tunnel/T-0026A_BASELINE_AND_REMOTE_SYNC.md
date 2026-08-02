# T-0026A Baseline and Remote Sync

## Result

`PASS`

T-0025 remote synchronization was verified before starting T-0026 work. The new
T-0026 branch and worktree were created from the synchronized T-0025 checkpoint.

## Repository

- Origin: `https://github.com/vnath99/CatDesk.git`
- Upstream: `https://github.com/Xeift/CatDesk.git`
- Parent branch: `infra/stable-mcp-transport`
- Parent checkpoint: `fabcfffedb8bd66488adb071a1b055c1169f3eef`
- T-0026 branch: `infra/openai-secure-mcp-tunnel`
- T-0026 worktree: `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-secure-mcp`

## Sync Evidence

- `git fetch origin`: completed.
- Local T-0025 HEAD: `fabcfffedb8bd66488adb071a1b055c1169f3eef`
- Remote T-0025 HEAD: `fabcfffedb8bd66488adb071a1b055c1169f3eef`
- Ahead/behind: `0 0`
- Force push: not used.
- Protected stabilization worktree: not modified.

## Worktree Inventory

- `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk`: protected Qwen
  stabilization worktree.
- `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-stable-mcp`: protected
  T-0025 transport worktree.
- `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-secure-mcp`: active T-0026
  worktree.

## Architecture Inventory

- MCP HTTP routing is built in `src/server.rs` with a runtime MCP path from
  `AppState::mcp_path`.
- MCP tool discovery and calls are implemented in `src/mcp.rs`.
- Runtime mode selection starts in `src/main.rs::start_services`.
- Transport startup is currently delegated to `src/ngrok.rs::start_transport`.
- Legacy managed ephemeral ngrok still uses the existing ngrok Rust SDK path.
- External tunnel mode marks an externally supplied public base URL and does not
  launch or stop tunnel processes.
- Managed stable ngrok currently fails explicitly because T-0025C0 did not prove
  assigned-domain reuse.
- OpenAI Secure MCP Tunnel is recognized by configuration but not yet wired to a
  runtime client.

## Baseline Leak Scan

Baseline scans found expected configuration identifiers and synthetic test
sentinels only:

- `ngrok_authtoken` field and helper names.
- `test-token-123` synthetic round-trip tests.
- `secret-token-that-must-not-leak` synthetic redaction tests.

No actual ngrok authtoken, OpenAI key value, ChatGPT cookie/session data,
complete public MCP endpoint, persistent route value, `.env` file, or browser
profile was intentionally recorded in this baseline document.

## Current Test Baseline

The inherited T-0025 branch previously passed:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test` with 294 passed and 9 ignored
- `cargo build --release`

T-0026 will rerun the full gates before every checkpoint commit.
