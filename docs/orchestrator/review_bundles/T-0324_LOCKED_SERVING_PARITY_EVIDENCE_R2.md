# T-0324 locked serving-parity evidence R2

## Classification

`FAIL_CLOSED` — the locked isolated output is the running CatDesk daemon, but
the exact repaired-source-to-binary binding required for serving parity is not
available.

## Accepted repair authority

The repair authority is the accepted terminal-failure diagnostic R2 completion
artifact with independent final-review record
`review-adc-t0324-reviewed-build-terminal-failure-diagnostic-r2-20260913-6-independent_final_review`.
That review is `COMPLETED_VERIFIED`. Its immutable reviewed-source snapshot is:

```text
snapshot ID:      5e67e86cdae2ee8d8c6c455140c0f2cf693de3b624b9321c6656be623a3a768a
manifest digest:  c82c1cc0b83848c409039214b7ed5e8041e80cf556c9e065068491cec8db11b7
src/reviewed_build.rs SHA-256: ac334b5127402069bb85164848ab14ca2fbef0ef260181b8d4d421f26a5cabad
src/reviewed_build.rs length:     417435
```

The current `src/reviewed_build.rs` has that same SHA-256 and length and
contains the accepted `descend_or_create` repeated-parent repair. No source
file was changed in this read-only evidence session.

## Read-only process and executable measurements

Windows process inspection identified exactly one `catdesk` process:

```text
PID:             38280
start (UTC):     2026-09-13T04:08:54.6251567Z
executable path: <USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop\.catdesk\verification-targets\autonomy-release\release\catdesk.exe
```

Independent SHA-256 and byte-length measurements of the running executable and
the fixed isolated candidate path are identical because they are the same file:

```text
path:            .catdesk/verification-targets/autonomy-release/release/catdesk.exe
SHA-256:         054617d87a3994fb1367a7a45b551300f1b1ed400762363d0b7f5bc01c3cbe7f
byte length:     26408960
```

This proves why `CARGO_BUILD_RELEASE_ISOLATED` cannot remove and replace the
fixed output: Windows is executing that exact file. No stop, reload, restart,
replacement, rename, rebuild, or measurement-based reuse was performed.

## Fail-closed source-binding gap

The above binary hash and size are cryptographically equal between the running
daemon and the locked output, but they are not a cryptographic binding to the
accepted terminal-failure repair snapshot. The existing accepted candidate
evidence for this binary belongs to the earlier lineage source snapshot:

```text
earlier snapshot ID:      8822fd08956d791dae06e06d9265a96eef51026266e4cdd91efa8e7699ce985b
earlier manifest digest:  fbba33af4a670675951d79aa89bb52dbb1d5710983d0f2c8f9b48b2f6c11812a
earlier src/reviewed_build.rs SHA-256: 59d475fbc497460958f29ea3c5e94c1470c38cf272b46d6e1e29f2c97337faab
earlier src/reviewed_build.rs length:     414960
```

The two reviewed-source measurements differ. No accepted build attestation or
other immutable record binds SHA-256
`054617d87a3994fb1367a7a45b551300f1b1ed400762363d0b7f5bc01c3cbe7f` to
terminal-repair snapshot
`5e67e86cdae2ee8d8c6c455140c0f2cf693de3b624b9321c6656be623a3a768a`.
Therefore filenames, timestamps, process identity, byte equality with the
locked file, and earlier candidate evidence cannot be used to claim repaired
serving parity.

## Non-mutating verification and boundaries

Only the contract-approved `GIT_STATUS` and `GIT_DIFF` checks are run after
this artifact is written. The only task-attributable source-tree mutation is
this predeclared evidence bundle; the large existing dirty worktree is
preserved. No live reviewed-build PREPARE/CONFIRM/RESULT, wake/target change,
tunnel/Secure MCP action, signing/elevation, Git publication, or external
project action occurred.

The exact unresolved prerequisite is an independently accepted immutable
build-to-terminal-repair-snapshot binding. A later separately authorized flow
must establish that binding without reinterpreting this earlier binary as the
repaired candidate.
