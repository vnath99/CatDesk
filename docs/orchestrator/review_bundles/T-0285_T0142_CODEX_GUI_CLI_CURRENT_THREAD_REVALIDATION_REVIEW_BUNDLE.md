# T-0285 / T-0142 current Codex GUI-CLI thread revalidation

## Prior versus current interoperability boundary

| Concern | T-0091 evidence | Current revalidation |
| --- | --- | --- |
| Existing visible Codex history | App-server discovery/re-read/resume adopted an exact existing thread without GUI scraping. | `host_prepare_codex_app_server_thread` selects only the durable project binding, or a completed verified session for the exact project/workspace, then revalidates it through the current-user app-server. |
| Canonical identity | Exact CWD, Terra/High, idle/direct-input-safe, and uniqueness checks were required before durable adoption. | The same exact-CWD Terra/High preflight remains in `bind_codex_app_server_thread`; project/session disagreement, active ownership, model/effort/cwd drift, and duplicate project-thread ownership fail closed. |
| CatDesk continuation | Later worker work had to resume the adopted ID. | The autonomous controller sets the persisted provider session to the canonical ID and invokes `resume_turn` for every Codex new-DAG or repair turn when the binding exists; it rejects handle/poll ID drift rather than overwriting it. |
| GUI/history coupling | No GUI scrape, private session-file parse, or browser automation was used. | The current path still uses only app-server metadata/turn APIs and durable bounded `providerThreadId`, `expectedCodexThreadId`, and `codexContinuity` metadata. It does not create/select a ChatGPT conversation or depend on browser wake. |

## Current canonical thread path

1. Host-side app-server preparation canonicalizes the project workspace and
   requires a durable project binding or a trusted completed session for that
   exact project/workspace. The current session cannot bootstrap authority.
2. The current-user app-server performs exact thread metadata/resume validation
   (exact CWD, required Terra model and High effort, direct-input-safe and
   non-concurrent).
3. The selected ID is atomically persisted to the autonomous session and the
   project registry. The registry enforces one project per thread.
4. Controller task selection intentionally remains independent of repair
   classification: any Codex task with a persisted binding resumes the exact
   session ID. A different provider-reported ID escalates fail closed.
5. The CLI emits `exec resume <captured-thread-id>` for continuation. The
   diagnostic deliberately redacts the raw ID and cannot turn it into a
   caller-selected thread or new thread creation.

The current source therefore shares Codex's supported thread/history identity
surface, not a browser conversation URL or wake route. External GUI behavior
remains an operator-visible acceptance concern and is not simulated as a live
browser action here.

## New deterministic regression evidence

- Both app-server continuity sequences issue `turn/start` against the same
  existing thread ID, preserve exact CWD/Terra/High/read-only/no-network
  policy, and contain no browser or wake authority.
- The CLI resume execution diagnostic contains only the fixed captured-thread
  slot after `exec resume`; it neither exposes the raw thread ID in diagnostic
  data nor uses new-session color setup.
- A durable session-store reopen retains matching `providerThreadId`,
  `expectedCodexThreadId`, and bounded two-turn continuity evidence. The
  existing state validator rejects continuity evidence that disagrees with the
  provider binding.
- Existing controller regressions additionally prove new DAG tasks and repairs
  resume the same canonical ID, and mismatched resumed handle/poll IDs remain
  fail closed.

## Verification

| Gate | Result |
| --- | --- |
| New app-server, CLI-resume, durable-history tests | PASS |
| Existing canonical DAG resume and app-server binding tests | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (839 unit tests plus integration suites) |
| `cargo build --all-targets --all-features` | PASS |
| `cargo check --release --bin catdesk` | PASS |
| `git diff --check` | PASS |
| Separate local `rust_full` runner | No standalone runner was configured/discoverable; equivalent prescribed Rust-wide commands were run directly. |

The local Rust invocations emitted the pre-existing non-fatal `could not
canonicalize path C:\\Users\\Volap` warning. `git diff --check` returned success
with existing CRLF working-copy warnings only.

## Attribution and prohibited live actions

T-0285-attributable test hunks are limited to
`src/delegated/codex_app_server.rs`, `src/delegated/codex_cli.rs`, and
`src/delegated/autonomy_state.rs`, plus this bundle. Those source files already
contain unrelated dirty-worktree changes; no broader changes are attributed,
staged, committed, reset, cleaned, or published.

No browser wake, ChatGPT conversation creation/selection/target mutation,
app-server live probe, Secure-MCP/tunnel action, host activation, signing or
provenance work, dedicated-producer work, or Git publication occurred.

## Conclusion

`READY_FOR_INDEPENDENT_T0142_ACCEPTANCE` - deterministic source and fixture
evidence supports current same-thread GUI-compatible continuity. Request
ChatGPT independent final review before treating operator-visible GUI/CLI
interoperability as accepted.
