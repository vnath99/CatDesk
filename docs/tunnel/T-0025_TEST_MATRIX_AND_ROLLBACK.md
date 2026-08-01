# T-0025 Test Matrix and Rollback Plan

Ticket: T-0025A  
Status: planned tests only; no runtime behavior changed

## Test Matrix

### Configuration

| Test | Mode | Expected Result |
| --- | --- | --- |
| Missing new transport config | legacy | Current ephemeral behavior is preserved |
| Known tunnel modes parse | all | Every documented mode is accepted |
| Unknown tunnel mode | all | Config load fails with clear error |
| Invalid port | all | Startup fails before binding |
| Non-loopback bind | stable/external | Rejected unless later explicitly approved |
| Malformed public base URL | external | Rejected; no fallback |
| Malformed ngrok domain | stable | Rejected; no fallback |
| Missing route in stable mode | stable/external | Route is generated once and persisted |
| Route survives restart | stable/external | Same route after three app restarts |
| Route rotation | stable/external | New route is atomically saved, restart is required, old route invalid after restart |
| Config write interruption | stable/external | Previous valid config remains or new complete config is loaded |
| Route/config permissions | stable/external | Owner-restricted permissions or explicit warning |

### Process Ownership

| Test | Mode | Expected Result |
| --- | --- | --- |
| Legacy managed ngrok | legacy | Existing embedded SDK behavior remains |
| Stable managed ngrok | stable | Configured domain is passed to ngrok SDK |
| External tunnel startup | external | CatDesk never starts ngrok |
| External tunnel shutdown | external | CatDesk never stops external tunnel |
| CatDesk-owned SDK task shutdown | managed modes | Only CatDesk-owned task is cancelled |
| Unrelated ngrok process present | all | Process is preserved |
| Occupied local port | all | Startup fails clearly |

### Redaction

| Test | Expected Result |
| --- | --- |
| Standard logs with public endpoint | Full URL omitted |
| Error logs after tunnel failure | Route secret omitted |
| Review bundle generation | Full URL, token, and route omitted |
| Copy full URL action | Full URL returned only to deliberate operator action |
| Reveal once action | Confirmation required and event is auditable |
| Connection fingerprint | Stable for same full URL; cannot reconstruct URL |

### Stable URL Continuity

| Test | Expected Result |
| --- | --- |
| Stable ngrok restart 1 | Same complete plugin URL |
| Stable ngrok restart 2 | Same complete plugin URL |
| Stable ngrok restart 3 | Same complete plugin URL |
| External tunnel CatDesk rebuild/restart | Tunnel process remains alive; URL unchanged |
| ChatGPT `catdesk_instruction` after restart | Succeeds without plugin recreation |
| ChatGPT `delegated_run_list` after restart | Succeeds without plugin recreation |

### T-0025C0 Stable Ngrok Feasibility Gate

Before implementing complete managed stable-ngrok process behavior, run a small
live proof:

| Gate | Expected Result |
| --- | --- |
| Installed ngrok SDK/version identified | Version recorded without token |
| User account development domain available | Domain confirmed without printing full URL |
| Disposable localhost server | Serves a harmless health response |
| Stable ngrok start 1 | Domain reaches disposable server |
| Stable ngrok restart 2 | Same domain reaches disposable server |
| Evidence redaction | No full URL, route, or token in evidence |

If this gate fails, T-0025C must stop before adding full stable-ngrok lifecycle
behavior.

### Delegated Runtime Regression

| Test | Expected Result |
| --- | --- |
| Existing Rust unit suite | Green |
| Qwen 3.6 disposable delegated smoke | `COMPLETED_VERIFIED` |
| DeepSeek disabled validation | No advisor invocation |
| Cloud fallback guard | No cloud provider used |
| Tool policy guard | Existing MCP policies unchanged |

## Required Live Tests in Later Phases

T-0025C0 feasibility proof:

1. Use the installed ngrok SDK/version.
2. Use the user's account-assigned development domain.
3. Start a disposable localhost HTTP server, not CatDesk.
4. Start stable ngrok forwarding to the disposable server.
5. Restart the disposable server and stable ngrok twice.
6. Confirm the same domain after both starts without recording the full URL.
7. Record only redacted domain/fingerprint evidence and no token.

Stable ngrok live proof:

1. Start CatDesk in managed stable ngrok mode.
2. Record only redacted URL fingerprint.
3. Connect ChatGPT plugin once.
4. Restart CatDesk three times.
5. Confirm the full plugin URL remains unchanged without printing it.
6. Confirm `catdesk_instruction` succeeds after each restart.
7. Confirm `delegated_run_list` succeeds after each restart.

External tunnel live proof:

1. Start operator-managed tunnel outside CatDesk.
2. Start CatDesk in external tunnel mode.
3. Rebuild and restart CatDesk.
4. Confirm the tunnel process was not stopped by CatDesk.
5. Confirm ChatGPT plugin still reaches the same endpoint.

Disposable delegated proof:

1. Use exact model `qwen3.6:35b-a3b`.
2. Keep DeepSeek disabled.
3. Use a disposable repository.
4. Complete read/search, patch preview, patch apply, verify, final reasoning.
5. Require `COMPLETED_VERIFIED`.

## Rollback Procedure

T-0025A rollback:

```powershell
git worktree remove C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-stable-mcp
git branch -D infra/stable-mcp-transport
```

Runtime rollback after later implementation:

1. Set tunnel mode to `managed_ephemeral_ngrok`.
2. Remove or ignore persistent route fields in
   `%USERPROFILE%\.catdesk\config.toml`.
3. Restart CatDesk from the reviewed stabilization branch.
4. Reconnect ChatGPT using the legacy generated URL if needed.

Emergency rollback:

1. Stop the T-0025 development binary.
2. Launch the reviewed stabilization binary from:

```text
C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk\target\release\catdesk.exe
```

3. Do not delete the stabilization branch.
4. Do not delete external tunnel configuration unless explicitly approved.

## PR Strategy

- T-0025A is docs/planning only.
- T-0025B implements persistent config and route abstraction.
- T-0025C implements stable ngrok and external tunnel modes.
- T-0025D handles Secure MCP Tunnel proof.
- T-0025E completes end-to-end validation.

The T-0025 PR should target:

```text
codex/delegated-loop-qwen36-stabilization
```

It should not target `main` until the stabilization branch itself is merged and
the user approves final integration order.

## Phased Credit Estimate

| Phase | Estimate | Notes |
| --- | ---: | --- |
| T-0025A | 25-40 | Architecture, worktree, planning, review bundle |
| T-0025B | 45-75 | Config, persistent route, redaction primitives |
| T-0025C | 60-100 | Stable ngrok, external mode, ownership checks |
| T-0025D | 25-50 | Secure MCP Tunnel proof and decision |
| T-0025E | 45-80 | End-to-end continuity and regression validation |

Total estimated budget: 200-345 credits.
