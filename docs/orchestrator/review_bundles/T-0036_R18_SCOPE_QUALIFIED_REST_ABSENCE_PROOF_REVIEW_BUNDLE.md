# T-0036-R18 Scope-Qualified REST Absence Proof Review Bundle

## Authority contract

R18 removes GraphQL from publication authority. Using only the already selected
trusted `gh` executable, preflight and confirmation first issue a fixed
included-header authenticated-user GET. It accepts only HTTP 200, exact login
`vnath99`, and one exact `repo` entry in `X-OAuth-Scopes`.

Only immediately after that proof, CatDesk issues a fixed included-header
`GET repos/vnath99/<policy-repository>` with the reviewed Accept and API-version
headers. HTTP 200 proves EXISTS only when bounded JSON has exact owner/login,
policy name, and a valid opaque ID. HTTP 404 proves ABSENT only with the
preceding same-selected-`gh` account/scope proof. Every non-200/non-404,
redirect, nonzero process, stderr, malformed/duplicate status/header, missing
scope, wrong account, non-UTF8, oversized, malformed JSON, or identity mismatch
fails closed without returning raw output through MCP.

The same REST authority is used at preflight, immediately before journal/create
confirmation, and for post-`REPOSITORY_CREATED` recovery EXISTS proof. R14-R17
canonical workspace, tool slot/path/SHA, private-only mapping, transaction,
origin, and safe branch push controls are unchanged.

## Evidence and residual risk

`src/delegated/github_bootstrap.rs` adds bounded internal stderr/header/body
capture, strict HTTP parsing, fixed scope proof, and fixed repository REST
parsing. Tests cover fixed endpoint argv, scoped 404, exact 200, wrong account
and scope, command/schema ambiguity, and existing resolver restrictions. No
token, environment credential, hosts file, browser state, or auth store is
read or exposed.

Residual operational risk is deliberate fail-closed availability: unavailable
scope headers, a proxy/redirect, rate limit, or API change stops bootstrap
rather than inferring absence.

## Host sequence

1. Build/reload an isolated reviewed candidate and require `CONNECTED_VERIFIED`.
2. Run one BYOVD preflight only; confirm only if all bounded evidence is fresh.
3. Prove the exact resulting origin, then independently repeat for Bug Bounty.
4. Only afterward resume the separately reviewed R12/R13 registration, thread
   adoption, and chat-target initialization sequence.

Local verification includes format, strict clippy, full Cargo tests, and
`git diff --check`. This worker performed no sibling-workspace, GitHub, browser,
tunnel, Scheduler, daemon, release, or Git publication action.
