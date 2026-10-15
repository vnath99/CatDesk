# T-0483 R15 — Host-turn active Sol nonce selection (source only)

**Scope:** `src/delegated/sol_worker_nonce.rs`; dormant handoff protocol. No worker launch, protected installation, review event, browser activity or Wake send.

## Change and authority

One create-once `active-{SHA256(length-prefixed session ID, length-prefixed host turn ID)}.json` record now selects the sole nonce for a trusted host turn. The host writes and syncs that selection **before** writing the issue record and before returning the nonce. `FILE_CREATE` under the same pinned ledger serializes concurrent issuers; a losing issuer creates no issue file. Binding a native Codex turn and verifying output both require the exact canonical issue **and** active selection for the host-held authority. `read_active_sol_worker_nonce` recovers the selected nonce after reopening the ledger, but is not a launch permit and can return a nonce that has already been burned; burn remains one-shot.

The selection record is keyed by session and host turn, while its canonical contents also bind task, contract, baseline and allowed paths. A valid but unselected issue record cannot authorize a native-turn bind or return output. The verifier now returns a private post-burn output type used by dormant staging; direct output verification alone cannot supply that input. If the process dies after selection and before a complete issue write, that turn remains unusable. A retry cannot silently issue a new nonce. The future controller must persist explicit attention/abandonment and use a new reviewed host turn when recovery is authorized.

## Verification

- Focused Windows `cargo test --locked --offline --all-features sol_worker_nonce -- --nocapture`: **11 passed, 0 failed**. Tests include two concurrent issuers (one winner, exactly two ledger files), routine second issue with no residue, valid unselected issue rejection, missing/torn selection and issue refusal, reopen, concurrent burn, native-turn substitution, replay and wrong model/effort.
- Final root and Wake format checks, full all-features suites, and strict all-target Clippy checks passed for the combined source. The main root suite passed 1,169 tests with 26 ignored. Exact-commit GitHub Windows CI and live integration remain outstanding.
- Independent source review found no active-nonce substitution path and requested the issuance race and crash-hold cases, now covered.

## Limits

The ledger is only safe when its pinned directory is outside worker write authority and that separation is verified by the eventual restricted worker service. Parent-directory metadata durability across power loss is not established. The source still deliberately refuses ordinary-process Sol workspace-write launch. Dormant staging exists, but no stopped-worker/lost-write-access proof, crash-safe stage transaction, real Sol deliverable, natural review-ready event or exact Wake receipt has been completed. This source change is not P1 acceptance.
