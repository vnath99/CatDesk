# T-0045 Live Authenticated Thread Binding Review Bundle

Status: `PARTIAL_LOCAL_APP_SERVER_RUNTIME_STATE_GATE`.

## Outcome

T-0045 preserved the existing T-0030--T-0044 working tree and exercised the new opaque CatDesk launch context without opening, copying, parsing, or printing authentication material. The supported direct Codex login classification is now `Logged in using ChatGPT`: positive evidence that the CatDesk-provided home context reaches an authenticated account.

The supported direct `codex app-server --stdio` child still exits before it can answer `initialize`. Its redacted, bounded diagnostic says that it failed to initialize its local SQLite state runtime; runtime cleanup warnings are `Access is denied`. This is a local runtime-state write boundary, not an authentication or rate-limit rejection. The task therefore failed closed before any live thread, registry, or turn mutation.

No credential material, API key, cloud/provider fallback, billing operation, forced exhaustion, branch change, reset, clean, commit, push, merge, PR, release, or deployment was performed.

## Daemon And Launch-Context Evidence

| Probe | Bounded result |
| --- | --- |
| `CATDESK_CODEX_HOME` in current launch context | Present; value and contents redacted |
| `CODEX_HOME` in current launch context | Present; value and contents redacted |
| Direct Codex executable setting | Present; value redacted |
| Local CatDesk health endpoint | `GET http://127.0.0.1:3200/` returned HTTP 200 |
| Durable relaunch handoff record | `stage=complete`, `status=RECOVERED_PENDING_TRANSPORT_CHECK`, MCP port `3200`; external tunnel state was not queried |
| Operator-provided transport state | Secure MCP `CONNECTED_VERIFIED`, local MCP `READY`, and daemon advertises 63 tools; not independently re-queried because no CatDesk MCP tool is available to this worker |

The existing process/query boundary does not authorize inspecting process environment values, command lines, auth files, tokens, cookies, or local database contents; none was attempted.

## Supported Direct Codex Authentication And App-Server Probe

The direct executable was invoked through the opaque configured executable setting. `codex login status` returned `Logged in using ChatGPT`. A fresh `app-server --stdio` process was then sent the supported `initialize` request with only client metadata and empty capabilities. No API-key variables were provided to the request or protocol.

| Probe | Result |
| --- | --- |
| `login status` | `AUTHENTICATED_CHATGPT` |
| `initialize` | No JSON-RPC response; child exited with code 1 before initialization |
| `account/rateLimits/read` | Not sent: unavailable before initialization |
| Redacted startup diagnostic | Local SQLite state-runtime initialization failed; stale argument-alias cleanup reports access denied |
| Diagnostic fingerprint | `sha256:41c55dbdfca711d28a510a4fa661d794ea7797f9e5b1031e61239d6690fa5345` |

This is materially different from T-0044's missing-auth-context finding: authentication is now supported and classified positively, while the required app-server runtime cannot create or open local state in this worker context.

## Thread, Continuity, And Registry Safety

Canonical workspace:

`<USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop`

Because `initialize` did not complete, no authenticated `thread/list` could be made. Therefore the worker did not inspect, bind, resume, or create any thread and did not use the canonical-replacement allowance. In particular, there is no proven exact candidate named `Integrate Codex MCP for ChatGPT` with all of: exact CWD, unowned state, and direct-input capability.

`autonomy_project_registry_bind` and `autonomy_project_registry_read` were not called: those are CatDesk MCP controls, and no CatDesk MCP tools are available to this worker. No project-to-workspace-to-thread mapping was written, no reconnect/list/read proof exists, and no first or second CatDesk-originated continuity turn was issued. This preserves the exact identity and one-writer gates rather than creating an unverified mapping.

## Accounting

All app-server-derived fields remain `UNKNOWN_NOT_CAPTURED`: account plan, rate-limit windows, used percentage, reset timestamp, model, reasoning effort, and thread token usage. The app-server was unavailable before the supported account/read request, so no values were estimated or inferred. No credits were purchased, reset, or deliberately exhausted.

## Remaining Gate

An operator/platform context that can write the authenticated Codex app-server's local SQLite runtime state is required. The safe repair is to run the already-authenticated direct app-server from the operator's writable CatDesk desktop context (or repair that context's local state-directory ACL) without supplying its path, auth files, tokens, cookies, or database contents to this worker. Do not delete, reset, or recreate the runtime database from this task.

After that boundary is resolved, rerun the work in order: `initialize`, `account/rateLimits/read`, fresh exact-CWD `thread/list`, exact candidate inspection, then either bind it or make at most one replacement. Only after reconnect/list/read and two harmless same-thread turns succeed may the bounded registry bind/read surface persist the mapping. CatDesk independent final verification remains pending and is not claimed here.

## Verification

- `cargo test codex_app_server -- --nocapture`: PASS (6 tests).
- `cargo test autonomy_supervisor::tests::project_registry_binding_is_canonical_evidence_gated_and_durable -- --nocapture`: PASS.
- `cargo test autonomy_runtime::tests::app_server_binding_requires_exact_workspace_thread_and_persists_bounded_telemetry -- --nocapture`: PASS.
- `cargo fmt --check`: PASS.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --no-fail-fast`: 399 passed, 7 failed, 9 ignored. The seven failures are the recorded environment-sensitive baseline: three configured advisor tests could not start their advisor program, the related advisor cancellation expectation failed, and three Windows process-tree cancellation tests received `ERROR: Access denied`.
- `git diff --check`: PASS; Git printed only pre-existing CRLF conversion warnings for dirty tracked files.

No CatDesk independent verification endpoint/tool is available in this worker context, so independent final verification remains pending and is not claimed.

