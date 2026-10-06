# T-0036 R1 cross-workspace project registration/bootstrap review bundle

## Implementation

T-0036 now has dedicated `autonomy_project_registry_preflight` and
`autonomy_project_registry_confirm` operations. Preflight accepts an explicit
candidate workspace, project ID, expected Git origin, and profile; it makes no
registry mutation. The central CatDesk registry canonicalizes the candidate,
requires it to be the exact Git top-level root, reads bounded `git` top-level
and `remote.origin.url` metadata only, and rejects symlink/reparse input,
nested roots, drift, aliases, project/workspace/thread conflicts, and malformed
or stale evidence.

The host, not MCP, observes the Codex app-server. It accepts no caller thread,
prompt, executable, auth, sandbox, or command. It requires one exact unowned
thread for the exact canonical cwd and re-checks authoritative Terra/High
metadata through metadata-only resume. A missing exact-CWD exec thread takes
one fixed host-owned read-only/no-tools bootstrap turn, then is re-read using
the same exact-CWD Terra/High gate. Concurrently owned and ambiguous threads
fail closed.

Preflight persists only a short-lived opaque confirmation record with an
SHA-256 structural fingerprint over project/profile/workspace/origin/thread/
model/reasoning evidence. Confirm re-runs bounded Git and host observations,
compares all evidence, serializes concurrent confirmation through a central
lock, and atomically replaces the central registry with a single project record
that already contains its canonical thread. Exact completed replay converges;
expiry, drift, conflict, and partial failure leave the registry unchanged.

Legacy `autonomy_project_registry_bind` retains its exact-unowned-direct-input
constraint. The registry now enforces unique canonical workspace and Codex
thread bindings, preserving the scheduler's one-writer workspace invariant.

## Deterministic coverage

Focused registry tests cover external registration preflight/confirm, no
preflight mutation, exact replay, expiry, evidence drift, wrong Terra model,
workspace conflict, thread conflict, and existing scheduler writer behavior.
The existing app-server tests retain exact-CWD, ambiguity, concurrently-owned,
and Terra/High gates. Live external registration is deliberately not executed.

## Live acceptance procedure and residual boundary

After independent review, CatDesk should build/reload the reviewed candidate
then preflight and confirm each external repository using its known expected
origin. It must observe the returned central registry, exact canonical cwd,
Terra/High thread metadata, and scheduler lease behavior. No BYOVD or Bug
Bounty repository was registered or modified by this worker.

The bootstrap uses the supported app-server `thread/start` metadata shape; if
the loaded host rejects that supported operation or re-read metadata, the flow
fails closed and requires operator review rather than accepting caller data.

Completed locally: `cargo fmt -- --check`, strict all-target clippy, and
`git diff --check` passed. The full suite reported 520 passed, 18 ignored, and
0 failed. CatDesk retains independent verification and all live registration
authority.
