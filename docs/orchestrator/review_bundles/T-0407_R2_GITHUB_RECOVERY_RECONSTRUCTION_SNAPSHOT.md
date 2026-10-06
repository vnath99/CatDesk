# T-0407 R2 GitHub Recovery Reconstruction Snapshot

## Classification

`RECONSTRUCTION_SNAPSHOT_READY_FOR_INDEPENDENT_REVIEW`.

This is an archival recovery baseline, not an assertion that one worker or one
ticket authored the inherited changes. It deliberately permits verified,
sanitized reconstruction inputs from the dirty worktree while retaining a
separate hold boundary for opaque or machine-specific material.

## Frozen manifest

The payload is frozen by
`docs/orchestrator/T-0407_R2_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`:

- SHA-256: `0ea60ab554fb5ea881623d5f935b25b0e789a200f8572d4a0db1ead426b42b75`
- byte length: `471265`
- candidate paths classified: `946`
- `INCLUDE_RECONSTRUCTION`: `885` paths, `15,626,025` bytes
- `EXCLUDE_RUNTIME_GENERATED`: `1` path
- `HOLD_SECURITY_REVIEW`: `60` paths

Every payload entry has a normalized workspace-relative path, SHA-256, byte
length, classification, and rationale. The manifest and this companion review
bundle are control/evidence records and are intentionally outside the payload
entries to avoid self-referential hashing; this bundle binds the manifest's
final digest above.

## Inclusion and holds

The inclusion policy follows
`docs/orchestrator/GITHUB_AND_LOCAL_RETENTION_POLICY.md`: source, tests,
scripts, Wake sources, manifests, static assets, architecture, tickets, and
sanitized review history are reconstruction inputs even where the dirty-tree
change predates this task. No file was deleted, relocated, reset, or cleaned.

The one exclusion is
`src/daemon-reload-approval-request-v1.json`: it is a generated approval
request bound to a local `target-verify` candidate, not reconstruction source.

The 60 holds are 12 opaque historical ZIP archives and 48 text files containing
user-specific absolute paths. They remain local and untouched. Each requires a
separate extraction/redaction or security decision before it can join a mirror;
they are not silently treated as disposable.

`KEYGEN.ps1` is included. It is source-only: it contains no private-key block
or credential-shaped value and writes any generated key material under the
per-user local application-data runtime location, outside this workspace and
outside the snapshot. The reviewed historical signing/rotation helpers are
also source-only reconstruction material; they were not executed and no key,
candidate, envelope, or installed runtime object was read or changed.

## Exact safe-to-commit set

Subject to independent final review, the exact safe-to-commit set is:

1. The 885 paths whose manifest classification is
   `INCLUDE_RECONSTRUCTION` (the manifest is the exact path-and-digest list).
2. `docs/orchestrator/T-0407_R2_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`.
3. `docs/orchestrator/review_bundles/T-0407_R2_GITHUB_RECOVERY_RECONSTRUCTION_SNAPSHOT.md`.

No `EXCLUDE_RUNTIME_GENERATED` or `HOLD_SECURITY_REVIEW` entry is in that
selection. This record authorizes neither a commit nor a push.

## Redacted security and machine-private scan

The repeat scan covered exactly the 885 included payload paths. Results:

| Check | Result |
| --- | --- |
| private-key block | 0 |
| GitHub/OpenAI/AWS credential shape | 0 |
| user-specific absolute runtime path | 0 |
| `.catdesk`, `.codex`, target, Wake-target, or log path | 0 |
| generated-binary/archive/database extension | 0 |
| bearer-shaped lexical matches | 3 source/documentation fixture literals only, value-redacted on inspection |
| browser-state lexical matches | 2 source-code handling literals only; no profile, cookie, local-storage, or authentication artifact |

The bearer-shaped source fixtures are in the prior T-0407 review bundle,
`src/openai_tunnel.rs`, and `src/server.rs`; redacted inspection confirmed they
are test/documentation literals, not credentials. No raw credential, browser
state, URL, cookie, tunnel authentication value, or machine-private path is
recorded in this bundle or manifest.

## Verification

| Gate | Result |
| --- | --- |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1` | passed |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed |
| `cargo test --workspace --all-targets --all-features` | passed (1,000 root tests; live-mutating tests remain explicitly ignored) |
| `git diff --check` | passed |

The formatter was checked only; no broad formatting mutation was performed in
this R2 session. No Wake, tunnel, recovery, protected reviewed-build, release,
LKG, browser, target, credential, runtime, Git branch, commit, or remote state
was mutated.

## Remaining gate

Independent review must validate the manifest and decide whether the 60 held
paths are redacted/extracted in a follow-up. Only then may an operator make a
separate Git commit or push from the frozen safe-to-commit set.
