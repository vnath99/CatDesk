# Workspace rebind: native owner gate (October 16, 2026)

Status: `IMPLEMENTED_SOURCE_ONLY` for a **read-only candidate diagnostic**. No
workspace rebind is authorized by this document or command.

The registered Bug Bounty root retains 237 untracked local inputs. A separate
clean checkpoint worktree has a different absolute path and a descendant Git
HEAD. Changing the CatDesk registration would preserve the old root, but the
bound Codex native thread must first be released by its current writer. An
authenticated read-only native resume attempt against the clean worktree
returned `thread-store conflict: thread ... already has an active writer` and
exited 1 before a turn or file edit. An idle ChatGPT app chat or operator
quiescence assertion does not prove native thread-store release.

`catdesk operator codex queue workspace-bind-preflight` accepts the registered
`--project-id`, exact old `--workspace`, `--new-workspace`,
`--expected-old-head`, `--expected-new-head`,
`--expected-binding-sha256`, and `--expected-thread-id` (the bound thread
UUID). It observes the exact old binding and thread, pinned Git executable,
both origins and HEADs, old-HEAD ancestry in the candidate, clean tracked and
nonignored untracked candidate status, ordinary index flags for all tracked files, and
fixed verification inputs. Ignored entries are counted as Git reports them;
their contents are not checked and they do not block this diagnostic. It reports
`BLOCKED_NATIVE_WRITER_RELEASE_UNPROVED`, `readOnly: true`,
`registrationMutation: false`, and `launchAuthorized: false` even when all
candidate checks pass. It creates no queue namespace, lease, marker, job, or
native turn. These observations can drift after the report.

No workspace registry writer is included in this source change. The fixture
shows the diagnostic leaves registration, the operator queue namespace, and
writer markers untouched. It accepts a candidate with an ignored generated
entry and reports its count without inspecting it. It also rejects a tracked
non-profile file whose skip-worktree flag would otherwise hide a modification
from Git status. A
native owner-release signal and a safe exact-thread resume from the
replacement cwd are still missing.

Before enabling a real rebind, obtain an authoritative native writer-release
receipt for the exact bound thread, prove it is not held by another app or
process, and test a bounded exact-thread resume from the replacement cwd
without changing project files. Then independently implement and review a
locked transaction, run current-source regression, and recheck the old and
new roots immediately before a live operator-approved binding. Keep the old
root and all untracked inputs intact.
