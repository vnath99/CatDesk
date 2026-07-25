# CatDesk Delegated Worker Setup And Release Review

Status: T-0023 documentation
Date: 2026-07-25

## Current Posture

CatDesk owns the delegated worker loop. OpenClaw is not required for v1. Remote
or browser providers remain optional future adapters and must stay behind the
CatDesk disclosure, tool, patch, and verification boundaries.

Do not publish a release from this branch. This sprint produces local commits
for review only.

## Runtime Pieces

| Area | Primary Doc |
| --- | --- |
| Worker runtime and provider-normalized events | `docs/orchestrator/WORKER_RUNTIME_AND_OLLAMA.md` |
| Execution contracts and event protocol | `docs/orchestrator/EXECUTION_PROTOCOL_V1.md` |
| Journal and idempotency | `docs/orchestrator/JOURNAL_AND_IDEMPOTENCY.md` |
| Context, compaction, and disclosure policy | `docs/orchestrator/CONTEXT_AND_HANDOFF.md` |
| Patch and diff protocol | `docs/orchestrator/PATCH_AND_DIFF_PROTOCOL.md` |
| Patch engine behavior | `docs/orchestrator/PATCH_ENGINE.md` |
| Run coordinator gates | `docs/orchestrator/RUN_COORDINATOR.md` |
| Provider routing and fallback | `docs/orchestrator/PROVIDER_ROUTING.md` |
| Supervisor MCP surface | `docs/orchestrator/SUPERVISOR_MCP_SURFACE.md` |
| Long-running jobs | `docs/orchestrator/LONG_RUNNING_JOBS.md` |
| Fault-injection and security matrix | `docs/orchestrator/FAULT_INJECTION_SECURITY_TESTS.md` |

## Ollama And Qwen

The v1 live local-provider path is Ollama with Qwen. The normal test suite does
not require Ollama to be running. The live smoke remains ignored by default and
must be run intentionally:

```powershell
cargo test delegated::runtime::tests::ollama_qwen_live_smoke_returns_normalized_response -- --ignored --nocapture
```

Expected local prerequisites for that live smoke:

- Ollama is already installed by the operator.
- The chosen Qwen model is already available locally.
- No paid provider credential is required.
- No remote disclosure is required.

If Ollama or the model is missing, keep the live smoke skipped and rely on the
deterministic fake-provider tests until the operator approves local setup.

## Provider Adapters

Provider adapters must normalize outputs into CatDesk runtime events and must
not receive filesystem, shell, Git, patch, or browser authority. CatDesk remains
the only execution boundary.

Adapter requirements:

- expose capabilities and health;
- report provider type as local API, remote API, browser, or fake;
- emit normalized text, tool-call, completion-claim, cancel, malformed, or
  terminal-error events;
- preserve provider session identity where supported;
- accept only bounded context bundles;
- return credential environment-variable names only, never values.

Remote and browser adapters require explicit `RemoteAllowed` disclosure policy.

## Context And Disclosure Policy

Context is assembled from contract summaries, bounded file excerpts, bounded
command output, artifact references, and compact checkpoints. Repository
instruction files are treated as untrusted project content and labeled for
prompt-injection risk.

Secrets are redacted before inspection or handoff when they match the supported
assignment patterns, such as `api_key=value`, `token=value`, `secret=value`, or
`password=value`.

## Patch And Diff Protocol

Models propose structured replace operations. CatDesk validates scope,
preimage hashes, stale base state, and conflicts before applying a patch.

Important rules:

- target paths must be inside allowed paths;
- forbidden paths are rejected;
- stale preimage hashes reject the patch;
- ambiguous replacement matches reject as conflicts;
- child patches can be compared against parents;
- model completion claims are rejected unless verification passed and the real
  diff is non-empty.

## Restart Recovery

The journal is durable JSON/JSONL state. Restart recovery restores active runs,
event cursors, tool-call records, patch proposals, and patch applications.

Mutating tool calls cannot be replayed after completion. `OUTCOME_UNKNOWN`
records require supervisor attention before any further mutation can continue.

## Long-Running Jobs

Long-running jobs use the existing CatDesk shell safety validation before spawn.
Each job has a durable record, bounded log polling, local stdout/stderr files,
one-generation log rotation, process-tree cancellation, and restart recovery
that marks missing running processes as `LOST`.

## First-Run Disposable Tutorial

Use a disposable repository before connecting the delegated loop to important
projects.

```powershell
$scratch = Join-Path $env:TEMP "catdesk-disposable-worker"
Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path (Join-Path $scratch "src") | Out-Null
Set-Content -LiteralPath (Join-Path $scratch "src\bug.txt") -Value "answer=41"
git init $scratch
git -C $scratch add .
git -C $scratch -c user.name="CatDesk Test" -c user.email="catdesk@example.invalid" commit -m baseline
```

Smoke-test expectations:

- project state and checkpoints are created under disposable paths;
- read-only actions do not mutate files;
- mutating actions require the configured CatDesk gates;
- forbidden commands and paths are rejected;
- patch previews show scope and stale-base failures;
- actual diffs are generated by CatDesk, not the model;
- long-running jobs can be cancelled;
- final review does not push or merge.

Clean up only the disposable path after review:

```powershell
Remove-Item -LiteralPath $scratch -Recurse -Force
```

## Security Limitations

- Remote and browser providers are not the default posture.
- Browser adapter implementation and login handling are deferred.
- OpenClaw remains optional research and is not part of the required v1 runtime.
- The current supervisor surface is an in-process tested surface, not a fully
  authenticated production MCP deployment.
- Redaction is pattern based and should not be treated as a guarantee against
  every secret format.
- The long-running job manager is a primitive, not a full scheduler.
- Release publishing is explicitly out of scope for this sprint.

## Release Review

Before any future release candidate:

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Optional local-provider smoke:

```powershell
cargo test delegated::runtime::tests::ollama_qwen_live_smoke_returns_normalized_response -- --ignored --nocapture
```

Review all sprint commits locally and confirm no branch was pushed, opened as a
pull request, merged, released, deployed, or published without explicit operator
approval.
