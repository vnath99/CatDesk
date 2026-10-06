# T-0407 GitHub Recovery Mirror Publication Readiness

## Classification

`PUBLICATION_NOT_READY` for a whole-worktree commit. This is a fail-closed
publication assessment, not a request to discard the reconstruction worktree.
The tree contains 757 tracked-modified or untracked candidate paths, including
497 documentation paths and 469 historical review bundles. Their provenance
cannot be established safely as one commit from this session.

## Bounded session actions

The interrupted, unreviewed `diagnose` facade addition was removed from
`catdesk.ps1` and its dedicated assertions were removed from
`scripts/test-catdesk-lifecycle.ps1`. Established autostart seams with similar
names were not changed.

Three deterministic publication-readiness fixture repairs were necessary:

- `src/mcp.rs` uses the existing feature-gated `Store::open_scoped_for_test`
  for the designated-chat rollover fixture.
- `wake/src/bin/CatDeskWakeHost.rs` and
  `wake/src/bin/CatDeskWakeProfileProbe.rs` accept a fixture parent only when
  compiled with non-production `test-support`; normal builds retain
  `Store::open` exactly.
- `wake/tests/python_bridge_validation.rs` selects the current deterministic
  browser-cleanup contract test rather than a removed selector.

No runtime state, protected state, browser profile, tunnel state, executable,
or credential material was deleted, moved, or changed. No commit or push was
performed.

The subsequent independent-verifier `CARGO_FMT` failure was repaired only by
running the repository's deterministic `cargo fmt --all` formatter. That
mechanical formatting action made no source-policy or runtime decision.

## Exact publication selection

The exact safe-to-commit set is **empty**. No full-file Git commit is approved
from this dirty worktree until a separate snapshot/attribution review
establishes ownership of each inherited change. The substantive session paths
below must be reviewed together rather than silently mixed with inherited
work; the workspace-wide formatter also made mechanical changes to inherited
Rust files and is not asserted as substantive T-0407 authorship:

1. `.gitignore`
2. `catdesk.ps1`
3. `scripts/test-catdesk-lifecycle.ps1`
4. `src/mcp.rs`
5. `wake/src/bin/CatDeskWakeHost.rs`
6. `wake/src/bin/CatDeskWakeProfileProbe.rs`
7. `wake/tests/manual_cli.rs`
8. `wake/tests/profile_probe_cli.rs`
9. `wake/tests/python_bridge_validation.rs`
10. `docs/orchestrator/review_bundles/T-0407_GITHUB_RECOVERY_MIRROR_PUBLICATION_READINESS.md`

All other modified/untracked reconstruction material is retained locally and
excluded from this bounded selection, not classified as disposable. In
particular, `KEYGEN.ps1` contains no embedded key but is a key-generation
helper; it is withheld pending focused security review. Browser/profile-named
source and test files are likewise source candidates, not evidence that any
profile state is safe to publish.

## Retention and cleanup

The retention policy supports tracking source, tests, architecture documents,
review bundles, milestones, and handoff notes; none was removed. Proven
rebuildable/transient directories were ignored without deletion:

- `/target-debug-*/`
- `/wake/target-*/`
- `/.pytest_cache/`

`git check-ignore -v` confirmed those rules cover the observed
`target-debug-dev57-catalog`, `wake/target-chat34-review`, and `.pytest_cache`
material. `.catdesk/**`, `.codex/**`, `target/**`, `wake/target/**`, and local
logs remain excluded under the established retention policy.

## Credential and machine-private audit

A value-redacted scan covered 753 textual publication candidates for private
key blocks, GitHub/OpenAI/AWS token shapes, and bearer credentials. It found no
private-key or credential-shaped value. The only pattern matches were
intentional source/test redaction and fixed fake-token literals in
`src/openai_tunnel.rs` and `src/server.rs`; no runtime credential was exposed.
Filename review identified only profile-related source/tests and historical
documentation, not browser storage, cookies, installed state, or keys.

## Verification evidence

| Gate | Result |
| --- | --- |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1` | passed |
| focused designated-chat rollover regression | passed |
| Wake `manual_cli` and `profile_probe_cli` fixture regressions | passed |
| `cargo test --test wake_local_runtime` | passed |
| `cargo test --workspace --all-targets --all-features` | passed (the live-mutating local Wake control probe remained ignored) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed |
| scoped Rust formatting for the session's Wake Rust files | passed |
| `git diff --check` | passed |
| `cargo fmt --all -- --check` | passed after deterministic workspace formatting repair |

The formatter was run solely to resolve the independent `CARGO_FMT` failure.
It does not resolve the separate whole-worktree provenance problem: the broad
retained reconstruction tree still requires an attribution review before any
GitHub recovery-mirror commit.

## Required next step

Before publishing a recovery mirror, obtain a reviewed source snapshot or
per-file attribution for the retained reconstruction tree, then repeat the
redacted scan and verification against that frozen selection. The present
session is deliberately not a commit or publication authority.
