# T-0012 Runtime Comparison

Date: 2026-07-24

## Candidates

### CatDesk Direct MCP

Status: present and tested through existing Rust MCP tests.

Strengths:

- Owns file and shell execution boundaries locally.
- Has project memory, planning, verification, Git workflow, delete confirmation, shell allowlist, and read-only modes.
- Existing tests cover many of the safety surfaces needed for a delegated runtime.

Weaknesses:

- Browser/devtools headless mode is intentionally unsupported in the first local-MCP implementation.
- The headless path is experimental and should remain local-only until the delegated runtime design is frozen.

### Ollama with Qwen 3.5 Local Model

Status: available locally and probed through localhost API.

Strengths:

- `qwen3.5:9b` is installed and supports tool-call output.
- Tool-call JSON emission works on a synthetic request.
- Tool-result continuation works with valid synthetic JSON.
- Localhost-only tests avoided private repo content and secrets.

Weaknesses:

- The model did not reliably reject malformed tool output when prompted to escalate.
- Any delegated-worker design needs deterministic schema validation outside the model.
- Tool-calling quality should be re-tested with the exact OpenClaw prompt/runtime shape before production use.

### OpenClaw Runtime

Status: installed after the initial spike; basic read-only CLI diagnostics completed. OpenClaw is not onboarded, the Gateway service is not installed, no providers or credentials are configured, and a disposable MCP registry can see CatDesk.

Potential strengths:

- Designed as a personal assistant gateway with model routing, channels, agents, tool policy, MCP support, and Ollama integration.
- Official docs show MCP server mode, outbound MCP server configuration, tool filters for MCP servers, and policy mechanisms that can remove tools before model calls.
- Ollama integration can launch OpenClaw and configure models.
- CLI supports process-scoped disposable state/config through `OPENCLAW_STATE_DIR` and `OPENCLAW_CONFIG_PATH`.
- MCP registry commands can inspect saved servers without connecting, and `probe` can later prove a CatDesk MCP connection.
- Disposable OpenClaw MCP config successfully registered and probed a headless CatDesk endpoint.
- Correctly quoted include filters exposed exactly four read-only CatDesk tools.

Blocking uncertainties:

- OpenClaw's own native file/shell/runtime tools must be disabled or denied before CatDesk can be considered the only execution authority.
- Documentation shows native runtime and file tools exist in the OpenClaw catalog, so a configured runtime audit is required.
- The installed runtime has not yet been tested with a model worker using the disposable CatDesk MCP server.
- Effective worker tool-policy inspection still needs to prove that only CatDesk MCP tools are visible and that OpenClaw-native execution tools are absent or denied.
- No OpenClaw model worker turn, session restart, provider fallback, or structured event retrieval was run.

## Comparison Matrix

| Area | CatDesk Direct MCP | Ollama/Qwen | OpenClaw |
| --- | --- | --- | --- |
| Installed locally | Yes | Yes | Yes |
| Persistent changes needed for spike | No | No | No for read-only diagnostics; approval required before disposable config or service setup |
| Tool-call support | Yes, MCP JSON-RPC | Yes, model emits tool calls | Expected, not verified locally |
| File/shell authority | CatDesk-owned | None by itself | Native authority exists unless disabled |
| Safety controls | Strong and tested | Prompt-dependent | Policy-based, needs runtime audit |
| Headless automation | Local computer-mode MCP probe works | Yes via HTTP API | MCP probe works through disposable paths, but live worker flow is not yet proven |
| Main risk | Experimental headless surface | Malformed tool-result tolerance | Authority boundary drift |

## Feasibility Judgment

The architecture remains plausible, but it is not yet proven. CatDesk has the right safety primitives, local Qwen can emit tool calls, CatDesk can now run a deterministic local MCP endpoint, and OpenClaw can discover filtered CatDesk tools through disposable config/state. The missing proof is whether OpenClaw can be configured so the worker sees only CatDesk MCP tools and cannot use native OpenClaw file/shell/network/browser tools. Until that is verified with effective tool-list inspection before a model turn, OpenClaw must be treated as an unproven orchestration layer rather than a safe execution boundary.
