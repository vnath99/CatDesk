# T0478 R3B4-C10 — Reviewed-candidate ready and single-owner fence

Date: 2026-10-14. Scope: source-only `--reviewed-candidate-mcp` lifecycle.

## Change

The command acquires the existing native create-once, delete-on-close Windows lock under a pinned canonical temp root before the GitHub witness or fixture staging. The name is fixed for CatDesk's one current reviewed-candidate lane, not derived from a fresh snapshot ID or commit. A second process or a preexisting name fails closed before creating a journal, root, or listener. The handle remains in scope through graceful stop and exact root/journal retirement. An abrupt process exit releases the kernel lock; no pathname stale-lock sweep is added.

After binding the loopback listener and persisting `serving.json`, the command starts Axum and proves the exact `/dev/mcp` route: unauthenticated `tools/list` must return 401, then the authenticated request must return a valid JSON-RPC response listing only `read` and `search`. The probe disables client proxies so the bearer token stays on loopback. Probe retries are bounded to five seconds and race the server task's exit. On failure the server task is aborted and boundedly awaited, startup attention is attempted, and no ready JSON is printed. Ready output now follows the successful probe.

## Verification

- Real isolated-router loopback test proves accepted authenticated catalog and rejected wrong bearer token.
- A separate test process holds the production lock while the contender fails; after the holder is hard-killed, another acquisition succeeds without removing a stale pathname.
- `cargo fmt --check`, `git diff --check`, strict offline workspace Clippy, `cargo build --locked --offline --bins`, and full `cargo test --locked --offline --all-features --quiet` pass. Main Rust suite: 1,091 passed, 26 ignored; all following binary/integration suites exited 0. Existing AppContainer child probes print access-denied diagnostics while their parent tests pass.
- `pytest tests/test_wake_bridge.py tests/test_stable_wake_browser_adapter.py -q` exited 0 (one skipped); this is a Python regression check, not a live browser acceptance.
- A real debug-binary negative canary for exact source commit `a259b0a1be3c4b97fb3a53bd4b5bcc6e762da5ae` exited 1 with `GitHub CI for expected commit is missing`, printed no ready JSON, left generated fixture roots 12→12 and journals 1→1, and left no process-lock file before or after. This proves the absent-CI fail-closed path only; no successful worker launch is claimed.

## Boundary

The lock is scoped to the canonical temp object seen by this process and the single CatDesk candidate lane. This does not prove that differently virtualized process contexts see the same temp object, nor does it authorize orphan adoption. The serving PID/port is still forensic; a parent-owned child process handle, process-instance/socket-owner verification, bounded restart, crash residue reconciliation, and a successful CI-witnessed launch remain T0478-D work. No installed WakeHost, signed image, official tunnel, authentication/browser state, or hourly deadman changed.
