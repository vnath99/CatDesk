# T-0458 — Unclaimed PREPARED reviewed-build supersession

## Problem

The reviewed-build control plane could deadlock on an active generation that was PREPARED but never CONFIRMED.

Live evidence:
- active generation: `a54292a4764747cfbae628d351491bf7`
- review binding: old T-0415 R2
- `attempt.json` exists and is valid
- `claim.json`: absent
- `worker-owner.json`: absent
- `result.json`: absent
- `attestation.json`: absent

A newer independently reviewed binding (T-0457R1) was rejected because `prepare_reviewed_build` only allowed a different authority to supersede an existing generation after a fully audited terminal failure. An unclaimed PREPARED generation is nonterminal but also has no worker/execution authority, so it could block all later reviewed builds indefinitely.

## Repair

`src/reviewed_build.rs` now defines an exact unclaimed-PREPARED predicate. A generation is eligible only when:
- its attempt digest is internally valid; and
- all of `claim.json`, `worker-owner.json`, `result.json`, and `attestation.json` are absent below the exact pinned active control directory.

PREPARE behavior is now:

1. Exact same reviewed binding + exact unclaimed PREPARED generation -> return the existing confirmation token (idempotent replay).
2. Different reviewed binding + historically revalidated exact active attempt + exact unclaimed PREPARED state -> create one fresh immutable generation and atomically move the active pointer to it.
3. If any ownership, execution, result, or attestation evidence exists -> the unclaimed path is unavailable and existing terminal-audit rules remain authoritative.
4. Retryable terminal failures still use the existing terminal audit/retry path unchanged.

The old generation is never deleted or rewritten.

## Transition mechanism

The repair reuses the existing:
- `generations/` immutable attempt directories,
- durable `retry-plans/` transition record,
- `ReviewedBuildActiveGenerationV1` pointer,
- exact pointer compare-and-swap / atomic replace logic,
- same-authority replay convergence check.

The unclaimed PREPARED source evidence is domain-separated as
`CATDESK_REVIEWED_BUILD_UNCLAIMED_PREPARED_V1`
and hashes the exact prior attempt. No caller-provided state or path is accepted.

## Regression coverage

Focused command:
`cargo test --bin catdesk unclaimed_prepared_ -- --nocapture`

Result: 3 passed, 0 failed.

Covered:
- different reviewed authority supersedes an unclaimed PREPARED generation;
- the prior confirmation token becomes unusable;
- exact new-authority replay converges on the already-published fresh token;
- presence of claim, worker-owner, result, or attestation evidence refuses supersession;
- active-generation pointer drift refuses the transition.

## Safety boundaries

This source ticket does not:
- CONFIRM or run a protected build;
- mutate live reviewed-build protected state;
- promote/reload CatDesk;
- mutate Wake/target/profile;
- stage/commit/push Git;
- bind GitHub publication authority.

## Next

After independent review, use the reviewed activation path to serve this control-plane repair. Then PREPARE T-0457R1 again, CONFIRM exactly once, require BUILD_ATTESTED, and promote the resulting current CatDesk binary. After serving parity, refresh the final current-source GitHub publication snapshot and resume bounded publication.
