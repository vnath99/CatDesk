# T-0407 R3 GitHub Recovery Mirror Sanitization Closure

## Classification

`RECONSTRUCTION_SNAPSHOT_READY_FOR_INDEPENDENT_REVIEW`.

This closes the T-0407 R2 path-sanitization hold without asserting original
task authorship for inherited reconstruction material. No runtime, Wake,
tunnel, recovery, protected reviewed-build, target, browser, credential, Git
history, commit, or remote state was changed.

## Held-set disposition

R2's exact 60 held paths were re-read. The 12 opaque archives are retained
locally, not extracted, and are now classified `EXCLUDE_ARCHIVAL_BINARY`:

1. `review-bundles/T-0001-persistent-project-memory.zip`
2. `review-bundles/T-0002-planning.zip`
3. `review-bundles/T-0003-safety-controls.zip`
4. `review-bundles/T-0004-verification.zip`
5. `review-bundles/T-0005-session-resume.zip`
6. `review-bundles/T-0006-task-queue.zip`
7. `review-bundles/T-0007-repository-map.zip`
8. `review-bundles/T-0008-git-workflow.zip`
9. `review-bundles/T-0009-terminal-summaries.zip`
10. `review-bundles/T-0010-prompt-templates.zip`
11. `review-bundles/T-0011-hardening-pass-source.zip`
12. `review-bundles/T-0011-qa-final-pass-source.zip`

Of the 48 text/source holds, 47 contained a real `C:\Users\...` historical
literal and were sanitized. The exact changed set is every R2 text/source hold
except `docs/orchestrator/review_bundles/T-0271_T0223_PRINCIPAL_BOUND_SUPERVISOR_PIPE_REPAIR_REVIEW_BUNDLE.md`.
That exception contained only the prose phrase `Authenticated Users`, which
the prior broad `/Users/` heuristic had falsely classified; it contained no
filesystem path and required no edit.

Historical documents, review bundles, logs, and root comments use the clear
`<USER_PROFILE>` historical placeholder. The production/source changes are
limited to:

- `scripts/chat27_network_visual_probe.py` — derives its profile from
  `LOCALAPPDATA`.
- `scripts/probe_codex_app_server_candidates.py` — derives its workspace from
  its repository-relative script location.
- `src/delegated/codex_app_server.rs` — neutral Windows fixture root.
- `src/delegated/github_bootstrap.rs` — expands the fixed `%USERPROFILE%`
  policy form during validation and derives the per-user GitHub CLI candidate
  from `LOCALAPPDATA`.
- `wake/src/runtime.rs` — neutral Windows fixture root.
- `wake/examples/env_diff_link_probe.rs`,
  `wake/examples/linker_location_probe.rs`, and
  `wake/examples/protected_link_env_probe.rs` — derive Rustup locations from
  `USERPROFILE` rather than embedding a local user name.

The generated `src/daemon-reload-approval-request-v1.json` remains
`EXCLUDE_RUNTIME_GENERATED` because it binds a local candidate path and hash.

## Regenerated manifest

`docs/orchestrator/T-0407_R2_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json` was
regenerated as schema version 2:

- SHA-256: `bd5ea6b42ddf628203ad0d4312cb626edf784993b574fd6be286466770b0dfb7`
- byte length: `471955`
- classified entries: `947`
- `INCLUDE_RECONSTRUCTION`: `934`
- `EXCLUDE_ARCHIVAL_BINARY`: `12`
- `EXCLUDE_RUNTIME_GENERATED`: `1`
- `HOLD_SECURITY_REVIEW`: `0`

Every manifest path is normalized workspace-relative and bound to a SHA-256
and byte length. The manifest itself remains a control record outside its own
entries to avoid self-referential hashing.

## Redacted include-only scan

The 934 exact included entries were scanned again:

| Check | Result |
| --- | --- |
| private-key blocks | 0 |
| GitHub/OpenAI/AWS credential shapes | 0 |
| unresolved user-specific absolute paths | 0 |
| runtime-root named paths | 0 |
| generated binary/archive/database extensions | 0 |
| reparse points | 0 |
| bearer-shaped matches | 3 reviewed source/documentation fixture literals only |
| browser-state lexical matches | 2 source-code handling literals only |

The three bearer-shaped matches are the previously reviewed redacted fixture
literals in `src/openai_tunnel.rs`, `src/server.rs`, and the T-0407 R1 review
bundle. No raw value is recorded here. No included path contains a browser
profile, cookie, authentication state, tunnel runtime state, or credential.

## Verification

| Gate | Result |
| --- | --- |
| `codex_app_server::tests` | 18 passed, 1 explicitly ignored live probe |
| `github_bootstrap::tests` | 21 passed |
| Wake `runtime::` focused profile | passed |
| Wake examples offline check | passed (inherited warnings only) |
| lifecycle fixture script | passed |
| `cargo fmt --all -- --check` | passed |
| strict workspace Clippy | passed |
| `cargo test --workspace --all-targets --all-features` | passed (1,000 root tests; live-mutating tests explicitly ignored) |
| `git diff --check` | passed |

Neither `python` nor the Windows `py` launcher is installed in this provider
environment, so the two sanitized diagnostic Python scripts could not receive
a local interpreter syntax check. Their edits are limited to standard
environment/repository path derivation; this is an environment limitation,
not a runtime action or an authority expansion.

## Exact safe-to-commit selection

Subject to independent review, the exact selection is the 934
`INCLUDE_RECONSTRUCTION` manifest entries, plus:

1. `docs/orchestrator/T-0407_R2_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`
2. `docs/orchestrator/review_bundles/T-0407_R3_GITHUB_RECOVERY_MIRROR_SANITIZATION_CLOSURE.md`

The 13 manifest exclusions are not selected. No commit or push was performed.
