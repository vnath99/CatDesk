# T-0161 / T-0154-R5 - Exact Review Authority Selection Binding

## R4 defects reproduced and removed

R4 selected all `COMPLETED_VERIFIED` sessions and required global cardinality
one. Historical completed sessions therefore blocked preflight. Conversely,
when only one completed session existed, its generic final-review text could be
used for a candidate with no ChatGPT-selected review record or attributed task
output proof. Neither session count nor review prose is promotion authority.

## Exact selector and binding

PREFLIGHT now requires `recordId` in addition to the candidate path and SHA.
The static-schema bridge accepts it only with the exact
`REVIEWED_PROMOTION_PREFLIGHT` decision and rejects any mixed fields. The
record is resolved from CatDesk's durable review inbox, not session discovery.
It must be exactly one acknowledged (`unread=false`) `catdesk` record in
`COMPLETED_VERIFIED` state with `nextAction=independent_final_review` and the
fixed `artifacts/completion.json` reference.

The selected session must be inactive and completed, its contract must be for
`catdesk`, completion verification must be `PASSED`, its final review must be
present, and its exact logical task must declare structured non-empty output
artifacts. The immutable task-output baseline must match the approved contract
hash and artifact set. A bounded SHA-256 authority digest binds record ID,
record/session fields, contract hash, logical task ID, and baseline
observations. The protected authorization now stores both record ID and that
digest. Confirmation reloads the bound record and rejects unread/project/state/
reference/session drift before scheduling.

R4 direct-script refusal, candidate/script/canonical/trusted-PowerShell drift
checks, authorization-to-transaction binding, reviewed-promotion/LKG ordering,
and detached worker behavior are unchanged.

## Changed files

- `src/delegated/autonomy_supervisor.rs`: exact `recordId` schema/bridge and
  review-inbox/session/contract/baseline authority validation.
- `src/daemon_reload.rs`: protected authorization now carries review record ID
  and exposes bounded preflight authority for confirmation revalidation.
- `scripts/promote-reviewed-catdesk-build.ps1`: validates the newly bound
  record identifier as part of the exact protected authorization schema.

## Local verification

`cargo fmt --check`, strict all-targets/all-features clippy, and `cargo test`
(587 passed, 18 ignored; 2 integration tests passed), plus the fixed promotion
and bootstrap PowerShell fixtures passed locally. No live promotion, reload, recovery,
tunnel, browser, Scheduler, external-project, protected-state, or Git action
occurred.

## Host acceptance

After independent review, acknowledge exactly the selected CatDesk review-inbox
record, build/load the isolated candidate, then use only first-class PREFLIGHT
with its `recordId`, CONFIRM, and RESULT (or the exact cached bridge). Never
use direct PowerShell or substitute another historical completed session.
