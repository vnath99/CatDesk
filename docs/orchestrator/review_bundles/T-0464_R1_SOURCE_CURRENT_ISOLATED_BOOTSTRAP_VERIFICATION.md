# T-0464 R1 — Source-current isolated bootstrap verification

**Decision:** **PASS — fixed isolated bootstrap-verification profile only.**
This result is neither a reviewed-build attestation nor a release, promotion,
LKG, serving reload, or Wake authorization.

## Exact source and concurrency boundary

- Source HEAD before the build and during post-build provenance readback:
  `3aafa22c1a900fc8fdf4c15d3b8b9fcf126afb48`.
- `git diff` and `git diff --cached` were both empty before invocation and at
  post-build readback; `git diff --check` passed.
- The eight pre-existing untracked paths were preserved. This report is the
  only new task-attributable working-tree path.
- A preliminary read-only process query returned no active `cargo`, `rustc`,
  `link`, or `cl` process rows. No competing direct Cargo writer was observed
  at that time. This is point-in-time evidence, not a lifetime guarantee.
- The current plan records that the earlier direct exact-command invocation
  timed out at its connector boundary with an unknown result. It was not used
  as success evidence and was not retried separately.

## Fixed verifier receipt

The sole build invocation in this task was the repository-defined
`CARGO_BUILD_RELEASE_ISOLATED` profile, with no alternate target or flags:

```text
cargo build --release --locked --target-dir .catdesk/verification-targets/autonomy-release
```

Receipt: exit code **0**, Cargo `release` profile finished in **4.67 seconds**.
Cargo emitted one non-fatal warning, `could not canonicalize path C:\\Users\\Volap`.
No second build, protected reviewed-build operation, deployment, service action,
or runtime control action was invoked.

The fixed profile is represented by the closed `CARGO_BUILD_RELEASE_ISOLATED`
catalog value; source tests reject a caller-appended path. Its output location
is workspace-contained verification material and does not establish any
reviewed-build, candidate, or release authority.

## Bootstrap output readback

Read-only post-build measurement found:

| Field | Value |
| --- | --- |
| relative path | `.catdesk/verification-targets/autonomy-release/release/catdesk.exe` |
| SHA-256 | `1096edfa483ff38c0b115bb3d80b500018b97b6111516d704fb0ef54d4cab855` |
| byte length | `26886144` |
| last-write UTC | `2026-10-10T17:15:10.9154261Z` |

The provenance claim is limited to the successful fixed Cargo process launched
from the clean, exact-HEAD workspace above. The executable was not independently
attested, installed, loaded, started, or compared with a serving process.

## Existing CI and source context

The project checkpoint records complete successful CI for source run
`38066384342` at `3160cd4`, containing the short Cargo-home repair commits
`fa86754` and `3160cd4`; the task's supplied source facts also state complete
CI success for documentation follow-up run `38067618614` at `e7784c4`. Current
HEAD `3aafa22` is the current reviewed documentation/source checkpoint. This
task did not query GitHub; those are existing/supplied CI facts, not a new
network assertion.

## Required guarded roll-forward and rollback boundary

Before any serving transition, a separate independently reviewed
`catdesk_daemon_reload` authority must bind the exact candidate path, SHA-256,
and byte length, then pass its own PREFLIGHT/CONFIRM flow. It must remeasure
the designated image, preserve the existing serving image for its guarded
rollback path, and prove post-reload serving identity/parity. A fresh
source-current reviewed-build authority and a new protected attempt are still
required before any `BUILD_ATTESTED`, promotion, or LKG conclusion.

Do not reuse this bootstrap receipt as daemon-reload approval, skip rollback
safeguards, retry the historical protected failure, or start/rebind Wake. The
external tunnel, installed CatDesk, protected reviewed-build control, and Wake
state were untouched by this task.
