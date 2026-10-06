# T-0036-R24 Unborn-Head Private-Origin Bootstrap Review Bundle

R24 adds an explicit typed `UNBORN_HEAD` state. A failed `rev-parse HEAD` is
not treated as empty history: CatDesk must prove an attached symbolic HEAD and
an exact successful `rev-list --count --all` value of zero. Any detached,
invalid, nonzero, or command ambiguity remains fail-closed.

The state, empty committed-head value, branch, and existing evidence are bound
to preflight/confirmation. Committed histories keep the existing exact private
create → origin add → current branch push path. Positively proven unborn roots
create only the mapped private repository and exact origin, then durably record
`PUSH_SKIPPED_NO_COMMITTED_HISTORY`; no push, index, commit, branch, checkout,
or content mutation occurs.

R14-R23 REST scope authority, trusted tools, policy mappings, journal recovery,
and origin restrictions remain unchanged. No live external action was performed.
Host acceptance requires an isolated reviewed reload, `CONNECTED_VERIFIED`, one
fresh Bug Bounty preflight with positive UNBORN_HEAD/zero-history/ABSENT proof,
confirmation, exact-origin proof, then reviewed registration/adoption steps.
