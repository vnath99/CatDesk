# T-0154 — Canonical Release Recovery Self-Healing Review Bundle

## Status

**SOURCE IMPLEMENTATION COMPLETE; FULL PROJECT VERIFICATION PASSED; LIVE FAULT-INJECTION ACCEPTANCE PENDING.**

This ticket addresses the real 2026-08-18 production failure in which `target/release/catdesk.exe` contained the reviewed A264C8DC... build while `target/release/catdesk.exe.sha256` still contained historical 532E05F4..., causing `CANONICAL_HASH_MISMATCH`. Because the public recovery facade validated the broken pair before entering recovery, `status`, `recover`, promotion, and MCP continuity all became operator-relay work.

The current worktree is already dirty from earlier CatDesk tickets. Whole-repository Git diff is therefore not valid task attribution. T-0154 attribution is limited to the exact files and behavior below.

## Task-attributable files

- `scripts/catdesk-release-recovery.ps1` — new internal trusted-release recovery layer
- `scripts/start-catdesk-stack.ps1` — recovery-before-canonical-validation integration
- `scripts/promote-reviewed-catdesk-build.ps1` — persistent LKG capture at reviewed promotion boundaries
- `catdesk.ps1` — allow public start/recover to enter the trusted recovery engine when canonical identity is broken
- `scripts/test-start-catdesk-stack.ps1` — deterministic split-brain/LKG/recovery fixtures
- `scripts/test-catdesk-lifecycle.ps1` — public recovery contract update
- `scripts/test-promote-reviewed-catdesk-build.ps1` — require persistent LKG after a successful promotion
- `tests/recovery_powershell.rs` — Windows integration harness that makes the PowerShell control-plane tests part of ordinary `cargo test`
- this review bundle

## Failure reproduced / architectural defect

The live incident proved that canonical binary and fingerprint could drift independently. The canonical binary was A264C8DC85FA55311E74114158F4F4CD598F6E9E9D0A6CA535E14D18AA649939 while the sidecar contained 532E05F4F772D12C68990BA313EBE5F9A9854ECB5723703CFBEEDDBE99BE3689.

`Get-CanonicalCatDeskIdentity` correctly failed closed, but `Invoke-CanonicalRecovery` called that identity check *before* invoking the recovery engine. The exact condition recovery needed to repair therefore prevented recovery from running.

Historical diagnostics show 532E... had once been a valid canonical release fingerprint, demonstrating that the two canonical files were moved independently across releases rather than one file simply being corrupt bytes.

## Trust model

1. Recovery never hashes the current executable and declares that hash trusted.
2. A recoverable authority is an exact binary+SHA pair previously captured only at the reviewed promotion boundary.
3. Active, validated interrupted-promotion evidence retains precedence over the persistent LKG store.
4. `status` remains non-mutating and fail-closed.
5. `start`/`recover` may restore only an already-trusted reviewed pair, then must re-run strict canonical identity before launching/accepting a daemon.
6. Release recovery contains no browser, tunnel endpoint, provider, authentication, credential, or network operation.
7. The official OpenAI Secure MCP runtime remains externally owned; existing lifecycle code only verifies/reattaches and must not duplicate or kill it.

## Persistent last-known-good store

New internal helper: `scripts/catdesk-release-recovery.ps1`.

Workspace-local layout:

- `.catdesk/release-recovery/slot-a/`
- `.catdesk/release-recovery/slot-b/`
- `.catdesk/release-recovery/current.json`

Each valid slot contains exactly the reviewed release evidence:

- `catdesk.exe`
- `catdesk.exe.sha256`
- `manifest.json` schema 1 with `generation`, `sha256`, fixed source `reviewed_promotion`, and `capturedAtUtc`.

The pointer contains schema, slot, generation, and SHA only.

### Crash semantics

The inactive slot is written first with exclusive FileStreams and `Flush(true)`, then fully re-read and hash-validated. Only after a complete slot exists does the small pointer change atomically. The previous valid slot remains intact.

Consequences:

- crash before pointer replacement → previous pointer/slot remains authority;
- crash after complete new slot but before pointer replacement → previous pointer remains authority;
- missing/corrupt pointer → only a unique highest fully valid generation may be reconciled;
- ambiguous generations fail closed;
- damaged inactive slot is ignored and cannot displace a valid authoritative slot;
- same reviewed hash with missing/corrupt pointer repairs the pointer rather than unnecessarily creating another generation.

All recovery paths reject reparse-point control/slot files, non-regular files, malformed or oversized metadata, zero/oversized binaries, hash mismatches, bad schemas, wrong source, invalid generations, and containment escapes.

## Promotion integration

`promote-reviewed-catdesk-build.ps1` now uses the reviewed promotion boundary as the sole LKG-seeding authority.

Before canonical disk mutation, after existing candidate/canonical process proof:

1. validate current canonical binary+sidecar pair;
2. persist current canonical as LKG;
3. create the existing promotion backup/transaction;
4. perform canonical swap.

After swap:

1. prove final canonical listener/path/hash;
2. persist the newly promoted canonical as the next LKG generation;
3. only then clear promotion transaction;
4. only then return `PROMOTED_CANONICAL_READY`.

If persistent recovery capture fails, promotion returns fixed `RECOVERY_SNAPSHOT_UNPROVEN_OPERATOR_ATTENTION` and does not claim production-ready completion. If failure occurs after canonical swap, the durable promotion transaction/prior backup remains for recovery.

## Public recovery behavior

`catdesk.ps1` no longer requires canonical identity before entering `Invoke-CanonicalRecovery`. This exception is intentionally limited to the recovery route; non-mutating `status` still reports a broken identity without changing disk state.

`start-catdesk-stack.ps1 -Mode recover -Execute` now:

1. attempts strict canonical identity;
2. if valid, proceeds normally;
3. if invalid, first tries validated interrupted-promotion recovery;
4. otherwise loads persistent reviewed LKG authority;
5. if current binary bytes differ from LKG, it may stop only the loopback listener whose owner is proven to be `catdesk.exe` at the exact canonical path;
6. restores only the differing canonical binary/sidecar from LKG through staged file replacement;
7. re-runs strict canonical identity;
8. proceeds through existing local-MCP readiness and official-runtime verification;
9. if no trusted authority exists, returns fixed `RECOVERY_RELEASE_AUTHORITY_REQUIRED` instead of guessing.

For the exact real incident shape (binary already correct, sidecar stale), no daemon stop is necessary: only the trusted sidecar is restored, then identity is revalidated.

## Deterministic regression coverage

PowerShell fixtures now cover:

- strict valid canonical pair;
- missing/malformed/mismatched sidecar remains rejected by identity/status;
- first reviewed LKG generation;
- sidecar-only split brain repaired from LKG;
- binary-only split brain repaired from LKG;
- both canonical halves corrupted and restored from LKG;
- second reviewed generation uses inactive slot;
- corrupt pointer reconciles unique highest valid generation;
- same-hash reviewed save repairs corrupt pointer;
- incomplete inactive slot cannot displace valid authority;
- no LKG authority fails closed;
- zero-byte LKG binary rejected;
- reparse-point `.catdesk` recovery parent rejected;
- actual recovery helper repairs the sidecar-only production incident shape;
- recovery engine re-runs canonical identity after repair before normal daemon orchestration;
- public `status` remains non-mutating while `start`/`recover` may enter bounded trusted recovery with broken identity;
- successful promotion must leave a persistent LKG pointer/slot whose binary, sidecar, and pointer all equal the exact proven promoted hash;
- existing interrupted-promotion, rollback, Phase-1 process-continuity, path/hash drift, malformed receipt, reparse, and crash-window promotion tests remain passing.

## PowerShell tests are now part of normal Rust verification

New Windows integration test `tests/recovery_powershell.rs` invokes only these exact checked-in fixture scripts:

- `scripts/test-start-catdesk-stack.ps1`
- `scripts/test-catdesk-lifecycle.ps1`
- `scripts/test-promote-reviewed-catdesk-build.ps1`

This is intentional: CatDesk's host shell allowlist continues to block arbitrary/nested interpreter launches, but ordinary Windows `cargo test` now fails if the release/recovery PowerShell control plane breaks. The safety allowlist was not weakened.

## Verification evidence

### Focused recovery harness

`cargo test --test recovery_powershell -- --nocapture` — **PASS**

Latest log:
`.catdesk/logs/1787104552-2deb5468-9f06-44be-b03a-64d0de85840c.log`

### Complete behavior suite

`cargo test` — **PASS**, including the new recovery PowerShell integration test.

Log:
`.catdesk/logs/1787104631-2291e596-5d7c-46c3-b29c-80b7eb76e2a4.log`

### Full CatDesk verification profile

CatDesk `verify_project` — **PASS** after formatting:

- `cargo fmt --check` — PASS
- `cargo test` — PASS
- `cargo build` — PASS

The test phase includes the PowerShell recovery/lifecycle/promotion fixtures on Windows.

## Live-state non-interference during source work

After source implementation and full tests, `catdesk_transport_status` remained:

- local MCP: `READY`
- transport: `CONNECTED_VERIFIED`
- official runtime monitor active
- existing official tunnel runtime reused; no duplicate created
- 77 tools exposed by the current old canonical daemon.

No T-0154 source edit reloaded/promoted the daemon, altered the live canonical release, restarted the official tunnel, or opened the wake browser.

## Remaining live acceptance gate

Do **not** call T-0154 production-complete solely from fixture success.

Required live sequence:

1. build isolated T-0153+T-0154 release candidate;
2. use the reviewed promotion path, not ad-hoc daemon reload, so the current canonical and promoted candidate each obtain durable LKG evidence;
3. prove local MCP `READY` and Secure MCP `CONNECTED_VERIFIED`;
4. inject a controlled sidecar-only mismatch matching the real failure;
5. run exactly `./catdesk.ps1 recover` once;
6. require automatic trusted-pair repair and return to READY/CONNECTED_VERIFIED with no manual hash inspection/copy/write;
7. confirm the official Secure MCP runtime PID/ownership was not duplicated;
8. then perform T-0153's live ChatGPT-busy five-minute retry canary;
9. then T-0155 repeated soak.

## Independent review recommendation

**Accept T-0154 as source-complete and verification-complete, but not yet production-complete.** The former architectural deadlock—recovery requiring valid canonical identity before it could repair canonical identity—is removed. The new trust authority is persistent reviewed promotion evidence, not current-disk self-blessing. Live promotion/fault-injection acceptance is still mandatory before unattended recovery can be considered trustworthy.
