# T-0485 R38 — Public recovery bootstrap audit

Date: 2026-10-16. Verdict: `OWNER_REVIEWED_BOOTSTRAP_REQUIRED`; installed recovery remains blocked. This is a read-only host audit and source review, not recovery acceptance.

## Exact source and installed evidence

Source base `06f347d3e3bf9a87842f6c4d124ca4a9b65f00c4` (tree `9a11122271792b741b607175b2784c7b613ea114`) was inspected in a clean isolated branch. The installed public lifecycle and recovery source files were unchanged from that base during the audit.

The installed `catdesk.ps1 status` exited 0 with `LKG_AUTHORITY_MISSING`. `diagnose` exited 0 with `ACTION_REQUIRED`, primary layer `RECOVERY_AUTHORITY`, and gates `CANONICAL_HASH_MISMATCH` and `LKG_AUTHORITY_MISSING`. Exit zero denotes a valid bounded status response, not a healthy installation.

The canonical `target/release/catdesk.exe` hashes to SHA-256 `9A8F4EBF6EA57CBB61EFAA12E8AFA54D18B8DB7374B9884067325BC84565568C`; its `catdesk.exe.sha256` names `A264C8DC85FA55311E74114158F4F4CD598F6E9E9D0A6CA535E14D18AA649939`. Escrow `slot-a` has a binary, sidecar, manifest and pointer internally matching `A264C8DC85FA55311E74114158F4F4CD598F6E9E9D0A6CA535E14D18AA649939`, generation 1, but its manifest source is `operational_verified`. `slot-b`, the promotion transaction and reviewed-promotion record, and promotion authorization and claim were absent. No listener was observed on ports 3200 or 3201, and no CatDesk or WakeHost process was observed. Final readback found the same canonical and escrow hashes and no promotion directories.

## Authority dependency

`scripts/catdesk-release-recovery.ps1` accepts only `reviewed_promotion` slots as rollback authority. `operational_verified` records runtime health, which cannot retrospectively prove independent release review. Its legacy migration requires both a valid canonical binary/sidecar pair and an exact durable reviewed-promotion handoff record; neither exists here. `scripts/start-catdesk-stack.ps1` can restore a broken pair only from a proven interrupted transaction or reviewed LKG, so the public recovery plan correctly fails closed.

The ordinary reviewed promotion path in `scripts/promote-reviewed-catdesk-build.ps1` validates the prior canonical pair before even planning. Execution also requires a live canonical listener before handoff. A newly built candidate or a review statement alone cannot satisfy those prior-state gates. Copying either observed hash into the other file, relabeling the operational slot, inventing a promotion record, or relaxing the listener check would create authority from unproven bytes. No narrow source-only repair is justified.

The minimum legitimate next project is a separately reviewed first-generation reconciliation transaction. Its owner approval must bind an independently reviewed, exact candidate and signed product-root authority where that policy applies, the exact divergent installed hashes and escrow state, and the intended recovery disposition. The implementation must verify exclusive process/host ownership and absence of competing transactions; stage and journal a crash-recoverable binary/sidecar change; rehash the resulting pair; prove the exact serving handback; and create reviewed recovery authority only after those proofs. Protected signed-image bootstrap or rotation decisions remain separate and were not cryptographically reverified by this audit. These approval, provenance and transaction rules require dedicated design and review before any installed mutation.

## Verification and limits

- `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1`: passed, `canonical bootstrap behavioral tests passed`.
- `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File scripts/test-promote-reviewed-catdesk-build.ps1`: passed, `reviewed build promotion fixture tests passed`.
- The isolated branch remained clean before this document. No source-code fix, installed recovery execution, image/sidecar/escrow change, process or service action, Wake target/pointer change, tunnel action, or workflow dispatch occurred. The fixture results do not establish live recovery or remote CI acceptance.
