# T-0299 — Fixed MCP stable-supervisor lifecycle bridge

## Decision

T-0299 closes one deterministic transport gap only. It adds a first-class MCP
bridge to the accepted, closed stable-supervisor lifecycle composition. It does
not activate the host supervisor, accept T-0223 host-live behavior, loosen
shell policy, or add authority to the lifecycle.

## Defect evidence

1. The reviewed lifecycle is implemented by the exact zero-choice operator
   grammar `operator supervisor status|preflight|activate` and its shared
   `execute_supervisor_operator_action` composition.
2. Normal MCP allowlist correctly rejects direct
   `target\release\catdesk.exe operator supervisor ...` execution; allowing it
   would create a generic executable/shell authority and is not a repair.
3. Existing MCP “supervisor” tools are delegated-run controls. They do not
   dispatch the stable supervisor lifecycle. No dedicated bridge previously
   reached `execute_supervisor_operator_action`.

## Design

| Tool | Input | Shared action | Annotation |
| --- | --- | --- | --- |
| `catdesk_stable_supervisor_status` | exactly `{}` | `SupervisorOperatorActionV1::Status` | read-only, closed-world, non-destructive |
| `catdesk_stable_supervisor_preflight` | exactly `{}` | `SupervisorOperatorActionV1::Preflight` | read-only, closed-world, non-destructive |
| `catdesk_stable_supervisor_activate` | exactly `{ "confirm": true }` | `SupervisorOperatorActionV1::Activate` | non-read-only, closed-world, destructive |

These tools are exposed only in the intended `multi-tools` and
`supervisor-only` MCP modes; they are not present in `read-only` mode. The
bridge parses the tool name and its bounded JSON object, then directly calls
the existing `execute_supervisor_operator_action`. It does not reimplement or
fork reviewed-image role binding, principal policy, native startup authority,
Task Scheduler ownership, protected receipt/current/LKG transaction, fixed
ports, private pipe, worker registration/generation, rollback, or recovery.

`confirm` absent, false, non-boolean, or accompanied by any other property is
rejected before the shared lifecycle function is invoked. Status and preflight
reject every property and are side-effect-free. Fixed lifecycle result values,
including `ELEVATION_REQUIRED`, are returned unchanged by the lifecycle JSON;
the bridge never auto-elevates or converts them into a shell request.

## Authority analysis

The schemas have no fields for a caller-selected executable, path, image,
bytes, digest, pipe, port, endpoint, task, SID, session, startup identity,
tunnel, shell command, or generic argument. Activation is explicitly marked
`destructiveHint: true` and `openWorldHint: false`; it remains the exact
transaction already accepted in the lifecycle chain. Secure MCP/tunnel
ownership stays external. Delegated-run supervisor tools are retained
unchanged and remain a distinct family.

## Deterministic evidence

- Tool discovery proves all three tools appear in `multi-tools` and
  `supervisor-only` modes, not `read-only`; delegated-run discovery remains.
- Schema tests prove no-authority read-only forms, sole `confirm` field,
  confirmation const, closed-world schemas, and destructive activation flag.
- Dispatch tests inject the shared executor seam and prove exact
  Status/Preflight/Activate enum routing. Missing/false/extra/path/port/pipe/
  hash/session/array inputs fail before the executor is called.
- MCP handler tests call real status and preflight routing in a fixture
  workspace, reject unsafe activation input without creating `.catdesk`, and
  prove direct target-release CLI text remains rejected by allowlist shell
  policy.

## Queue disposition

The queue contains two checkbox occurrences of T-0296: its historical open
contract entry and a later completed correction. `src/task_queue.rs` rejects
duplicate checkbox IDs rather than choosing one silently. T-0299 leaves them
unchanged: converting one record without a dedicated durable-history migration
would change its historical checkbox meaning. This bounded housekeeping issue
does not affect the bridge and is documented rather than bypassed.

## Changed files

- `src/mcp.rs`
- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- this review bundle

## Prohibited-action audit

No live supervisor activation, ProgramData or Task Scheduler mutation, browser
wake/profile/target action, Secure MCP/tunnel action, external-project action,
signing/provenance/dedicated-producer work, Git publication, or shell-policy
weakening occurred. Tests use only deterministic seams and read-only lifecycle
results.

## Verification

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test --workspace --all-targets --all-features` — passed: 886 tests,
  0 failures (21 explicitly ignored Windows/Python-dependent tests).
- `cargo build --workspace --all-targets --all-features` — passed.
- No standalone `rust_full` runner is configured in this workspace; the
  documented rust full-equivalent fmt/clippy/workspace-test/all-target-build
  profile above was run.
- `git diff --check` — passed. The worktree is intentionally accumulated and
  dirty; T-0299 attribution is limited to `src/mcp.rs`, the milestone/current
  plan/queue updates, and this bundle.

## Exact next host step

After independent review, a separately authorized host-live T-0223 ticket may
deploy/reload the reviewed build, call
`catdesk_stable_supervisor_status` and
`catdesk_stable_supervisor_preflight`, then call
`catdesk_stable_supervisor_activate` only with `{ "confirm": true }` if the
preflight result is ready and non-elevated. If it reports `ELEVATION_REQUIRED`,
park that exact operator boundary; do not auto-elevate, use shell, or substitute
another host authority.
