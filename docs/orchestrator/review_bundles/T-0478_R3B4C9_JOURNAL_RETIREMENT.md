# T0478 R3B4-C9 — Clean owner-journal retirement

Date: 2026-10-14. Scope: source-only reviewed-candidate disposable fixture lifecycle.

## Change

After the consuming owner-bound retirement reports that its exact opened root was removed, the owner journal receives a create-only `retired` outcome. The new cleanup rereads canonical intent/root/serving/start/outcome records, requires the same serving generation and absent generated root, and checks the protected owner-only descriptor and exact five-file flat inventory. It binds each file's length, SHA-256 and Windows object identity, opens all five exact DELETE handles before disposition, and disposes only those handles. It then requires an empty journal, reopens the same direct-child journal for DELETE, rechecks owner/DACL and emptiness, and disposes that opened directory object. A final pinned-parent check requires absence.

An unexpected journal child, changed record, occupied root name, owner/DACL drift, or uncertain retirement outcome leaves the journal for operator attention. `attention` outcomes never enter clean cleanup. A failure during file disposition can leave partial journal residue; no automatic name-scan recovery exists.

## Evidence

- Windows focused tests: clean root plus journal absent; an injected unexpected journal file survives and returns attention; same-name root replacement survives with a durable `attention` outcome. All three pass.
- `cargo clippy --all-targets --all-features -- -D warnings`: pass.
- `cargo test --all-features --quiet`: pass; main binary 1,087 passed, 26 ignored, followed by passing binary and integration suites. The AppContainer child helper still emits access-denied diagnostics while its parent assertions pass.

## Acceptance boundary

This is a local source increment, not an installed worker or accepted T0478 lifecycle. Cross-process exclusion, bounded owner PID/port liveness, power-loss recovery, successful live witnessed launch, and exact-commit three-job Windows CI remain outstanding. GitHub Actions dispatch currently returns HTTP 422 disabled. P0 immutable WakeHost promotion and P1 real Codex/review/Wake cycle remain open. No signed runtime, installed WakeHost, official tunnel, auth/browser state or hourly deadman was changed.
