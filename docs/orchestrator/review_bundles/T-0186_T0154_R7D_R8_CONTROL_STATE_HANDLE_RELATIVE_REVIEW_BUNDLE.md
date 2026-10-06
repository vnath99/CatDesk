# T-0186 — reviewed-build control-state handle-relative authority

## Scope and rejection addressed

T-0184 and T-0185 were rejected because green tests did not establish that the
reviewed-build authority path was free of pathname reopens.  T-0186 closes only
the build-attempt control records beneath
`.catdesk/reviewed-build-control/<buildAttemptId>`:
`attempt.json`, `claim.json`, `worker-owner.json`, `result.json`, and
`attestation.json`.  Candidate/output and promotion-control data-plane work is
intentionally not represented as completed by this bundle.

Before this change, most creates and reads had already moved to
`control_create_json` / `control_read_json`, but the remaining production
control-state residues were:

- `prepare_reviewed_build` tested `control.join("attempt.json")` with a
  pathname existence helper before replaying the attempt;
- `reviewed_build_result`, terminal replay, and producer-attestation
  validation reopened `worker-owner.json` with pathname `read_json`;
- terminal-result detection reopened `result.json` by pathname; and
- the worker replaced `claim.json` with `atomic_json(&control.join(...))`,
  which created a pathname temporary and used pathname `fs::rename`.

## Handle chain and immutable ownership transition

`BuildControlRoot` owns the accepted R7C `ProtectedDirectoryGuard` for the
workspace, `.catdesk`, `reviewed-build-control`, and the exact attempt
directory.  `control_read_json` accepts only one of the five fixed names and
uses the shared RootDirectory/no-follow `read_relative_regular` primitive.
It validates the opened child as a bounded ordinary non-reparse file and parses
only those bytes.  `control_create_json` uses the matching create-new primitive
and writes/flushes through the exact opened child handle.

Prepare now attempts create-new first; on an already-present child it reads the
same exact named child through the pinned control directory and compares the
full immutable binding.  It no longer makes a pathname existence decision.

Claim replacement was removed rather than reimplemented as a weak rename:
`claim.json` is immutable `SPAWN_OWNER_RESERVED`.  The create-new
`worker-owner.json` proof is the durable linearization point for
`WORKER_OWNED_PENDING` and all terminal authority.  The worker must create the
matching proof before source mutation.  A duplicate helper cannot create that
record; a replay observes either claimed-unproven or the exact proven owner.
This preserves one owner without a second mutable claim object, temporary
child, or pathname commit path.

## Negative and replay matrix

| Condition | Result |
| --- | --- |
| Existing exact attempt | Exact pinned-child replay returns `PREPARED` |
| Existing different attempt binding | `REVIEWED_BUILD_BINDING_IMMUTABLE` |
| Claim reserved, no owner proof | `CLAIMED_PENDING_UNPROVEN` |
| Claim reserved with matching create-once owner proof | `WORKER_OWNED_PENDING` |
| Worker/helper owner mismatch or duplicate proof | Fails before materialization |
| Malformed/unsafe control child | Relative child open/read fails closed |
| Terminal record without matching proven owner | Terminal state is rejected |
| Child reparse/substitution or parent identity drift | Shared R7C no-follow/pinned guard rejects it before authority I/O |

The production control names are closed and single-component; callers cannot
select a child path.  The shared R7C guard retains Windows handle-relative
RootDirectory semantics and non-Windows mutation remains fail closed where an
equivalent identity primitive is unavailable.

## Regression coverage

`reviewed_build::tests::control_state_records_never_reintroduce_pathname_authority`
is a source-level regression over the production module.  It fails if one of
the five concrete control records returns to `control.join(...json)`, if
pathname `atomic_json` is applied to the control root, or if generic pathname
`read_json` returns to the control path.  Existing reviewed-build tests retain
the closed child set, immutable claim/owner binding, duplicate/stale owner
proof rejection, and unsafe `.catdesk` intermediate rejection.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0186_T0154_R7D_R8_CONTROL_STATE_HANDLE_RELATIVE_REVIEW_BUNDLE.md`

## Verification

Focused verification run during this task:

- `cargo fmt --check`
- `cargo test reviewed_build -- --nocapture` (10 passed)

Full verification run during this task:

- `cargo fmt --check` — passed;
- `cargo clippy --all-targets --all-features -- -D warnings` — passed;
- `cargo test` — 623 passed, 18 ignored; the two recovery fixtures also
  passed; and
- `cargo build --release` — started but exceeded the 120-second command
  window while compiling, with no compiler diagnostic emitted before timeout.

Every T-0186 control-state requirement is implemented by the pinned control
root plus exact-child create/read model described above.  This bundle makes no
claim that the later reviewed-build data-plane tickets are complete.
