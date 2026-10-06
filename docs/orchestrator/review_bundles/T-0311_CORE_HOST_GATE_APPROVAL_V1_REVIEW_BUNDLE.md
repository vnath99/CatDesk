# T-0311 — Core Host Gate Approval V1

## Scope and policy result

T-0311 implements the operator-selected Option-A authority-domain primitive,
not a live-host gate workflow. `core-host-gate-approval-v1` is distinct from
ordinary `independent_final_review` acknowledgement and from reviewed-source,
reviewed-build, promotion, main-image, wake, provider, queue, or GUI evidence.
No T-0223, T-0222/T-0139, T-0152, or T-0155 gate became accepted.

## Reused authority and domain separation

`AutonomousSupervisorV1::resolve_core_host_gate_review_authority` in
`src/delegated/autonomy_supervisor.rs` first uses the existing acknowledged
independent-review remeasurement boundary. That boundary refuses an unread
record and rechecks completed/verified state, approved contract, completion
verification, final-review digest, completion-artifact attribution, and
current output observation. Only then does it emit a new
`CoreHostGateReviewAuthorityV1` with the fixed purpose
`core-host-gate-approval-v1` and a derived authority digest. The promotion
snapshot itself is not returned through this API and is not an approval.

The derived identity carries exact review record/session, approved-contract
identity (`fnv1a64:`), completion-final-review SHA-256, and remeasurement
SHA-256. It is
not constructible by changing inbox `unread=false`; the supervisor regression
proves unread ordinary final review is refused, and changed current output
alters the derived authority. T-0311's own review bundle is not an issuance
surface: there is no MCP/CLI writer or evaluator integration in this ticket.

## Receipt model and boundaries

`src/core_host_gate_approval.rs` introduces the only accepted vocabulary:

| Field family | Exact requirement |
| --- | --- |
| Domain | schema 1; `CatDesk`; `catdesk`; `core-host-gate-approval-v1` |
| Gate | typed `T0223`, `T0222`, or `T0152`; no wildcard/string form |
| Observation | fixed product-derived observation ID and 64-hex digest |
| Existing live binding | exact T-0224 session/record pair and current target SHA-256 |
| Review binding | exact record/session/contract/completion/remeasurement/derived-authority identities |
| Runtime/replay | build identity, nonzero generation, host session, positive issue time/revision |
| Verdict | typed `APPROVE` only; no `accepted` boolean or prose |

The module accepts no path, command, provider, uploaded receipt bytes,
screenshot, wildcard gate, arbitrary target/hash, or caller assertion. It has
no public transport surface. The existing T-0293 pure evaluator remains
unchanged and is not called by the tests or persistence helper.

## Storage and replay semantics

The dormant future-finalizer helper uses only the compiled relative root
`core-host-gate-evidence/core-host-gate-approvals-v1.json` through
`ProtectedDirectoryGuard` and the pinned/no-follow regular-file helpers in
`src/windows_protected_fs.rs`. Tests use temporary workspaces only; the helper
was never invoked against ProgramData or a host authority in T-0311.

Canonical bounded JSON is atomically written, reopened, and byte-validated.
An exact receipt is idempotent. Same reviewer record with different bindings,
lower/equal conflicting revision, cross-gate replay, malformed/oversized state,
or conflicting receipt bytes fail closed. Missing evidence remains absent and
non-authoritative.

## Tests and verification

Focused hostile tests cover exact idempotency; ACK-only and ordinary-review
rejection; wrong purpose/version/product/project/gate; wildcard-shaped JSON;
wrong observation/T-0224/target binding; cross-gate replay; rollback/conflict;
malformed and oversized storage; and protected reparse refusal when supported.
The supervisor remeasurement regression covers acknowledgement and current
output revalidation. Existing preflight tests still prove live evidence is not
upgraded by this primitive.

Verification passed:

- `cargo fmt --all -- --check`
- focused `core_host_gate_approval`, acknowledged-review, core-host-preflight,
  and protected-filesystem tests
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-targets --all-features` — 893 tests passed
- `cargo build --workspace --all-targets --all-features`
- `cargo build --release --locked --target-dir .catdesk/verification-targets/t0311`
  — PASS (release compile/link evidence; workspace-contained verification output only)

The Cargo commands emitted only the pre-existing workspace canonicalization
warning for `C:\\Users\\Volap`. The isolated target is not a reviewed image,
promotion candidate, deployment authority, or runtime replacement. No default
`target/release` output or live process was touched.

## Attribution and prohibited-action audit

T-0311-attributed source: `src/core_host_gate_approval.rs`,
`src/delegated/autonomy_supervisor.rs`, and `src/main.rs` (module registration),
plus `.catdesk/todo.md`, `CATDESK_MILESTONES.md`, `.catdesk/current_plan.md`,
and this bundle. Unrelated dirty-worktree changes are preserved and not
attributed. No host, browser, wake, target, tunnel, supervisor, protected
production storage, signing/provenance, external project, Git, or publication
action occurred.

## Exact T-0312 boundary

T-0312 may implement the fixed T-0308 capture/ledger/finalizer/read-only
reader integration only after consuming an independently revalidated
`core-host-gate-approval-v1` receipt. It must validate every bound field before
forming the existing evaluator inputs, retain T-0293 as the sole acceptance
engine, and must not manufacture live acceptance or invoke a host action.
