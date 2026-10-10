# T-0463 R2 — Short Cargo-home independent source review

**Decision:** **PASS — source-review scope only.** This is not a protected-build
attestation, release promotion, serving deployment, daemon reload, or Wake
acceptance. Those actions remain blocked pending a fresh guarded reviewed-build
attempt and its exact attestation/result gates.

## Identity and evidence boundary

- Checked-out HEAD is documentation-only commit
  `e7784c49f5e5140e4d8982c30843155ecf5ab2b2`, whose parent is source commit
  `3160cd43ca021c2b6b55c29ca61801b0cd9cfa3e`.
- The reviewed source commits are `fa86754e0524fd4778d37a94aacc434e47be3245`
  (shorten isolated Cargo home) and `3160cd43ca021c2b6b55c29ca61801b0cd9cfa3e`
  (retain the Cargo-home pin for the complete worker lifetime). The bounded
  `src/reviewed_build.rs` diff across those commits is 261 lines: 235 added and
  26 removed.
- At review time, both `git diff` and `git diff --cached` were empty. Eight
  unrelated untracked paths were present and preserved; none is part of this
  review or alters the tracked reviewed source.
- Project checkpoint evidence records GitHub Actions run `38066384342` for
  exact source `3160cd4` as successful: Rust, independent WakeHost Rust, and
  Python Wake/advisor jobs all succeeded, including Rust format, Clippy, the
  isolated PowerShell fixture, and the remaining Rust suite.
- The task's current supplied fact is that later run `38067618614` for
  documentation-only `e7784c4` also succeeded. The local older session note
  still says that run was in progress; this reviewer did not query GitHub, so
  the later success is recorded as supplied CI evidence rather than newly
  independently fetched evidence.

## Observed prior failure and constrained rationale

The old protected attempt remains terminal and was not retried. Its bounded
diagnostic/reproduction evidence identifies `ring 0.17.14`, MSVC C1083, and
`prefix_symbols.h`; the file and parent existed in the disposable cache, with
the full filename measured at 264 UTF-16 units. That supports a Win32/MSVC
path-length hypothesis, not a proof that every original protected stderr line
had the same cause or that the production short-root repair succeeds.

## Source review

### Attempt binding and short layout

`build_attempt_output_root_guard` accepts only a valid attempt identifier. For
active generation controls it additionally requires that the pinned control
generation directory name equals that exact identifier; mismatch fails closed
with `REVIEWED_BUILD_EVIDENCE_DRIFTED`. It then obtains a
`ProtectedDirectoryGuard` at the fixed workspace-local,
attempt-specific `target-verify/rb/<attempt>` path, creates children one
component at a time, and calls `assert_stable` before use. The legacy
unversioned layout remains separate.

`build_target_guard` places only `target` beneath this guarded parent.
`seed_isolated_cargo_home` places `cargo-home` under that same guarded parent,
so unpacked registry source no longer inherits the deep reviewed-control
generation prefix. Neither output/cache directory becomes control, review,
attestation, or promotion authority.

### Pin lifetime and untrusted paths

The worker binds `let (_cargo_home_guard, cargo_home)` before constructing the
Cargo command. The underscore binding intentionally keeps the
`ProtectedDirectoryGuard` alive until Cargo exits; the seeded home is not
reacquired through a mutable pathname. The target, temporary directory, and
isolated cache use guarded no-follow descent and post-descent stability checks.
The caller supplies no output/cache path: the workspace, active pinned control,
and verified attempt identity determine it.

Attempt validation remains exact: attempt digest, review record/authority,
snapshot identity and digests, fixed current policy, and independently
fingerprinted Cargo/Rustc identities are checked before worker execution.
Ownership remains a create-once `claim.json` plus immutable
`worker-owner.json` proof under the pinned control root; the active-generation
pointer is rechecked at diagnostic/process gates. This review found no change
that weakens those compare-and-validate/CAS-style authority boundaries.

### Offline cache integrity and worker environment

The worker command retains fixed `build --release --locked --offline` and a
closed environment with the isolated `CARGO_HOME`. Cache seeding reads a
lockfile-derived closure, validates crate and version components, requires one
shared guarded registry identity, uses regular-file guarded reads, and copies
each sparse-index leaf once per lowercase crate name. Every versioned `.crate`
archive remains SHA-256 checked against the lockfile checksum before a
create-new destination write. The implementation explicitly avoids executing
from ambient unpacked `registry/src`.

## Verification and limitations

The recorded successful CI profiles above are the verification evidence for
the exact reviewed source and documentation follow-up. This read-only R2 task
did not run Cargo, invoke an ignored replay, contact GitHub, retry the
protected build, or change any runtime/deployment state.

Consequently, the following are **not proven here**:

1. a fresh protected reviewed build reaches `BUILD_ATTESTED` using the short
   production root;
2. the path-length hypothesis is the complete root cause rather than one
   condition in the old failure;
3. promotion, candidate measurement, serving reload, recovery, or paired Wake
   target behavior.

## Required next gate

Use only the guarded reviewed-build PREPARE/CONFIRM/RESULT flow with fresh
current-source review authority. Proceed to promotion or serving reload only
after an exact `BUILD_ATTESTED` result and its attestation validate. Do not
replay the old failure, infer successful deployment from CI, or start/rebind
Wake as part of this source-review decision.
