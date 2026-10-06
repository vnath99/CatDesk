# T-0314 Isolated Release Verifier Profile Repair

## Scope and root cause

T-0314 repairs one autonomous-verifier profile defect. Before this change,
`AutonomousCommandProfileV1::CargoBuildRelease` mapped only to the immutable
argv `cargo build --release`. `ContractVerifierV1::verify` executes every
required profile through `exact_argv`, then the controller requires PASSED
verification and a non-empty authoritative diff before final review.

That legacy default-target command could not independently reproduce T-0312's
safe isolated build: it attempts to replace the locked live default release
output. T-0312-R2 therefore has externally green safe verification evidence,
but is not formally autonomously accepted. This is a verifier-profile defect,
not a T-0312 ledger defect and not a live-gate acceptance.

## Fixed profile matrix

| Serialized profile | Immutable argv | Purpose |
| --- | --- | --- |
| `CARGO_BUILD_RELEASE` | `cargo build --release` | Legacy/default release profile; unchanged. |
| `CARGO_BUILD_RELEASE_ISOLATED` | `cargo build --release --locked --target-dir .catdesk/verification-targets/autonomy-release` | Workspace-contained isolated compile/link verification only. |

The new enum variant is `CargoBuildReleaseIsolated` in
`src/delegated/autonomous_contract.rs`. It supplies no path, suffix, ticket,
session, buildPath, environment, or caller argument input. The fixed target is
relative, contains no parent component, and is distinct from `target/release`.
Like every profile, it is direct-process execution under the existing
workspace-contained policy check; there is no shell, command chain, or nested
shell route.

## Authority and runtime boundaries

`.catdesk/verification-targets/autonomy-release` is disposable verification
output only. It is never a deployment candidate, reviewed image, main image,
LKG, promotion input, signing/provenance artifact, or runtime selection source.
This source change does not update the currently running daemon/verifier or
make it understand the new profile. It performs no release build itself and
does not touch the default `target/release` executable.

`ContractVerifierV1` still calls `profile.exact_argv()` for each contract
required command and still obtains an authoritative diff independently.
`AutonomousControllerV1` still rejects completion unless verification is
PASSED and the diff is non-empty; its final-review checkpoint remains required.
No generic command-profile or caller-selected command surface was added.

## Changed files and regressions

- `src/delegated/autonomous_contract.rs`
  - adds only `CargoBuildReleaseIsolated`;
  - maps it to the sole static isolated argv; and
  - adds `isolated_release_profile_is_closed_world_and_legacy_release_is_unchanged`.
- `CATDESK_MILESTONES.md`, `.catdesk/current_plan.md`, `.catdesk/todo.md`, and
  this bundle record the bounded repair.

The regression proves serde round-trip for `CARGO_BUILD_RELEASE_ISOLATED`,
exact argv including `--locked` and the fixed target, unchanged legacy argv,
relative/no-parent/non-default target shape, rejection of an unknown caller-like
serialized profile, and policy rejection when a caller substitutes a target
argument. The existing controller regression
`completion_requires_independent_verification_and_diff` preserves the PASSED
plus authoritative-diff completion condition.

## Verification

Passed locally:

- focused `isolated_release_profile_is_closed_world_and_legacy_release_is_unchanged`;
- focused `completion_requires_independent_verification_and_diff`;
- `cargo fmt --all -- --check` after formatting;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
- `cargo test --workspace --all-targets --all-features` (893 tests); and
- `git diff --check` (only pre-existing CRLF warnings).

T-0314 deliberately does not require execution of the legacy
`CARGO_BUILD_RELEASE` profile: that locked default-target behavior is the
defect being repaired. The new profile's exact command is tested as immutable
argv rather than used to create any authority-bearing output.

## Attribution and live-state audit

T-0314 attribution within `src/delegated/autonomous_contract.rs` is restricted
to the command-profile enum/exact-argv hunk and its one regression. The file
already contains unrelated accumulated task-graph, artifact, and provider-model
changes visible in the authoritative diff; those, all prior T-0312/T-0312-R1
source, and the rest of the dirty worktree remain preserved and are not
attributed to this ticket. R2 documentation attribution is limited to the four
documentation files named above.

No daemon reload, deployment, bootstrap, supervisor activation, browser/wake,
target/tunnel mutation, ProgramData write, signing/provenance action, external
project mutation, or Git publication occurred. T-0224 remains accepted;
T-0223 remains `OPERATOR_BOOTSTRAP_REQUIRED`; T-0222/T-0139, T-0152, and T-0155
remain unaccepted.

## Exact next bounded action

Request independent final review of this no-authority-expansion profile repair.
After it is accepted and after T-0312 is genuinely accepted through the repaired
verifier path, T-0313 may be separately approved for fixed product-derived
host-observation capture only. It must not start host acceptance or infer proof
from source tests, review text, provider output, GUI state, or queue state.
