# T0478 R3B4-C3 — Snapshot serving owner binding

Date: 2026-10-14. Source-only change to the dedicated isolated snapshot HTTP router. No installed runtime, WakeHost, signer, tunnel, or browser mutation.

Router construction now canonicalizes the in-memory app workspace and requires it to equal the pinned snapshot root. It derives the ordinary interactive owner SID from the current OS token and verifies the exact generated direct child, stable pinned root identity, validated in-memory manifest, actual owner and fixed protected DACL. Snapshot-backed HTTP requests repeat the root/owner/DACL check before dispatch and after response construction; a failed check suppresses the candidate response. No client-provided SID, SDDL, manifest or root path is accepted as authority.

Real loopback tests use an owner-only test root. They prove that a wrong app workspace and a foreign DACL refuse router creation, an unauthorized request and global `run_command` are denied, an active listener prevents root rename, and read/search reject changed bytes or newly injected leaves. The tests remove only their own exact flat files and empty directories.

Local focused HTTP tests: 2/2 passed. Full `cargo test --all-features --quiet` exited 0 (main binary 1,082 passed, 26 ignored; remaining binary and integration suites passed). Strict workspace all-target/all-feature Clippy, formatting and diff checks passed.

This does not yet make the router a live reviewed worker. No launcher calls `stage_witnessed_candidate_snapshot` and passes that specific result to this router. A future launch transaction must bind the live GitHub witness, owner journal and snapshot to the listener, handle source/CI failure without starting HTTP, and own bounded PID/port/stop/recovery. Root owner/DACL checks bracket requests but do not prove an atomic ACL lock against a same-user security-descriptor change between checks. Independent exact-commit Windows CI remains required; Actions dispatch currently returns HTTP 422 because Actions is disabled for this repository.
