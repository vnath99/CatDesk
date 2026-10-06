# T-0324 reviewed-source snapshot finalization repair

## Classification

**`REVIEWED_SOURCE_SNAPSHOT_FINALIZATION_REPAIRED`**

## Exact root cause and isolation

SR3N-R3 passed its required verification and had valid declared completion
artifact membership, baseline, and current output attribution. Its controller
then recorded only the bounded public reason
`reviewed_source_snapshot_unavailable`. The failing invariant was isolated
without touching host state:

- the fixed workspace, `.catdesk`, and `.catdesk/reviewed-source-snapshots`
  are regular directories with no reparse attributes;
- the protected, handle-bound read-only reopen of that exact snapshot root
  succeeds; and
- source-input collection on the current workspace returned
  `reviewed source snapshot coverage is ambiguous` before staging creation.

The old collection code counted text occurrences of `include_str!` and
`include_bytes!` and compared that count with a literal-path regular expression.
Because `src/reviewed_source_snapshot.rs` contains those spellings in its own
scanner strings, documentation, and hostile fixtures, it could reject the
repository's own valid source input set before creating a staging directory.
This is a parser defect, not a OneDrive reparse, pinned-root, output-baseline,
or completion-authority defect.

## Repair and authority boundary

`src/reviewed_source_snapshot.rs` now uses a bounded lexical recognizer that
collects only actual normal-string literal `include_str!` and `include_bytes!`
invocations. It skips comments, ordinary strings, raw strings (including byte
raw strings), and closed character literals. A real include macro with a
nonliteral, escaped, malformed, or otherwise unsupported argument remains
`coverage is ambiguous` and fails closed. Every collected path is still
lexically normalized, constrained to the workspace, rejected on an escape or
excluded root, checked with no-follow metadata, and copied through the existing
protected staging/manifest/handle-pinned commit sequence.

No path-only fallback, reparse allowance, mutable-source authority, output
baseline change, or controller finalization relaxation was introduced.

## Regression coverage

- Existing `rejects_dynamic_include_coverage` still refuses dynamic include
  expressions.
- `ignores_macro_like_text_but_captures_real_literal_include` proves strings
  and comments create no phantom dependency while an actual literal include is
  byte-captured.
- `ignores_macro_like_text_in_byte_strings` covers byte-string false positives.
- `current_workspace_source_inputs_are_collectable` proves the exact current
  CatDesk source tree—including its self-scanner, scripts, and fixture
  dependencies—can produce a bounded input set.
- Existing snapshot tests continue to cover manifest/output drift, reparse and
  intermediate substitution refusal, stale staging reclamation, pinned
  cleanup, and exact committed replay.

## Prohibited-action audit and next step

No SR3N reviewed-build PREPARE, CONFIRM, or RESULT was attempted. No protected
host/release/wake/target/Secure-MCP state, ProgramData/Program Files,
browser/tunnel, signing/UAC, Git publication, or external project was changed.

After independent final review of this repair, the safe next bounded action is
a fresh controller finalization/reconciliation of the already verified SR3N-R3
record. It must remeasure the declared output and create/validate the protected
snapshot through this repaired code before any separately authorized
reviewed-build PREPARE.

## Verification and attribution

- `cargo test reviewed_source_snapshot -- --nocapture` — PASS: 23 focused
  snapshot tests.
- `cargo fmt --all -- --check` — PASS (only the environment's existing
  canonicalization warning).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --workspace --all-targets --all-features` — PASS: 926 primary
  tests plus all target-specific binaries.
- `git diff --check` — PASS; only pre-existing CRLF conversion warnings.

Attributable implementation change: `literal_include_paths` and its lexical
helpers plus the three named snapshot regressions in
`src/reviewed_source_snapshot.rs`.
Attributable documentation: this bundle. Existing dirty-worktree files remain
outside this repair's attribution. Independent final review is requested.
