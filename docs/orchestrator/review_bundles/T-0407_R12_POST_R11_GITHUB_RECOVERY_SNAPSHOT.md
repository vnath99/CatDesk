# T-0407 R12 — Post-R11 GitHub Recovery Snapshot

## Scope and classification

This is a fresh archival reconstruction snapshot of the post-R11 current
source tree. It records source and review evidence only; it grants no Git
publication, activation, release, Wake, tunnel, recovery, runtime, or
protected-build authority.

## Frozen artifacts

- Manifest:
  `docs/orchestrator/T-0407_R12_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`
- Manifest SHA-256:
  `6c8ea66d1ac14ef4eb923040a4e71296baab788357103c3025e580e6ff353597`
- Descriptor:
  `docs/orchestrator/T-0407_R12_GITHUB_PUBLICATION_DESCRIPTOR.json`
- Descriptor SHA-256:
  `df8b12e7798b2ac33c5d7dbf721af229722c5ce0c8be95803982d2c4d06be3f1`

The schema-v2 manifest contains 949 exact
`INCLUDE_RECONSTRUCTION` entries, each with a normalized relative path,
byte length, and SHA-256. It retains 12 opaque historical ZIP files as
`EXCLUDE_ARCHIVAL_BINARY` and retains exactly one generated local approval
artifact as `EXCLUDE_RUNTIME_GENERATED`:
`src/daemon-reload-approval-request-v1.json`.

R10's formerly self-supplemental manifest, descriptor, and accepted refresh
bundle are now ordinary historical reconstruction entries. The R11 authority
review bundle is included as the fourth post-R10 retention entry. The R12
manifest, descriptor, and this review bundle remain separate fixed
supplemental artifacts to avoid self-reference. The resulting future literal
publication set is 952 paths: 949 included entries plus those three fixed
artifacts.

The descriptor binds the manifest identity and the unchanged configured Git
identity: the origin is represented only by SHA-256
`ad0fe55abd1661a15106f8aa635ec753b0579bdc0266def9eb51232b80171cd8`,
the feature branch is `orchestrator/chatgpt-codex-autonomous-loop`, and the
local HEAD is `b958eb9fff4522168ebb1ae4a726209896a27451`.

## Containment and security evidence

- All 949 included files re-opened as regular files under the workspace;
  their byte lengths and SHA-256 values exactly match the new manifest.
- No containment failure, reparse point, included ZIP/binary/generated
  artifact, or unexpected classification was found.
- The exact set contains zero private-key blocks, GitHub-token shapes, and
  AWS-access-key shapes.
- Comparison with the accepted R10 set found zero new credential-shaped or
  user-specific-absolute-path lexical fingerprints. Broad `sk-`/Bearer and
  path scans retain only inherited test, redaction, historical-prose, or
  fixture lexical matches; no credential values or real user-specific paths
  were recorded or exposed.
- The descriptor manifest digest and all 949/12/1 classification counts were
  revalidated, and the current branch/origin-hash/HEAD match the descriptor.

## Verification evidence

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `cargo test --workspace --all-targets --all-features`: 1,015 passed,
  0 failed; existing explicitly ignored platform/live tests remain ignored.

No file was staged, committed, pushed, or published. No Wake, runtime,
tunnel, protected build, toolchain, remote Git, or external-project state was
mutated. Independent final review remains required.
