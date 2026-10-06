# T-0407 R6 GitHub publication executor closure

Classification: `READY_FOR_REVIEWED_ACTIVATION` — implementation exists for independent review; this record does not authorize or perform a publication.

## Closed authority

R6 retains the R5 schema-v2 frozen-manifest parser and binds the exact digest
`bd5ea6b42ddf628203ad0d4312cb626edf784993b574fd6be286466770b0dfb7`.
It accepts exactly 934 `INCLUDE_RECONSTRUCTION` paths, 12 archival ZIP
exclusions, and the one runtime-generated exclusion. The executor does not
accept caller-supplied Git executable, origin, branch, path list, command, or
commit message.

`catdesk_github_publication` is a closed supervisor/MCP route with only:

- `PREPARE(recordId, approval)`: resolves independent `COMPLETED_VERIFIED` /
  `PASSED` authority, remeasures the reviewed artifact identity, validates the
  manifest and literal file set, probes fixed origin/branch/HEAD/staged state,
  authorizes typed `GitPush`, and persists `PREPARED` evidence plus a 300-second
  bounded confirmation token/fingerprint in the existing session journal.
- `CONFIRM(confirmationToken)`: revalidates the review, manifest, repository
  evidence, token, expiry, and empty initial index; stages each literal path
  using `git add -- <path>`, verifies the exact staged set, commits only with
  the fixed message, persists `REMOTE_OUTCOME_UNKNOWN` before the only remote
  action, and executes only `git push origin <contract feature branch>`.
- `RESULT(confirmationToken)`: uses fixed `git ls-remote --heads origin
  <feature-branch>` evidence and confirms only exact remote/local HEAD
  equality. An absent exact ref may become `RECONCILED_NOT_APPLIED`; mismatch
  and command ambiguity remain recoverable `REMOTE_OUTCOME_UNKNOWN` states.

No force option, protected-branch operation, shell, raw Git output, credential,
or arbitrary command/path input is representable. The Git subprocess has its
working directory fixed to the canonical workspace and uses null stdin and
discarded stderr. Bounded stdout is consumed only for identity/ref/staged-list
validation and is never returned through MCP.

## Attributable changes

- `src/delegated/github_publication.rs`: durable prepared-token evidence and
  journal state lookup, retaining the R5 parser/gate.
- `src/delegated/github_publication_executor.rs`: production fixed-argv runner,
  PREPARE/CONFIRM/RESULT/reconciliation seam, and fake-runner tests.
- `src/delegated/autonomy_supervisor.rs`: strict catalog schema and dispatch.
- `src/delegated/mod.rs`: internal executor module registration.

## Verification

Focused checks passed in this workspace:

- `cargo test --workspace --all-targets --all-features github_publication_executor::tests -- --nocapture` — 4 passed.
- `cargo test --workspace --all-targets --all-features github_publication::tests -- --nocapture` — 8 passed.
- `cargo test --workspace --all-targets --all-features github_publication_schema_is_closed_to_prepare_confirm_and_result -- --nocapture` — passed.
- `cargo test --workspace --all-targets --all-features multi_tools_list_exposes_run_command_mv_without_move_path_tool -- --nocapture` — passed after adding the closed publication route to the exact MCP inventory expectation.
- `cargo test --workspace --all-targets --all-features` — completed against 1,007 unit tests with no reported failure.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed.
- `cargo fmt --all -- --check` — passed.
- `git diff --check` — passed (only existing CRLF advisory messages).

No real `PREPARE`, `CONFIRM`, `RESULT`, local commit, push, network
publication, branch change, Wake/tunnel/recovery action, or external-project
mutation was executed by this ticket.
