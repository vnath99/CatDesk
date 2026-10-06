# T-0364 R5 Reboot Recovery / Promotion Readiness Review Bundle

## Classification

`REBOOT_RECOVERY_PROMOTION_READY_WITH_UNRELATED_WAKE_STORE_TEST_FAILURE`

No defect was found in the reviewed reboot-recovery parameter repair or the
reviewed-promotion/LKG authority boundary. No product source was changed in
this review.

## Exact recovery-script assessment

The current `scripts/start-catdesk-stack.ps1` parameter block uses inert empty
defaults for both relevant inputs:

| Parameter | Param-block default | Late-bound behavior |
| --- | --- | --- |
| `Workspace` | `""` | When empty only, set to `Join-Path $PSScriptRoot ".."` after parameter binding. |
| `BuildFingerprintPath` | `""` | When empty only, set to `$Workspace\target\release\catdesk.exe.sha256` after Workspace initialization. |

The parameter prefix contains no `$PSScriptRoot` reference. The script uses
`$PSScriptRoot` for trusted helper-path resolution only after binding and the
two empty-value initializers. Consequently explicit nonempty `Workspace` and
`BuildFingerprintPath` inputs are preserved rather than overwritten.

`scripts/test-start-catdesk-stack.ps1` parses the bootstrap script, isolates
the source through the `$ErrorActionPreference` boundary, rejects any
`$PSScriptRoot` in the parameter prefix, requires both empty defaults, and
requires the two late-binding statements. The sanctioned
`cargo test --test recovery_powershell -- --nocapture` profile passed 4/4,
including the lifecycle and reviewed-release recovery fixtures.

## Reviewed promotion and LKG authority boundary

The reviewed-promotion script records `reviewed-promotion.json` with stage
`CANONICAL_HANDOFF_PROVEN` only after its canonical pair evidence matches both
the canonical disk hash and the transaction candidate hash. The record binds
the transaction/authorization IDs, candidate hash, fixed canonical relative
path `target\release\catdesk.exe`, and exact canonical SHA-256.

`Initialize-CatDeskLastKnownGoodFromReviewedPromotion` first requires that
exact record through `Get-CatDeskReviewedPromotionAuthority`; malformed,
missing, reparse, wrong-stage, or hash-mismatched evidence fails closed before
an LKG snapshot can be written. The recovery fixture proves that a generic
native reload receipt and a review bundle cannot mint LKG authority, while an
exact `CANONICAL_HANDOFF_PROVEN` record can initialize the reviewed LKG.

Generic `catdesk_daemon_reload` remains a separately exposed destructive
reload path using `prepare_reload`/`execute_reload`; it does not invoke the
reviewed-promotion authority resolver or make LKG authority. Production
release promotion remains the independent reviewed-build ->
reviewed-promotion preflight/confirmation path.

## Verification evidence

| Approved profile | Result |
| --- | --- |
| `CARGO_FMT` (`cargo fmt --all -- --check`) | passed |
| Focused `CARGO_TEST` recovery harness | passed, 4/4 |
| Full `CARGO_TEST` (`cargo test --workspace --all-targets --all-features`) | failed: 927 passed, 1 failed, 21 ignored |
| `GIT_DIFF` (`git diff --check`) | passed |
| `GIT_STATUS` | 176 dirty entries observed and preserved |

The sole full-suite failure was
`stable_wake_owner_mode::tests::independent_target_staging_is_exact_and_preflight_is_non_authoritative`,
which failed at temporary `catdesk_wake::store::Store::open` with
`CONFIG_ROOT_UNAVAILABLE`. It occurs before the test's target staging/assertion
logic and has no recovery-script, parameter-binding, reviewed-promotion, or
LKG authority dependency. This bounded review does not modify unrelated
Wake Store setup.

## Attribution and prohibited-action audit

The only task-attributable workspace mutation is this predeclared bundle.
`scripts/start-catdesk-stack.ps1` and `scripts/test-start-catdesk-stack.ps1`
remain preserved inherited untracked work. No wake send/test, browser action,
target/state mutation, Secure MCP/tunnel action, Scheduler/daemon/reload/release
action, Git publication, signing/elevation, or external-project mutation
occurred.

## Independent final review request

Request independent final review of the recovery readiness conclusion and the
unrelated Wake Store test-environment failure before any separately authorized
reboot or production promotion operation.
