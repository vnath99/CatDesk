# MCP and ChatGPT wake semantics

MCP is request/response. CatDesk can return a review-inbox record or review
bundle only while ChatGPT has already made an active MCP request. The Secure
MCP tunnel transports that request; it does not give the server authority to
originate a new ChatGPT user or assistant turn once the conversation is idle.

The local wake bridge is therefore deliberately separate from MCP. It polls
only CatDesk's durable `review-inbox.json`, and can use an operator's dedicated,
manually authenticated SeleniumBase browser profile to send a single minimal
wake message into one operator-configured conversation. It does not use an
OpenAI API, private backend, browser storage, cookies, credentials, or tokens.

The bridge retires the historical unread backlog on first initialization (or
through an explicit local retirement operation), then records an event ID
locally before it will consider it again. Its bounded durable accounting keeps
only record ID, claim/send timestamps, observable latency, result, and an
operator-attention category—never page text, URLs, browser diagnostics,
cookies, tokens, or browser storage. It debounces messages and stops on login,
CAPTCHA, exact-conversation origin drift, an existing draft, or ambiguous
composer/send selectors. There is no polling, webhook, API, or alternate
provider fallback that can claim a wake without the dedicated browser submit.

Every mutating bridge execution (`--once`, backlog retirement, or the normal
poll loop) holds the fixed project-local `.catdesk/wake-bridge/wake-bridge.lock`
for its full lifetime. It uses a non-blocking standard-library OS file lock
(`msvcrt` on Windows and `fcntl` on POSIX), released when the process handle
closes. A competing current-version process returns `WAKE_BRIDGE_BUSY` before
it can change bridge state, claim an event, or launch SeleniumBase. The lock
file contains no credentials or wake content.

## Local operator bootstrap

The bridge has an explicit local-only bootstrap command. It validates an exact
credential-free conversation URL and a project-local dedicated profile path,
then writes only local configuration. It never launches a browser, imports
SeleniumBase, reads profile contents, or requests authentication. Creating an
empty profile directory is opt-in; manual sign-in happens later in an
operator-controlled browser session.

```powershell
python scripts/wake_bridge.py --workspace . --bootstrap `
  --conversation-url "https://chatgpt.com/c/<exact-conversation-id>" `
  --profile-dir ".catdesk/wake-bridge/browser-profile" --create-profile-dir
```

The command refuses an existing configuration unless `--overwrite-config` is
supplied, refuses profile/config paths outside the workspace, and does not
accept bootstrap-only flags during a normal wake run. The generated file
contains the URL, profile directory, bounded timing values, and debounce only;
it contains no credentials, cookies, tokens, or provider selection.

Before that configuration is written, the operator may create a dedicated
project-local Python environment and explicitly install the one pinned browser
dependency. This is a setup action, not a browser action: it does not start
Chrome, open the profile, visit ChatGPT, read credentials, or authenticate.

```powershell
.\scripts\bootstrap_wake_bridge.ps1 -PythonExecutable "C:\path\to\python.exe" `
  -InitializeEnvironment -InstallSeleniumBase

.\scripts\bootstrap_wake_bridge.ps1 -PythonExecutable "C:\path\to\python.exe" `
  -WriteConfig -ConversationUrl "https://chatgpt.com/c/<exact-conversation-id>"
```

The resulting virtual environment, browser profile, state, and config all live
under `.catdesk/wake-bridge/`, which is ignored by Git. The script refuses to
replace an existing environment, requires explicit installation/configuration
switches, and refuses profile paths outside the workspace. Manual login is a
separate, operator-owned, headed-browser step after this bootstrap.

## Production CDP wake send path

The production adapter uses SeleniumBase UC with native CDP mode only. It
activates CDP mode on the exact configured conversation URL, verifies the
current URL remains that exact HTTPS target, and waits for `#prompt-textarea`.
It fails closed on login, CAPTCHA, target drift, an existing draft, a busy
ChatGPT stop control, or an unconfirmed submit. Chrome's own network-error page
is retried at most twice through CDP reload and then an exact-target CDP get.

The adapter types the fixed automated project-resume prompt with CDP
`press_keys`, clicks a current visible send selector when one is available, and
uses native Enter on the exact editor only when none is visible. It records a
delivery only after the editor clears during the bounded confirmation window.
The prompt contains the triggering review record ID and directs ChatGPT to
inspect authoritative CatDesk state; it does not embed review-bundle contents.

Historical `MCP_READY` receipts, `--preflight`, and manual per-wake connector
acknowledgements are acceptance scaffolding, not production wake-bridge gates.
They are not read, written, or consumed by `wake_bridge.py`. CatDesk Phase 2
will decide whether and how the controller invokes this one-shot adapter.

## Phase-two exact-record handoff

The adapter accepts `--record-id` for one exact review record. It refuses a
missing, malformed, acknowledged, stale, or non-actionable record and accepts
only `COMPLETED_VERIFIED` / `independent_final_review` and
`WAITING_FOR_CHATGPT` / `chatgpt_decision_required` records. This prevents a
manual diagnostic invocation from scanning or selecting unrelated unread work.
The runtime-owned dispatcher is the intended production caller; the manual MCP
tool retains explicit confirmation and must supply the same exact record ID.

The runtime dispatches only after a controller outcome is
`COMPLETED_VERIFIED` or `WAITING_FOR_CHATGPT` and finds that session's matching
newest unread actionable record from the bounded review inbox. It starts one fixed project-local Python child with
only workspace, config, script, and record-ID arguments, without a shell and
without waiting for Chrome. A bounded in-memory record-ID set prevents duplicate
launches in one daemon; the adapter singleton and durable claim remain the
cross-process boundary. Worker-owned states never enter this dispatcher.
