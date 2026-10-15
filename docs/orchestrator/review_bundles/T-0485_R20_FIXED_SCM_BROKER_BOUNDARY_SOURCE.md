# T-0485 R20 — Fixed SCM broker and bounded one-shot transport (source only)

## Change

`CatDeskWakeInstallBroker` is a separate Windows service entry point with one compiled service name and an exact `--service` command line. It has no console mutation mode. Its SCM `ServiceMain` reports `RUNNING` before querying SCM state, then requires the fixed protected root and policy, a retained global ledger lock, and the broker's own running service/process and clock readback before it creates the fixed first-instance pipe. It admits at most one request and terminates with `WAKE_BROKER_TRANSACTION_GATES_UNAVAILABLE` before any ledger INTENT, candidate stage, package, pointer, owner, or browser mutation. The service does not bootstrap itself.

The endpoint now uses overlapped I/O. Connect and one-message read have fixed deadlines and observe service STOP. A pending operation retains its event, `OVERLAPPED`, read buffer, and byte-count allocation through cancellation/completion; uncertain cancellation deliberately retains them until this one-shot process exits. The byte-count pointer is never a stack address. Even a test callback error or unwind first cancels and retires the pending operation. The last read is followed by the same-pipe operator-token admission before other policy I/O. The endpoint is never reused after a refusal.

## Verification

- The final independent Wake crate passed 181 library tests, its binaries and integration tests; two manual-only tests remained ignored.
- Deterministic fixtures stop the service I/O path after Windows reports a pending connect or pending read. Each returns within four seconds against a ten-second deadline and refuses a late client connect/write. Callback panic/error fixtures verify safe pending-operation retirement. Queued synchronous and delayed pending reads verify both byte-count paths. The focused seven-test group passed ten consecutive runs.
- Strict Wake all-targets/all-features Clippy, `cargo fmt --manifest-path wake/Cargo.toml --all -- --check`, and `git diff --check` passed. The new binary built. Direct no-argument and interactive `--service` invocations both refused with exit code 1.

These are local source tests. No broker service, signing identity, protected namespace, public key, or operator connection was provisioned. A console refusal is not an SCM installation readback.

From clean tracked source commit `69130987f655c61a6827eafc6432af3e76e93f29`, inert `wake/install.ps1 -BuildOnly` rebuilt WakeHost and the GUI. The unsigned diagnostic candidate is `1.0.0-dev.85-0625897729b0-807db9652f3a`: WakeHost SHA-256 `0625897729b075d2a8eb85f72b03e495dfb0669ba5b07cf24785fc879d336f86` (1,661,440 bytes), GUI SHA-256 `807db9652f3a77e2d7b83319a1e7f3505239287344afe126e43c7df112747f11` (27,823,616 bytes). The new broker binary is also unsigned: SHA-256 `48030e2be51372382257ec686cd6fe422c401aee93b85ed98a638d0348e546c3` (470,528 bytes). Adapter and bridge hashes remain `f4e01f6e2fb392ae59d2a8ac173587a373a8b5fbb4106250a6e4302ba82ed20d` and `a1816d2dec83d693300aa5d0fbb6b5c9b2fdfc0bc1b7303d66f1cbe57bdcbc9a`. BuildOnly returned before install staging. The pushed source SHA has zero workflow/check runs on bounded readback; this diagnostic is neither signed nor approved.

## Remaining gate

The service deliberately cannot enter the install transaction. It lacks an independently established current dev.84 connected owner/store anchor and exact-source CI provider; it has no reviewed signed service/bootstrap, deployed protected namespace, package/pointer/owner transaction, or protected launch-reader cutover. The existing `WAKE_INSTALL_BROKER_UNPROVISIONED` guards remain closed. Exact source Windows CI, adversarial worker and inherited-handle denial, operator-controlled bootstrap, separate offline approval, and fresh Chat53 receipt are still required before dev.85 activation.
