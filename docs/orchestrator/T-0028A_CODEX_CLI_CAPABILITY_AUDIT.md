# T-0028A Codex CLI Capability Audit

Status: PASSED

Date: 2026-08-05

## Scope

T-0028 requires a CatDesk-owned autonomous controller that launches and
resumes the installed Codex CLI through stable, headless, structured interfaces.
This audit intentionally makes no runtime or source-code change. It does not
read authentication stores, request credentials, or start a Codex worker.

## Isolated Baseline

| Item | Evidence |
| --- | --- |
| T-0028 branch | `orchestrator/chatgpt-codex-autonomous-loop` |
| T-0028 base | `7da3013af6661fa83c1d295cb9fff150327b336d` (`feat: add Secure MCP runtime operations`) |
| Origin | `https://github.com/vnath99/CatDesk.git` |
| Protected T-0026 worktree | clean at `e07e3f16db2f7f2746aa6e918f2750194a1b7753` |
| T-0027 operations worktree | clean at `7da3013af6661fa83c1d295cb9fff150327b336d` |

## Initial Blocker Resolution

The original audit ran before a standalone CLI installation existed. That
Microsoft Store package binary remains unsuitable for CatDesk process launch,
but it is no longer the selected executable. On 2026-08-06, a user-level
standalone CLI was found before the Store package on `PATH`.

The selected executable is the npm command shim in the user npm directory. It
launched from an ordinary PowerShell child process and reported:

```text
codex-cli 0.146.1
codex login status -> Logged in using ChatGPT
```

This audit did not read authentication files or expose account details.

## Superseded Store Package Finding

`Get-Command codex -All` resolves only the Microsoft Store package executable
under the OpenAI Codex package resources directory. No alternate user-level
CLI installation was found under the usual npm, local-bin, Program Files, or
user-program locations.

The desktop application is currently using that package's `codex.exe` to run
`app-server`, which proves the package contains a Codex binary. It does not
prove that CatDesk, a normal user process, can launch the binary headlessly.

Before the standalone installation, every direct process-launch probe against
the Store package from an ordinary PowerShell child process failed before
command execution with Windows `Access is denied`:

```text
codex --version                 -> Access is denied
codex exec --help               -> Access is denied
codex exec resume --help        -> Access is denied
cmd.exe /c "codex --version"    -> Access is denied
```

The package ACL grants package-identity execution rights in addition to normal
read access. The observed behavior is consistent with the Store-installed
binary being usable by the packaged desktop application but not launchable by
an ordinary CatDesk process. This audit does not modify those ACLs, copy the
binary, inspect authentication material, or attempt to bypass Windows package
execution policy.

## Required Capability Matrix

| T-0028 requirement | Result | Evidence |
| --- | --- | --- |
| Stable headless `codex exec` launch | PASS | `codex exec --json --sandbox read-only` completed normally from a child process. |
| `codex exec --help` capability discovery | PASS | Help exposes `--json`, `--sandbox`, `--output-schema`, `--output-last-message`, and `resume`. |
| `codex exec resume --help` discovery | PASS | Help accepts a session ID or `--last`, with JSONL output. |
| Structured JSONL event mode | PASS | Both probe turns emitted four parseable JSONL events. |
| Durable Codex thread/session identifier | PASS | `thread.started.thread_id` was captured and persisted in disposable evidence. |
| Resume the same Codex session | PASS | `codex exec resume <thread_id>` returned the same thread ID and completed normally. |
| Safe owned-process cancellation | PASS | CatDesk can terminate only the child process tree it created; two disposable background probes were cancelled without touching unrelated processes. |
| Bounded event parsing | PASS | Each sample had four allowed events; largest event line was 160 UTF-8 bytes. |
| Rate-limit classification | PENDING IMPLEMENTATION | The event/error surface is available, but a genuine rate-limited event was not induced during the harmless probe. |
| Authentication status without secret exposure | PASS | `codex login status` reported only `Logged in using ChatGPT`; no authentication file was read. |

T-0028's stated provider gate is satisfied. T-0028B may introduce a
provider-neutral abstraction, and T-0028C may implement the Codex CLI adapter.
Rate-limit classification remains a T-0028C fake-process and parser test
requirement rather than a prerequisite for this harmless capability probe.

## Disposable Probe Evidence

The probe used the isolated T-0028 worktree with a harmless instruction,
`--sandbox read-only`, and no source edits. The first turn produced:

```jsonl
{"type":"thread.started","thread_id":"<captured-thread-id>"}
{"type":"turn.started"}
{"type":"item.completed","item":{"type":"agent_message"}}
{"type":"turn.completed","usage":{"input_tokens":14661,"output_tokens":11}}
```

The resume command targeted the captured ID and produced the same four event
types with the same `thread_id`. Before and after the resumed turn, Git status
was identical. Raw probe logs remain in a disposable temporary directory and
are intentionally not copied into the repository or review package.

For cancellation, the audit started background CLI process trees solely for
this purpose. The Windows launcher leaves stdin open in that background form,
so the CLI waited for input after emitting `thread.started`; that is a launcher
limitation, not an execution failure. In both cases the audit confirmed the
root PID belonged to the probe, terminated only that root tree, and confirmed
the root had exited. T-0028C must launch direct child arguments and own the
process tree explicitly; it must not use the background shell form from this
audit.

## Official Interface Confirmation

The current official Codex CLI reference classifies `codex exec` as stable and
documents JSONL output through `codex exec --json`, including `thread.started`,
`turn.started`, `turn.completed`, `turn.failed`, `item.*`, and `error` events.
The official non-interactive-mode guide also documents `codex exec resume
--last` and `codex exec resume <SESSION_ID>` for follow-up turns. Those are the
interfaces T-0028 should use after capability discovery succeeds.

Sources:

- [Codex CLI command reference](https://learn.chatgpt.com/docs/developer-commands.md?surface=cli)
- [Codex non-interactive mode](https://learn.chatgpt.com/docs/non-interactive-mode.md)

This confirms the target architecture does not need an undocumented app-server
substitution. It does not overcome the local process-launch denial, and no
unsupported flag or app-server protocol is assumed by this audit.

## Current CatDesk Architecture Relevant to T-0028

The reviewed baseline has these extension points, which remain unchanged by
this audit:

| Existing component | Current responsibility | T-0028 extension direction after the capability gate passes |
| --- | --- | --- |
| `src/delegated/runtime.rs` | `ProviderClientV1`, normalized provider events, worker-session snapshots, tool dispatch history | Introduce a Codex CLI provider behind the existing provider boundary without changing Qwen semantics. |
| `src/delegated/integrated.rs` | CatDesk-owned integrated worker loop, journaled mutations, verification and final review | Add the autonomous controller only after provider event/session semantics are proven. |
| `src/delegated/journal.rs` | Durable run records, ordered events, tool-call outcomes, state transitions | Persist Codex thread ID, provider cursor, rate-limit backoff, escalation, and lease state. |
| `src/delegated/contracts.rs` | `ExecutionContractV1`, provider policy, approvals, run states | Add `AutonomousDevelopmentContractV1` as a separate versioned contract/policy surface. |
| `src/delegated/supervisor.rs` and `src/mcp.rs` | Delegated-run supervisor tool surface and MCP handlers | Add cursor-based autonomy contract/session tools without weakening existing supervisor approvals. |

Existing provider-neutral design and independent CatDesk verification are a
good fit for T-0028. They are insufficient by themselves to claim Codex CLI
support: the provider's stable launch, event, resume, cancellation, and
rate-limit behavior must be established first.

## Risk Register

| Risk | Severity | Current control | Required resolution |
| --- | --- | --- | --- |
| CatDesk cannot launch the Store-packaged Codex executable | Hard stop | No source change has been made; no workaround attempted | Install or make available a supported standalone Codex CLI that normal user processes can launch. Approval is required before any installation. |
| CLI flags/schema could be assumed incorrectly | High | No flags have been hard-coded | Capture `--version`, `exec --help`, `exec resume --help`, and a disposable structured event sample from the supported CLI. |
| Session continuity could be lost | High | No provider adapter exists | Prove documented resume behavior before implementation; otherwise T-0028 must fail closed rather than start a new session. |
| Credentials could be exposed by diagnostics | High | No authentication data was read | Keep future capability checks limited to redacted status/output and process-scoped configuration. |
| Desktop app-server may be a different interface from CLI exec | High | Treated as separate and unproven | Do not substitute the app-server protocol; that would require a new architecture decision and revised scope. |

## Conclusion

T-0028A has established branch/worktree isolation, the existing CatDesk
extension map, a supported standalone CLI, JSONL event parsing, durable thread
capture, same-session continuation, and owned-process cancellation. The
remaining work is implementation and test coverage, starting with T-0028B.
