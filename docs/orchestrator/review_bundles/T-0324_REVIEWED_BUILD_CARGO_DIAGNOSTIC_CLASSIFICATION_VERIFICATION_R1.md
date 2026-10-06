# T-0324 reviewed-build Cargo diagnostic classification verification R1

## Classification

`CARGO_DIAGNOSTIC_CLASSIFICATION_VERIFIED`

This is an independent verification of the exhausted classifier-repair
candidate. No fresh source defect was found, so no product source, test, or
control-state change was made in this verification session.

## Candidate and scope

The reviewed candidate is the bounded diagnostic change described by
`T-0324_REVIEWED_BUILD_CARGO_DIAGNOSTIC_CLASSIFICATION_REPAIR.md`. It is
limited to `src/reviewed_build.rs`, whose current SHA-256 is
`90110c7cfbf51a142f13917ed4a1f439194545254792d43e7cc3e1dd25712237`.

The exhausted repair session was not resumed, and historical reviewed-build
generation `8982400e6a444392ac4a51db25c91cc4` was not read for mutation,
retried, or otherwise changed. The fixed isolated serving output at
`.catdesk/verification-targets/autonomy-release/release/catdesk.exe` was
explicitly avoided.

## Independent source assessment

The classifier observes every drained Cargo stderr chunk before the existing
4 KiB capture is bounded. It uses an ephemeral 128-byte lowercased
cross-chunk matcher tail, clears that tail before returning, and returns only
one fixed label:

| Observed signature family | Label |
| --- | --- |
| registry plus network/download failure | `CARGO_DEPENDENCY_NETWORK_UNAVAILABLE` |
| lock-file update plus `--locked` | `CARGO_LOCKFILE_OUT_OF_DATE` |
| linker invocation plus failure | `CARGO_LINK_FAILED` |
| Cargo terminal compile indication | `CARGO_COMPILATION_FAILED` |
| any unrecognized stderr | `CARGO_EXIT_NONZERO` |

The persisted result retains the existing phase, exit code, classification,
bounded-capture SHA-256, bounded length, and truncation bit. It serializes no
matcher buffer, raw stderr, environment value, credential, URL, path, or
arbitrary compiler text. The Cargo argv, cleared-environment policy, trusted
tool selection, source binding, owner/claim requirements, one-use token
semantics, and `BUILD_FAILED_OR_AMBIGUOUS` return path are unchanged. Thus an
unknown error remains terminal and generic rather than becoming authority or a
caller-directed execution path.

Focused tests cover a network marker located after the retained prefix, each
fixed causal label, absence of a sentinel secret from serialized diagnostics,
and the generic fallback. No authority or execution surface is broadened.

## Verification evidence

| Profile | Result |
| --- | --- |
| `CARGO_TEST` (`cargo test --workspace --all-targets --all-features`) | passed |
| `CARGO_BUILD_RELEASE` (`cargo build --release --locked`) | passed in 5m 04s; ordinary `target/release` only |
| `GIT_DIFF` (`git diff --check`) | passed |
| `GIT_STATUS` | 172 dirty-worktree entries observed and preserved |

The resulting ordinary release candidate is
`target/release/catdesk.exe`, SHA-256
`06a7798709f8a6bfd81f8c2a6d5615878b1eb5a8d431b815eefe1c2d8aed0c12`,
length `26437632` bytes. This is build verification evidence only; it grants
no serving, release, reviewed-build, wake, tunnel, or host authority.

## Attribution and prohibited-action audit

The sole task-attributable source-tree output is this bundle. The candidate
source and prior repair bundle were preserved unchanged. No reviewed-build
`PREPARE`, `CONFIRM`, or `RESULT`, daemon reload, wake/target action, Secure
MCP/tunnel action, Git publication, signing/elevation, protected-state edit,
or external-project mutation occurred.

## Independent final review request

Request independent final review of this verification record, the closed
classifier vocabulary, and the redaction/fail-closed boundary before any
separately authorized use of the candidate.
