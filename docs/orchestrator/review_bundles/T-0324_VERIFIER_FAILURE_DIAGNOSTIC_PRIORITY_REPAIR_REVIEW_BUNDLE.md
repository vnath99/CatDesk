# T-0324 verifier failure-diagnostic priority repair

## Classification

`VERIFIER_FAILURE_DIAGNOSTIC_REPAIRED`

This repair awaits independent ChatGPT final review.

## Root cause and bounded repair

The exhausted queued-finalization serving-candidate session recorded
`verification_or_diff_incomplete`, but its durable verification summary began
with large successful `CARGO_TEST` output. `ContractVerifierV1::verify`
previously appended summaries in command order and only then applied the
2,048-byte summary bound, so a later failed-profile diagnostic could be
truncated away. The existing failed command error was already redacted and
bounded to 512 bytes by `execute_profile`; it was only ordered too late.

`src/delegated/autonomy_verifier.rs` now collects successful and failed
profile summaries separately. It serializes the failing closed-world command
profile name and places every failure summary before successful output before
the existing bounded summary is produced. A passed verification still keeps
successful summaries in their original contract order. Command allowlisting,
direct no-shell execution, status determination, output redaction,
per-command/output bounds, authoritative-diff capture, and final-review
requirements are unchanged.

## Regression coverage

- `failure_summary_precedes_large_successful_output_before_truncation` proves
  that an exact late `GIT_DIFF` failure remains at the beginning of a bounded
  summary even when preceding success output exceeds the durable cap.
- `successful_profile_summaries_retain_their_original_order` proves ordinary
  all-success summary behavior remains unchanged.
- Existing UTF-8-bound and sensitive-assignment-redaction tests continue to
  cover the shared bounded/redacted output primitives.

## Verification and attribution

- `cargo test autonomy_verifier -- --nocapture` — PASS: 4 verifier tests.
- `cargo fmt --all -- --check` — PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  PASS (with the existing environment canonicalization warning).
- `cargo test --workspace --all-targets --all-features` — PASS: 930 primary
  tests plus target-specific test binaries; existing platform-dependent tests
  remain explicitly ignored.
- `git diff --check` — PASS; only inherited CRLF conversion warnings appeared.

Attributable implementation: `ContractVerifierV1::verify`,
`failure_summary`, `bounded_verification_summary`, and the two verifier
regressions in `src/delegated/autonomy_verifier.rs`. Attributable output: this
bundle, predeclared as this task's sole `completionArtifactIds` member. Other
dirty-worktree changes are not attributed.

## Prohibited-action audit and next boundary

No daemon reload, SR3N-R3 resume, reviewed-build action, release or protected
state mutation, wake/target/browser action, Secure MCP/tunnel action, Git
publication, or external-project mutation occurred.

The next bounded action is independent ChatGPT review of this repair and its
attributable diff. It may then use the improved durable diagnostic to identify
the serving-candidate verifier's actual failed profile; this repair itself does
not retry, accept, reload, or otherwise mutate that candidate.
