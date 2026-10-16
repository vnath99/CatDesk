# T-0485 R32 — Authenticated GitHub Actions read-only audit

## October 16 evidence

- The repository `vnath99/CatDesk` is active. Authenticated `gh api` readback reported Actions permissions `enabled:true`, `allowed_actions:"all"`, and workflow `311012404` (`.github/workflows/ci.yml`) `active`. Both the earlier and current committed workflow declare `push` and `pull_request`, with no `workflow_dispatch`. A manual workflow dispatch is therefore not an acceptance probe for the configured workflow.
- The exact earlier CLI stderr retained at session-log line 2971 was: `could not create workflow dispatch event: HTTP 422: Actions has been disabled for this repository. (https://api.github.com/repos/vnath99/CatDesk/actions/workflows/311012404/dispatches)`. The raw HTTP body and headers were not retained. This message is a separate observation from the missing manual trigger; it neither proves that the trigger caused the 422 nor establishes that Actions is administratively disabled now.
- The latest visible successful push run was `38646210548`, created October 14, 2026 at 22:14 UTC for older source `4e9a2e2`. A bounded read-only check at remote branch tip `711192a` found zero exact-commit workflow runs and check-runs. That older success cannot accept the newer source. No run/check result for the subsequently integrated Wake source `002c792` is claimed here.
- The billing/account API returned HTTP 404 because the authenticated CLI token lacks `user` scope. Billing and account restrictions remain unknown. No dispatch retry, scope refresh, Actions setting change or billing change was made.

## Gate and owner action

Exact-source three-job Windows CI remains open. The repository owner should inspect the authenticated Actions page and account notices. If GitHub confirms an administrative disable despite enabled repository settings, provide GitHub Support with the 422 stderr, workflow ID and trigger configuration, the older successful run, newer SHA readbacks with no runs/checks, and the enabled/active API readbacks. Continue local source verification without treating it as remote CI acceptance.
