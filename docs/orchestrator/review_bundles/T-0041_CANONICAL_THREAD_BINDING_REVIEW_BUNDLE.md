# T-0041 Canonical Codex Thread Binding Review Bundle

Status: `PARTIAL_PLATFORM_GATE_NO_UNVERIFIED_BINDING`.

## Outcome

T-0041 retained the exact-workspace and single-writer invariant.  No CatDesk
project/thread mapping was persisted because the live supported Codex
app-server connection did not expose a durable, exact-CWD, direct-input-ready
thread that could be verified.  CatDesk did not bind the stale T-0040 thread,
did not bind a title-only candidate, and did not use a credential, cloud, or
billing fallback.

The missing CatDesk registry MCP surface was implemented as the smallest
bounded read/bind pair.  It is source-verified but deliberately not deployed
into the already healthy daemon: a restart cannot produce a valid binding
while the Codex account/app-server access gate remains, and the active Secure
MCP connection must not be disrupted merely to expose an unusable write tool.

## Preserved Worktree And Live Daemon

- Existing T-0030--T-0040 changes were retained; no reset, clean, branch
  change, commit, push, merge, PR, release, or deployment was performed.
- The detached-handoff record identifies the active replacement as PID
  `18548`, with the prior local MCP port `3200` and build hash
  `42cf34d22c5d01b5b3628526bc3c7e47f2550b84e11d4a3627e7f725392cac69`.
- A fresh loopback `GET http://127.0.0.1:3200/` returned HTTP `200`.
- The protocol boundary used was the installed `codex app-server --stdio`
  JSON-RPC server.  It initialized successfully and its generated schema
  confirms `thread/list`, `thread/start`, `thread/setName`, `thread/read`,
  `turn/start`, and `account/rateLimits/read` as the relevant supported
  surfaces.

## Fresh Thread Discovery

Canonical workspace:

`<USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop`

Fresh read-first calls were made after the source-folder change:

| Request | Result |
| --- | --- |
| `initialize` | Success |
| `thread/list` | Success; `result.data=[]`, `nextCursor=null` |
| `thread/list` filtered by canonical cwd and `Integrate Codex MCP for ChatGPT` | Success; zero candidates |
| `account/rateLimits/read` | Supported endpoint returned `codex account authentication required to read rate limits` |

Consequently, the T-0040 candidate `019f4b3a-c8cb-7c50-8a6c-323f603f35ff`
was neither accepted nor inferred from stale metadata.  There was no exact
candidate from which to inspect a direct-input flag or ownership state.  The
task's own active worker thread is separately marked in the CatDesk session
state as in-flight and therefore is not a safe competing writer or a
replacement for the requested user-facing conversation.

## Replacement And Continuity Gate

The supported schema permits a replacement through `thread/start` with an
explicit `cwd`, followed by `thread/setName`, and direct input through
`turn/start`.  One bounded read-only replacement attempt was issued with the
canonical cwd, `approvalPolicy=untrusted`, and the intended title
`Integrate Codex MCP for ChatGPT - Canonical`.  The app-server stream then
failed while awaiting the continuity-turn notification; after reconnecting,
`thread/list` again returned no canonical or titled thread.  The operation
therefore produced no durable exact thread identity, no durable replacement,
and no safe continuity proof.

No retry/new routine thread was created.  In particular, no opaque or
unverified ID was written to the project registry.  A future run may make at
most one new replacement attempt only after a supported authenticated
app-server connection can list/read the created thread and prove the
continuity turn completes.

## Registry MCP Surface

`src/delegated/autonomy_supervisor.rs` now exposes:

- `autonomy_project_registry_read` (read-only)
- `autonomy_project_registry_bind` (write)

The bind operation is intentionally limited to the canonical MCP workspace,
requires `resolutionEvidence=exact-unowned-direct-input`, atomically
registers the project when absent, rejects cross-project thread reuse, rejects
workspace changes, and reloads the registry to verify the stored mapping.  It
does not accept executable paths, authentication material, or billing input.

The mapping remains absent in this live run because the required app-server
evidence is unavailable.  The active daemon predates this source edit; no
restart was performed due to the verified platform gate and continuity safety
rule.

## Usage And Accounting

All account-dependent fields are `UNKNOWN_NOT_CAPTURED` for this run:

| Field | Evidence |
| --- | --- |
| Selected/effective model | No durable replacement thread could be read |
| Reasoning effort | No durable replacement thread could be read |
| Thread token usage | No durable replacement thread could be read |
| Rate-limit windows / used percent / reset | `account/rateLimits/read` required account authentication |
| Plan / credit metadata | Same supported endpoint was unavailable; no inference was made |
| Continuity turn | Not proven; replacement identity was not durable after reconnect |

No credits were bought, reset, consumed deliberately, or inferred from token
estimates.

## Verification

Focused checks passed:

- `cargo test autonomy_supervisor::tests::project_registry_binding_is_canonical_evidence_gated_and_durable -- --nocapture`: PASS.
- `cargo test codex_app_server -- --nocapture`: PASS (4 tests).
- `cargo test autonomy_projects -- --nocapture`: PASS (2 tests).
- `cargo fmt --check`: PASS after formatting.
- `cargo clippy --all-targets --all-features -- -D warnings`: PASS.
- `git diff --check`: PASS (Git emitted existing line-ending warnings only).

`cargo test --no-fail-fast` was run.  It completed with `397` passed, `7`
failed, and `9` ignored.  The seven pre-existing environment-sensitive
failures are unrelated to this change: three advisor tests could not start
their test advisor program, one advisor-cancellation expectation then failed,
and three Windows process-tree cancellation tests received `ERROR: Access
denied`.  The new registry and app-server focused tests passed within that
run.

## Diff Evidence

- Tracked working-diff Git blob: `64a9c7848b469ecddda64f6f81c8811d9496c5ef`.
- Untracked manifest/content SHA-256 excluding this self-referential review
  bundle: `1af6776798fe0f9b0720b7f4228fd4f2472a1fc9be3b9aa755d3576d485d458a`
  across `96` files.
- The working tree remains intentionally uncommitted.

## Remaining Gate

The required external condition is an authenticated, durable supported Codex
app-server connection that can enumerate/read a canonical-CWD thread and
accept a completed direct-input continuity turn.  Until that condition is
available, binding any ID would weaken the exact identity/direct-input/one
writer safety invariant.  CatDesk independent verification remains pending.
