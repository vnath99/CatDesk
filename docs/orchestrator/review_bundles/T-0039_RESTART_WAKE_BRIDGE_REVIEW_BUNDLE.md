# T-0039 Restart Handoff + Wake Bridge Review Bundle

Status: `IMPLEMENTED_WITH_OPERATOR_ATTENTION`. This bundle records the
deterministic implementation and the actual detached-handoff result. It does
not claim that CatDesk independent verification, Secure MCP reconnection, or a
live browser wake completed.

## Restart handoff

New scripts:

- `scripts/restart_catdesk_daemon.ps1` performs a non-mutating preflight and,
  only with `-Execute`, launches `restart_catdesk_daemon_worker.ps1` detached.
- The worker accepts an explicit PID, requires that it is named `catdesk`,
  waits briefly, stops only that PID, starts the verified build from this
  workspace, and polls replacement process/listener readiness.

The helper never reads command lines or process environments, and it does not
enumerate, stop, reconfigure, or replace any tunnel-client process. It records
only PID, loopback port count/values, build SHA-256, status, stage, timestamps,
and recovery instructions in `.catdesk/restart-handoff/latest.json`. It never
prints or persists `CATDESK_CODEX_CLI_EXECUTABLE` or any credential value.

Worker execution used PID `10524`, the active CatDesk process, with the current
verified debug build. Preflight observed no loopback listener owned by that
process. The detached worker recorded `FAILED_OPERATOR_ATTENTION`, and the old
process was still running afterwards; therefore the helper failed closed before
replacement. The external `tunnel-client` process remained running and was not
altered. The recovery marker instructs the operator to start the verified build
from the approved workspace and verify reconnection of the existing tunnel,
without creating or restarting a replacement tunnel.

After the persisted failure state, no new MCP control surface could be verified
from this worker. On reconnect, CatDesk should verify the redacted transport
status and that `autonomy_execution_accounting`, review-inbox, and project
registry surfaces are listed. This is also the recovery marker for ChatGPT.

## Wake bridge

`scripts/wake_bridge.py` polls only the durable CatDesk
`.catdesk/autonomy/review-inbox.json` records. Its `WakeSink` interface supports
the dedicated SeleniumBase browser implementation only. The later T-0039-R2
repair removed the scheduled-poll and webhook fallback paths because neither
can prove an exact conversation received a submitted wake. The SeleniumBase
sink requires an exact
operator-configured ChatGPT HTTPS conversation URL and an existing dedicated
manually authenticated browser profile; it neither reads profile files nor
browser storage.

For each unread event it sends only:

```text
CatDesk wake event <id> is ready. Inspect CatDesk through MCP and continue.
```

The event ID is durably claimed before browser interaction. A crash or browser
driver error can therefore lose a notification but cannot duplicate it. A
60-second configurable debounce permits one event at a time. Invalid inbox
records, missing profile, unsupported origin, CAPTCHA, login/composer/send
selector uncertainty, a non-empty draft, and unexpected driver failure persist
`operator_attention` and stop all later writes until an operator explicitly
clears the state after resolving the condition.

No safe already-authenticated dedicated browser profile or Python interpreter
was available in this worker session. No live browser was launched and no
disposable event was injected. The local manual gate is explicit: provide the
dedicated profile and operator-selected conversation URL in local ignored
`.catdesk/wake-bridge/config.json`, then run the bridge; do not provide
credentials, cookies, tokens, or API keys to CatDesk.

## MCP semantics

[MCP_WAKE_BRIDGE_SEMANTICS.md](../MCP_WAKE_BRIDGE_SEMANTICS.md) documents that
MCP is request/response and the Secure MCP server cannot originate a new
ChatGPT turn after idle. Long-poll is explicitly experimental and
non-authoritative because tool timeouts and conversation lifecycle make it
unsuitable for hours-long orchestration.

## Verification evidence

- PowerShell AST parser validation for both restart scripts: passed.
- `cargo build`: passed.
- `cargo fmt --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- Full `cargo test --no-fail-fast`: 394 passed, 7 failed, 9 ignored. The seven
  failures match existing environment limitations: three require the absent
  advisor program, three require Windows process-tree termination permission,
  and the configured-advisor cancellation expectation then lacks that program.
- Python deterministic tests were added at `tests/test_wake_bridge.py`, covering
  event detection, restart idempotency, debounce, selector/login failure,
  generic driver failure redaction, and invalid inbox fail-closed handling.
  They could not run because neither `python`, `python3`, nor the Windows `py`
  launcher is installed or available on PATH in this worker environment.
- `git diff --check`: to be repeated by independent CatDesk verification after
  review; it does not include untracked worktree files.

## Independent final verification request

CatDesk should independently run the Rust and Python suites in an environment
with Python/SeleniumBase, inspect the persisted restart marker, then perform
the approved operator-owned relaunch/reconnect and a single disposable
already-authenticated dedicated-profile wake. Do not commit, push, merge, PR,
release, deploy, access credentials, or use an OpenAI API/private backend.
