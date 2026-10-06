# T-0036-R17 GitHub GraphQL Absence-Probe Compatibility Review Bundle

## Implementation

R17 changes only the fixed repository-state request used by the R14-R16
bootstrap preflight/confirmation path. The production `gh api graphql` argv is
constructed once and uses:

`api graphql -f query=<static GraphQL query> -F owner=vnath99 -F name=<fixed policy repository>`

The former malformed bare-query shape is not emitted. The static query and
owner are not caller inputs; the repository name comes only from the existing
two-project policy table.

The parser remains authoritative and fail-closed: only a successful command
with exactly `{"data":{"repository":null}}` proves ABSENT and only an exact
bounded repository ID object proves EXISTS. Process failure, auth/network/API
or rate-limit error, GraphQL errors, malformed/oversized/non-UTF8 data, and any
extra/ambiguous envelope fail as `TARGET_REPOSITORY_PROBE_FAILED`, never as
absence. Confirmation reruns the same proof before journaling/create.

## Security invariants

R14-R16 policy mappings, private-only creation, canonical workspace and Git
root checks, selected Git/gh identity+SHA revalidation, exact `vnath99` auth,
durable partial recovery, exact origin add, and committed current-branch-only
push are unchanged. No generic argv, filesystem authority, mutation, browser,
tunnel, Scheduler, daemon, or Git publication path was added.

## Evidence

`src/delegated/github_bootstrap.rs` now centralizes the exact argv and tests
assert its `query=` argument, fixed owner/name variables, EXISTS/ABSENT paths,
and ambiguous/failed envelope refusal. Local checks completed: format, strict
clippy, focused bootstrap tests, full Cargo tests, and `git diff --check`.

## Host acceptance

After a separately reviewed isolated reload and `CONNECTED_VERIFIED`, the host
may run one fresh BYOVD preflight only. It may confirm only after fully positive
bounded evidence. Any GraphQL, auth, resolver, or evidence rejection stops
without mutation and requires separately approved reconciliation.
