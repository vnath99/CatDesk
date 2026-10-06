# T-0324 — Qwen 3.8 continuation/runtime parity investigation

Date: 2026-09-08
Status: OPEN — T-0319 independently accepted; deployment/source skew proven; remediation blocked on trusted reviewed-candidate bootstrap into the pre-T-0299 serving generation

## Scope

Investigate the fresh bounded `qwen3.8:27b` failure observed during T-0322, where turn 3 failed before mutation with Ollama HTTP 500 `no user query found in messages` after CatDesk tool-result history. Determine whether current repository history serialization is defective or the live/deployed runtime is not at parity with current source. Do not trust Qwen for substantive implementation until this ticket closes.

## Evidence collected

1. Current repository source contains the Qwen 3.8 successful-tool-result continuation behavior expected by the prior compatibility fix.
2. The focused regression was rerun correctly as:

   `cargo test qwen38_successful_tool_result_gets_non_authority_user_continuation -- --nocapture`

   Result: **1 passed, 0 failed**.

   An earlier invocation in this investigation used Cargo `--exact` with an unqualified test name and therefore executed zero matching tests. That filtered run is explicitly discarded as evidence; only the subsequent 1/1 execution is counted.
3. Current CatDesk transport is `CONNECTED_VERIFIED`, local MCP is `READY`, and CatDesk remains attached to the already-running official Secure MCP runtime. No tunnel/runtime replacement was performed.
4. The live transport identity reports binary fingerprint `203534cfbdd8b1bd`, but reports `gitCommit=unknown`, `buildState=unknown`, and `dirtyBuild=unknown`. The current workspace HEAD is `b958eb9fff4522168ebb1ae4a726209896a27451` and the worktree is intentionally dirty. Therefore the live runtime cannot presently attest that it was built from the current repository generation.
5. Direct host-process enumeration and ad-hoc file hashing were rejected by CatDesk's bounded shell safety policy. Those controls were not bypassed.
6. No active T-0324 delegated implementation session exists. Historical WAITING/PAUSED/DRAFT autonomy records remain control-plane backlog and are not treated as active ownership of this ticket.
7. Codex provider status is `OPERATOR_CONFIGURATION_REQUIRED`; no repeated Codex probe or replacement thread/session was attempted.
8. Source inspection corrected an over-strong inference in the earlier checkpoint. `gitCommit` is populated only from compile-time `GIT_COMMIT` or `VERGEN_GIT_SHA`, and `buildState`/`dirtyBuild` depend on compile-time `GIT_DIRTY`; the repository contains no in-tree producer for those variables. `unknown` therefore does **not** itself imply a stale deployed binary.
9. `binaryFingerprint` is also not a release SHA-256. Current source derives it from the running executable path plus file length and modification time and then applies the short connection fingerprint. It is suitable as a redacted runtime discriminator, not as reviewed-release provenance.
10. The first-class lifecycle facade `catdesk.ps1 status` was invoked through CatDesk's intercepted read-only lifecycle surface and returned `READY`. This confirms the currently serving stack remains coherent with CatDesk's canonical/recovery lifecycle checks without restarting or mutating the daemon.
11. The canonical reviewed sidecar currently names SHA-256 `A264C8DC85FA55311E74114158F4F4CD598F6E9E9D0A6CA535E14D18AA649939`. That exact binary is already documented by the T-0154 recovery bundle as the old canonical daemon/reviewed build present during the 2026-08-18 incident; T-0300 independently records the same release hash as predating T-0299.
12. `git log -S "push_qwen38_tool_continuation_after_success" --oneline --all -- src/delegated/integrated.rs` returns no committed introduction, while a bounded `git diff -G "qwen38|tool_continuation" -- src/delegated/integrated.rs` shows `push_qwen38_tool_continuation_after_success`, the Qwen 3.8 model-family gate, and `qwen38_successful_tool_result_gets_non_authority_user_continuation` as additions in the preserved dirty worktree.
13. Because lifecycle `status` proves the serving daemon is the valid canonical release and that canonical release is the old A264... reviewed binary, while the explicit continuation compatibility implementation exists only in newer dirty workspace source, the serving daemon cannot contain the current continuation fix. Deployment/source skew is therefore proven for this compatibility path. No live daemon mutation was needed to establish this.
14. Post-T-0363 reconciliation on 2026-09-08 confirmed the cumulative recovery gate is independently accepted and the sanctioned task queue now marks T-0319 closed. A direct read of the expected current producer-issued `.catdesk/promotion-control/reviewed-build-attestation.json` returned **File not found**. This is bounded evidence that the new independent recovery verdict did not itself mint reviewed-build producer attestation/promotion authority. It does not prove every historical protected artifact is absent, but it confirms the exact current attestation expected by the protected promotion consumer is not present at that canonical location.
15. The serving connector catalog remains the legacy 77-tool generation. No first-class reviewed-build/promotion or stable-supervisor lifecycle tool became available after T-0363, and no current T-0324 mutator owns the workspace. The known T-0322 delegated record remains historical `CANCEL_REQUESTED` deployment-skew evidence rather than authority to delete protected state.

## Current conclusion

The fresh live Qwen failure is **not reproduced by the current repository's focused continuation regression**, and the remaining parity question is resolved: the serving canonical daemon is an older reviewed A264... release, while the explicit Qwen 3.8 post-tool user-continuation implementation and regression are newer dirty-worktree additions. The prior `gitCommit=unknown` / `binaryFingerprint` reasoning was insufficient, but the canonical SHA plus Git attribution closes the gap cleanly.

T-0324 remains open only for remediation/acceptance, not diagnosis. T-0319 is independently accepted through the separate Codex/Terra-high T-0363 durable verdict and is now marked closed through the sanctioned stable-ID task API, so recovery review no longer blocks this ticket. The remaining blocker is narrower: the still-serving 77-tool pre-T-0299 generation does not implement the reviewed-build compatibility bridge and therefore cannot mint the current review -> immutable snapshot -> fixed reviewed-build attempt -> producer attestation -> protected promotion-preflight chain. The post-T-0363 attestation check also confirms that no current producer-issued promotion attestation appeared merely because the recovery verdict was accepted.

The accepted T-0306 authority graph still forbids substituting mutable dirty source, verification-only binaries, raw legacy reload, direct promotion-script execution, protected-lock deletion, or self-minted authorization. The correct remediation remains to carry an independently reviewed candidate through the protected reviewed-image mechanism once a legitimate bootstrap authority exists, reconnect without taking Secure MCP ownership, and then rerun the bounded live continuation canary.

T-0364 classifies that bootstrap boundary as `TRUSTED_T0299_BOOTSTRAP_AUTHORITY_NOT_DEMONSTRABLE_FROM_CURRENT_SAFE_SURFACE`: T-0215 is a stale signed epoch-1 image, T-0217 is an unsigned historical epoch-2 request, no current workspace-visible reviewed-build chain exists, and protected-host receipt/image state is unobservable rather than inferred absent. The only legitimate cross-generation route is a separately independently reviewed T-0299-or-newer payload with its exact product-root-signed canonical envelope, followed only by separately authorized administrator fixed-path consumption.

## Safety / authority disposition

- No browser wake bridge invocation.
- No protected wake-target mutation.
- No Secure MCP tunnel recreation or ownership transfer.
- No canonical promotion or LKG authority mutation.
- No dirty-worktree cleanup/reset.
- No Qwen substantive implementation dispatch.
- No Codex retry based solely on the legacy provider-status surface.
- No raw reload/manual binary copy/direct promotion/protected-lock deletion used to bypass the bootstrap boundary.

## Queue disposition

- T-0319 is independently accepted through T-0363 and is now marked done through the sanctioned stable-ID task API; do not create another recovery-hardening/re-review loop absent a new reproducible defect.
- T-0324 is the highest-priority core blocker. The serving generation remains legacy and cannot produce the protected reviewed-candidate/attestation/promotion chain through its exposed MCP surface.
- The unchecked T-0357/T-0358/T-0359 remediation-shaped lines are historical/misaligned queue labels superseded by the explicit T-0319/T-0324 reconciliation section; do not force those ambiguous stable IDs merely to make the Markdown look green.
- Continue other genuinely safe, non-mutating acceptance work around this bootstrap boundary rather than weakening reviewed-release authority.
- Qwen remains implementation-ineligible until runtime/source parity is reconciled and the bounded live continuation canary passes.
