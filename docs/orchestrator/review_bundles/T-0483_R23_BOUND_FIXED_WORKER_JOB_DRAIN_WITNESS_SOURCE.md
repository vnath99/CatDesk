# T-0483 R23 — Bound fixed-worker Job drain witness (source only)

## Change

`src/delegated/sol_worker_lifecycle.rs` now has separate suspended and running private owner states. The same-user `Command` route resumes only after exact Job membership is rechecked, and its bounded drain still returns only `Result<()>`. A separate private transition accepts the opaque `BoundFixedLocalSolWorkerProcessV1` while the original child is suspended. It compares the retained R20 process handle with the Job owner's opened child handle using Windows `CompareObjectHandles`, revalidates the restricted token and fixed local account, and resumes only after that check. Failure terminates the exact suspended child with a bounded wait and returns no bound state.

Consuming a bound owner through the existing bounded original-process and Job drain returns `DrainedBoundFixedWorkerJobV1`. Its fields are private and it is not cloneable. The R20 token observation happens before resume because a normally completed worker can be exited when drain begins. The drained value retains the original process binding but makes no post-exit live-token claim.

The `CompareObjectHandles` import uses `mincore.lib`, which contains the symbol in the installed Windows SDK. This keeps kernel-object comparison instead of relying on a PID. No fixed-account launcher, host-held run/output-root binding or downstream consumer was added.

## Verification

- `cargo test --locked --offline sol_worker_lifecycle -- --nocapture`: 7 passed; one spawned-helper test ignored in the normal runner.
- `cargo test --locked --offline sol_worker_ -- --nocapture`: 54 passed; one helper ignored.
- `cargo clippy --locked --offline --all-targets --all-features --workspace -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `git diff --check` on the scoped P1 files: passed.

The tests check that different opened child process objects are refused, the same object compares exactly, a same-user diagnostic child cannot satisfy fixed-worker binding, invalid deadlines produce no drain observation, original and direct descendant Job members drain, and a retained duplicate process handle does not make an exited Job appear active. No positive fixed-account test could run: `CatDeskSolWorker` is absent on this host.

## Boundary

This witness only establishes that the bound original process and members of its retained Job drained. It does not establish the intended Codex image, exact task/nonce/output root, all possible descendants or loss of write authority. [Microsoft's Job Objects documentation](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) says a child launched through `Win32_Process.Create` is outside the Job; [handle inheritance](https://learn.microsoft.com/en-us/windows/win32/sysinfo/handle-inheritance) and duplication can carry writable handles into another process. The staging API still requires a live R20 process during readback, so adding this witness as an extra argument there would fail after a real drain. A later host handoff must capture the trusted run and pinned output root before resume, consume a proven stopped state at nonce burn, replace live-stage revalidation with an appropriate frozen identity check, and separately establish outside-Job and ancestor/inherited-handle exclusion before repository use.

No nonce, stage, repository, runtime, review or Wake path accepts this witness. The Sol/medium end-to-end request remains open.
