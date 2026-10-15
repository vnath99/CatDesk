# T-0485 R21 — Broker-owned exact-CI fetch before INTENT (source only)

## Change

The production `BrokerLedgerGuard::accept_with_observation` now verifies the detached approval against pinned policy/observations, fetches its signed exact CI tuple itself, compares complete GitHub run and attempt-job inventories, and only then calls `begin`, which may create INTENT. The network bytes and endpoint cannot be supplied by the client or worker. Existing fixture-only acceptance uses an injected test seam under `#[cfg(test)]`.

The WinHTTP transport is fixed to `api.github.com:443` and the compiled `vnath99/CatDesk` repository and workflow. It constructs only bounded run/job paths from the validated signed SHA/run/attempt; it selects no caller URL, proxy, credential, cookie, redirect or authentication. It retains default certificate validation, enables revocation checking, requires HTTP 200, bounds each response and the total bytes, and refuses malformed/incomplete/changed before-and-after inventories or missing/failed Windows jobs. A 30-second ledger wait fails closed; one process-wide worker permit prevents piled-up network threads if a synchronous native call outlives that wait. A late network worker owns no ledger handle and cannot write INTENT.

## Verification

- Full independent Wake crate: 189 library tests passed, one opt-in live network probe ignored by default; binaries/integrations passed with two other manual tests ignored. Strict all-targets/all-features Clippy, formatting and staged diff check passed.
- Offline transport fixtures cover exact fixed path order, invalid signed tuple before network, non-200 statuses including redirect/rate-limit/auth/server errors, oversized/incomplete/changed inventory, deadline and the single-worker permit. A ledger fixture verifies a CI rejection leaves no INTENT.
- One opt-in, read-only native WinHTTP probe against **older** successful run `38646210548` at SHA `4e9a2e212c47b0b7229b41874a3b1ec5bfd7d083` passed in 1.16 seconds. It validates the native request/options against that public API; it is not exact-current-source CI acceptance.
- Independent source review found no acceptance bypass in the fixed transport, permit, comparison or pre-INTENT ordering.

Source commit `d66d22a` was pushed and had zero GitHub workflow runs and zero check runs on bounded readback. The prior manual dispatch HTTP 422 text and missing `workflow_dispatch` trigger remain separate facts; no new dispatch, permissions change or account action was attempted.

## Remaining gate

The separately signed service/policy/protected namespace is unprovisioned and the current SCM runtime still refuses every transaction before INTENT. A trustworthy connected dev.84 owner/store observation, installed bootstrap and public approval root, signed exact-current approval, full package/pointer/owner/reader transaction, adversarial worker-denial probes, exact-commit three-job Windows CI, and fresh Chat53 acceptance are absent. The broker fetcher cannot manufacture CI when GitHub schedules no run. Both install guards remain closed; the dev.85 candidate remains inert.
