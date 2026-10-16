# T-0478 D6 — Reviewed-candidate owner service health watch

Date: 2026-10-16. Status: `IMPLEMENTED_SOURCE_ONLY` and focused local tests. Base: `f098b34a6f263337149d2fb9b9b4b03363a1a7a3`.

The parent-owned candidate already checked that its child process and loopback listener remained present. A child that kept the socket bound while its isolated MCP route stopped answering could remain reported as ready indefinitely. The owner now verifies the exact Windows listener PID on both sides of an authenticated `tools/list` probe. The response must still expose exactly `read` and `search`, and the unauthenticated request must be rejected. This uses the existing isolated route and bearer token; no new endpoint or authority is added.

The existing cheap process/listener check remains at one-second cadence. The MCP service probe runs every 15 seconds and has an independent six-second deadline, so a continuously wedged route is normally detected within about 21 seconds plus scheduling delay. This avoids sending two extra MCP requests every second while preserving a bounded failure observation. A failed check exits the owner with an error. Its existing retained kill-on-close Job then attempts to stop and reap the exact child process tree. The owner does **not** write `runtime-attention.json`, adopt or restart a child, dispose of a journal/root, or declare a new READY. A prior READY line is only a point-in-time result; after failure the owner exits nonzero and the remaining journal needs separate read-only diagnosis. A successful CI-witnessed launch and durable orphan classification were not exercised here.

## Exact local evidence

- `cargo test --locked --offline --bin catdesk reviewed_candidate_ -- --test-threads=1`: 14 passed, zero failed, including a bound-but-stalled listener, authenticated healthy route, exact socket PID, owner crash Job closure, and descendant reaping.
- `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings`: pass.
- `cargo fmt --all -- --check` and `git diff --check`: pass.
- A default-sandbox full Rust suite was attempted but stopped after unrelated project-registry fixtures failed. Exact examples: `binagotchy_cli::tests::projects_view_redacts_targets_threads_and_git_identity` returned `PROJECT_REGISTRY_UNAVAILABLE`; `delegated::autonomy_projects::tests::control_root_resolves_three_git_projects_without_external_registry_writes` returned `Access is denied. (os error 5)` on registry I/O. This is **not** a full-suite pass. An unsandboxed exact integrated full suite remains the next gate.

No installed WakeHost, production supervisor, external tunnel, protected release state, GitHub workflow setting, or real candidate listener was changed. The exact source commit still needs independent review, integration, exact-source regression, and remote CI evidence.
