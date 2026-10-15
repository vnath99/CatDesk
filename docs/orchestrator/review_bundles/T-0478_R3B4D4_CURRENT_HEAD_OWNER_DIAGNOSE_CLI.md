# T-0478 D4 — Current-head, read-only candidate owner diagnosis CLI

Date: 2026-10-15. Status: source-only forensic route; no cleanup or live recovery accepted.

## Command and source boundary

`catdesk.exe --reviewed-candidate-owner-diagnose --source-workspace <exact source repository> --expected-commit <lowercase 40-hex current branch HEAD> --snapshot-id <canonical UUID>` is the only exposed diagnostic shape. The parser rejects absent or duplicate required values, unknown options, noncanonical snapshot IDs and invalid commit shapes. It accepts no journal path, candidate root path, manifest, replacement owner or cleanup option.

On Windows, the route canonicalizes the source repository, fetches a fresh witness for the fixed `orchestrator/chatgpt-codex-autonomous-loop` branch at the requested exact HEAD, and requires the matching latest push run of the fixed CI workflow to have all three named Windows jobs successful. It then reads only the two fixed local Git objects (`Cargo.toml` and `src/main.rs`) at that witnessed commit to reconstruct one candidate manifest for the requested snapshot ID. It checks that manifest's source commit and snapshot ID against the witness/request before calling the D3 read-only owner classifier. The JSON report labels `BRANCH_HEAD_AT_FETCH`, gives the canonical diagnostic-process `tempParent`, scopes classification to `EXACT_SNAPSHOT_IN_DIAGNOSTIC_PROCESS_TEMP_PARENT`, and states `DIAGNOSTIC_ONLY` authority. A failure exits without a diagnostic report.

The D3 classifier may report `NO_JOURNAL`, `INTENT_BEFORE_ROOT`, `BOUND_BEFORE_SERVING`, `OWNER_PROCESS_ALIVE`, `ORPHANED_OWNER_EXITED`, `ORPHANED_PID_REUSED`, `RETIRED_JOURNAL_PRESENT` or `ATTENTION`. Even an orphan result is evidence for reviewed recovery only. This command never starts or adopts a worker, takes a cleanup lease, removes a journal/root, or grants object disposition.

## Temporal limit and verification

The fixed witness requires the requested SHA to be the **remote branch HEAD at fetch time**. After the branch moves, this command cannot diagnose residue from a historical commit through its present route. A future historical diagnosis would need a separately reviewed, immutable authority path. The classifier uses the diagnostic process's canonical `%TEMP%`; `NO_JOURNAL` means absent only beneath that reported parent, not globally absent. Supplying an old commit or a different temp parent cannot bypass these limits.

The source includes parser regressions for the exact option shape, swallowed mode marker, flag-shaped values, duplicate/unknown/cleanup or malformed inputs, plus a JSON scope regression. The existing D3 classifier has focused live, exited, PID-reuse, residue and legacy-record tests. The final post-review full root all-features Rust suite exited 0 (main binary: 1,154 passed, 26 ignored; integrations passed); strict all-target/all-feature Clippy, rustfmt and diff check passed. A source binary invocation against predecessor `48a62de` and a canonical snapshot ID exited 2 before classification with `GitHub CI for expected commit is missing`, as intended. A successful end-to-end CLI diagnosis of a real CI-witnessed candidate has not occurred; the exact next source commit's full three-job Windows CI is pending.

## CI and release hold

Pushed predecessor `48a62de0ca0cc6398a46855bb111231edb4761b4` has zero GitHub check runs; explicit workflow dispatch returned HTTP 422, `Actions has been disabled for this repository`, despite API `enabled=true` and an active workflow. [GitHub documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-github-actions-settings-for-a-repository) says a separate GitHub-controlled disabled state may require GitHub Support review. Until an exact successful three-job witness exists, this CLI fails closed before local classification. The P0 Wake broker remains unprovisioned, and no reviewed production recovery is accepted.
