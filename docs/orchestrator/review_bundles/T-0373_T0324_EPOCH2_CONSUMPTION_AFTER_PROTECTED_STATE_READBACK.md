# T-0373 — T-0324 epoch-2 consumption after protected-state readback

## Purpose

Consume the already-designated and product-root-signed T-0366 epoch-2 reviewed main image only after T-0372 proved that no protected epoch-2 rotation state exists in Program Files.

## Evidence consumed

T-0372 read-only host state at 2026-09-10T14:13:49Z established:

- installed `C:\Program Files\CatDesk\CatDesk.exe` = exact T-0215 epoch-1 candidate, SHA-256 `2421a90aeb9ad7775ec9927f8dfbe294faa3cbfe11ec7a071a1ed554eee28459`, length `25134592`;
- accepted bootstrap envelope exists, epoch `1`, payload bound to the same T-0215 candidate; observed SHA-256 `2684020a2bb4c4be81a207af713cdb4239a41e65a48628c07f3bbbaca42b3a81`, length `527`;
- protected pending rotation envelope = absent;
- protected installed rotation envelope = absent;
- protected rotation staging candidate = absent;
- fixed incoming candidate = exact T-0366 candidate, SHA-256 `d09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f`, length `26302464`;
- fixed incoming envelope = exact historical signed T-0217 epoch-2 transport envelope, SHA-256 `50194cf9613e8b9b541db597045fb2fc73a70511cbe90efcf1b512b19ce02644`, length `519`.

Because no protected T-0217 pending or installed receipt exists, the historical signed T-0217 object is stale ProgramData transport only. It never became accepted rotation authority. Therefore T-0366 may legitimately remain epoch 2: the reviewed rotation predecessor is the accepted T-0215 bootstrap epoch 1, and no later protected rotation predecessor exists.

## T-0373 bounded action

`scripts/t0373_apply_epoch2_after_protected_state_readback.ps1` is hard-bound to the exact T-0366 candidate and signed envelope. Before mutation it revalidates:

- exact T-0366 project candidate and envelope;
- exact accepted T-0215 bootstrap envelope hash/length;
- installed image is exact T-0215 predecessor (or T-0366 already installed);
- protected pending rotation envelope absent;
- protected installed rotation envelope absent;
- protected rotation staging absent;
- incoming candidate is exact T-0366;
- incoming envelope is either exact signed T-0217 transport or exact T-0366 envelope.

If and only if the incoming envelope is the exact signed T-0217 transport object, T-0373 renames it to a timestamped `.superseded-t0217-signed-*` evidence sibling. It then stages the exact T-0366 envelope, rechecks all protected-state absence immediately before entry, and invokes only the reviewed zero-parameter `--catdesk-reviewed-main-image-rotate-fixed-policy` command on the exact T-0366 executable.

The Rust state machine remains the only authority for signature verification, predecessor selection, epoch monotonicity, pending receipt creation/recovery, payload binding, staging, atomic replacement, installed receipt persistence, and pending cleanup.

After the command returns the accepted receipt, T-0373 requires exact installed T-0366 image/envelope readback, pending/staging absence, and unchanged T-0215 accepted bootstrap envelope before writing `.catdesk/t0373-rotation-result.json` and emitting `T0373_ROTATION_VERIFIED_SUCCESS`.

## Safety properties

- no arbitrary path/hash/epoch/signature values are operator supplied;
- no protected pending or installed rotation receipt is deleted or rewritten manually;
- no raw copy into the Program Files installed destination is used;
- no raw reload, promotion script, browser wake, wake-target edit, service/Scheduler change, tunnel replacement, Git publication, or worktree cleanup is performed;
- unknown or changed host state fails closed;
- one Windows UAC consent remains the only operator boundary.
