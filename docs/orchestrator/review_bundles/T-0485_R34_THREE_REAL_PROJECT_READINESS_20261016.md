# T-0485 R34 — Three registered project readiness, read-only

Date: 2026-10-16. Status: `TESTED_LOCAL` for read-only inspection; three real queue jobs remain `BLOCKED`. This record does not authorize a dispatch, profile change, registration migration, historical claim reconciliation, or external research action.

## Source and control snapshot

| Item | Readback |
| --- | --- |
| CatDesk control branch | `orchestrator/chatgpt-codex-autonomous-loop` |
| Exact HEAD / tree | `aa307fcdcaa0299b88dbca7724565e3cc42ed2dd` / `f94f3b1f9f0d034b57740bc56016615dc23cae59` |
| Built source image | `target/debug/catdesk.exe` SHA-256 `4AB3F948B2AE2862D331A8DFCEB25545C489B673E2EF25609C36079F1FD4C664`. Commit `aa307fc` changed documentation only after source integration `911dc615`, as `git log --name-only` confirms. |
| Private registry | `.catdesk/projects/projects.json` SHA-256 `6F06C8EE771D9B44D05216D079A7E51B7734F49ED20B56B085BE3FBE73A76FBF`; three registered Git roots, distinct bound Codex threads and ChatGPT target digests, `maximumCodexWorkers:1`. Raw target URLs and thread content are omitted. |
| Current operator queue | `codex queue status`: `jobs:[]`, explicit dispatch only. Scheduler leases: zero; cursor absent. |
| Native toolchain | All three `codex status` calls returned `authenticated:true`, `codex-cli 0.160.0`. Normal-host `uv` resolves to Python 3.12 Scripts and reports `uv 0.12.23`. No registered-root `uv run` was attempted. |

The status field `launchAuthorized:false` is part of a read-only, nontransactional diagnostic with `writerLockChecked:false`. It is not exclusive-lock or owner-handoff proof, including in the two projects with no recorded recovery blocker.

## Registered projects at this checkpoint

| Project | Exact registered root, branch and HEAD | Git witness and active-work indicator | Registration / recovery |
| --- | --- | --- | --- |
| `catdesk` | `C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop`; `orchestrator/chatgpt-codex-autonomous-loop`; `aa307fcdcaa0299b88dbca7724565e3cc42ed2dd` | Tracked/index clean; nine preexisting untracked files preserved. Bound app thread was `notLoaded`; latest observed turn completed 2026-10-10 20:00Z. | Binding SHA-256 `5ff509a5b980a1ecc5dd8765e4bae6720e537c722d3651007410aa0220b95c3f`; profile `rust_full`; `NEEDS_OWNER_RECOVERY`: 35 historical queue blockers, 85 accounting review rows, no pending writer marker. |
| `BYOVD_DRIVER_PIPELINE` | `C:\Users\Volap\OneDrive\Desktop\Projects\BYOVD_DRIVER_PIPELINE`; `codex/october-corpus-triage`; `ac61b689f40cb11528d0f989a42ccb0ff8d6cd72` | Tracked/index clean; six preexisting untracked operator documents preserved. Bound app thread was `notLoaded`; latest turn completed 2026-10-16 10:39Z, with its own goal reported paused. | Binding SHA-256 `e067c01e6731f92d98cb8205ed8e46793e28879fa7041133549c2c7fe54afd34`; still `rust_full`; no recorded recovery blocker. Fixed target profile is `python_uv_group_dev_v1`. |
| `BUG_BOUNTY_RECON_PLATFORM` | `C:\Users\Volap\OneDrive\Desktop\Projects\BUG_BOUNTY_RECON_PLATFORM`; `codex/wake2-public-resource-client`; `a5d7de3538bf7512315bf16f8b1b9bb68ae6249f` | Tracked/index clean; 237 untracked paths. Bound app thread was `idle`; latest turn completed 2026-10-16 13:12Z in ongoing project work. Its separate existing `h1-pilot-readiness` worktree at `5eaec71c93085c6881e2cf6202a847bf7772e44d` retains seven unstaged tracked text files; neither worktree was touched. | Binding SHA-256 `d506e58c28dd8fe3ba4d5fbb85ab4288001e3e968f7e7c1d80db944a90ae7049`; still `rust_full`; no recorded recovery blocker. Fixed target profile is `python_uv_extras_dev_lab_v1`. |

The two Python roots have clean, tracked `pyproject.toml` and `uv.lock`, and their package/dependency declarations match the fixed source profiles. Current SHA-256 pairs (manifest / lock): BYOVD `E97B343AE64B3A1B2CDF5E0C24AEC76FB5AE94EAACFFAD57BD842BF27E8CEAA3` / `CECEC1B5852BC6D438B1DD99DBE65384353180A14F3FC2DAEEA072A10546D9FB`; Bug Bounty `2C342A91FF18CC06902077D2A881D841C08118232D50E8E2AB86C99EEA1FC7C0` / `3894D3973E7279EC1A1C2756E3D6CA330E7155C81559FD46D586C37A5936E0C1`. This checks inputs, not offline resolution or actual Ruff/pytest acceptance.

## Exact read-only probes

Run from the registered CatDesk control root with ordinary host Git identity access. The restricted sandbox returned `CODEX_QUEUE_SCOPE` for the same queue read, while a normal-host read returned the state above. These commands did not write queue or project state:

```powershell
$control = 'C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop'
$cli = Join-Path $control 'target\debug\catdesk.exe'
Set-Location -LiteralPath $control
git rev-parse HEAD
git rev-parse 'HEAD^{tree}'
git status --short --branch
Get-FileHash .catdesk/projects/projects.json -Algorithm SHA256
& $cli codex queue status
& $cli codex status --project-id catdesk --workspace $control
& $cli codex status --project-id BYOVD_DRIVER_PIPELINE --workspace 'C:\Users\Volap\OneDrive\Desktop\Projects\BYOVD_DRIVER_PIPELINE'
& $cli codex status --project-id BUG_BOUNTY_RECON_PLATFORM --workspace 'C:\Users\Volap\OneDrive\Desktop\Projects\BUG_BOUNTY_RECON_PLATFORM'
Get-ChildItem .catdesk/projects/leases -Force
Test-Path .catdesk/projects/scheduler-cursor.json
```

For each registered root, `git status --short --branch -uno`, `git rev-parse HEAD`, `git remote get-url origin`, `git ls-files --others --exclude-standard | Measure-Object -Line`, `git diff --name-only`, `git diff --cached --name-only`, and `git ls-files --stage -- pyproject.toml uv.lock` were read. The Bug Bounty worktree inventory came from `git worktree list --porcelain`; its seven modified tracked paths were confirmed with `git status --short --branch -uno` at the existing `h1-pilot-readiness` checkout. Codex app `list_threads` and `read_thread` were used read-only to check bound thread status and last-turn timestamp; only aggregate state appears here.

## Minimal safe path to three actual, verifiable jobs

1. Obtain a project-owner handoff for each exact bound native thread and its active worktree. An app `idle` or `notLoaded` label and zero recorded blockers do not prove exclusive ownership. Preserve the Bug Bounty worktree's seven-file patch and BYOVD's paused goal; do not resume either silently.
2. Reconcile CatDesk's 35 queue claims and 85 accounting review rows claim by claim with original-owner, direct-child-exit and descendant-quiescence evidence under a reviewed exclusive compare-and-swap transaction. The current read-only `recovery prepare` does not clear or authorize those claims.
3. Make each exact registered root a clean committed Git baseline through its owner-reviewed checkpoint, preserving the nine/six/237 untracked items and the separate Bug Bounty patch. If a clean approved worktree must replace a registered root, use the reviewed registration/thread-adoption authority and preserve the target digest; an arbitrary `--workspace` cannot substitute for the bound path. Do not hide operator files in Git excludes merely to pass the witness.
4. When quiescence and queue/lease emptiness are actually established, bind BYOVD from `rust_full` to `python_uv_group_dev_v1` and Bug Bounty to `python_uv_extras_dev_lab_v1` using `codex queue profile-bind`, each with its exact *current* old binding hash and the exact bound-thread quiescence assertion. Re-read the resulting registration; the assertion itself is unverified. Run the fixed locked/offline Ruff and pytest commands in owned roots before enqueueing and record their exits.
5. Reconfirm clean Git status, exact HEAD, origin, target, thread, profile, no writer marker and no lease immediately before `codex queue add`. Add three distinct scoped task IDs/prompts, then `codex queue dispatch` sequentially from the control root. Record each selected project, actual thread/root, process receipt, lease release, `codex queue verify --receipt-id`, review package and independent decision separately. A source/local verify is not a ChatGPT independent review or Wake approval. Demonstrate pause/resume only on a quiescent READY job and failure isolation after the three authorized baselines are in place.

Source `src/operator_codex_cli/queue.rs` permits `add` before a clean Git witness, but its exact pre-spawn guard refuses an unborn or dirty baseline as `REVIEW_BASELINE_DIRTY_BEFORE_SPAWN`, then records an `ATTENTION` child-absent receipt and releases the lease. Enqueueing the current three roots would therefore produce durable attention work, not a verifiable job. `profile-bind` checks the exact binding, tracked clean manifest/lock, no project jobs/leases and an operator assertion; it does not prove active-chat ownership. No narrow source bug suitable for an isolated repair was found. CatDesk's historical claims still need both claim-specific evidence and a reviewed owner-governed reconciliation path.

No registration, profile, target, thread, queue, scheduler lease, Git worktree, historical claim or external account setting was changed for this audit. No actual job was dispatched and no remote CI acceptance is claimed.
