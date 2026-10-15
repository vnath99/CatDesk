# T-0478 D2 — Candidate child cleanup and supervisor port diagnostic

Date: 2026-10-15. Status: source-only recovery increment.

The reviewed-candidate parent now explicitly closes its private kill-on-close Windows Job on every owner exit, including early errors. It checks the close result, attempts whole-Job termination and a second close if needed, falls back to its retained exact child process handle, and bounds child reaping. A Windows regression starts a descendant and proves that dropping the parent-owned child stops both exact processes. It emits a fixed diagnostic if an implicit cleanup fails.

The stable supervisor's read-only status now performs one bounded loopback connect to its fixed port 3201. The result distinguishes an unavailable/refused port from an occupied but **unattested** listener. A connection cannot prove the process image, protected state, Task Scheduler definition, backend health, or activation authority. An ephemeral-port test proves listener detection and that a closed socket is never reported as a live listener.

Focused Windows owner tests and the port test pass locally. The final root all-features Rust suite exited 0 (main 1,136 passed/26 ignored plus integrations); strict root Clippy and formatting passed. Exact-commit GitHub Windows CI remains unavailable. No reviewed-candidate live launch, production supervisor start, Task Scheduler change, tunnel change, or protected install occurred. Durable orphan reconciliation, owner-attested supervisor health, and a CI-witnessed successful candidate lifecycle remain open.
