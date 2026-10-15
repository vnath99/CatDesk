# T-0485 R3 — Independent CI claim comparison source

**Scope:** `wake/src/install_broker_ci_witness.rs` and Wake library export. This pure parser is intended for a future offline approval issuer. It fetches nothing, signs nothing, accepts no package bytes, and grants no installation authority.

The caller must obtain complete GitHub responses independently over fixed HTTPS API routes, with no candidate-selected URL, redirect, or response. The comparison checks the signed approval's fixed repository, branch, workflow ID/path, source SHA, push event, latest matching run and selected attempt. The run must be completed and successful; the exact three signed job IDs must be distinct, complete, successful, bound to that run/attempt/SHA and labeled `windows-latest`. It rejects incomplete or inconsistent pagination and requires the before/after run inventories to match around the job fetch. The returned result is only a consistency fact about those supplied bytes; it is not proof of their origin.

The schema was checked against a real older successful GitHub run `38646210548` for source `4e9a2e2`; that run does **not** validate current source or authorize dev.85 installation. Seven focused adversarial parser tests passed. The final independent Wake all-features library suite passed 96 tests plus integration groups/two manual ignores; strict Wake all-target/all-feature Clippy and format checks passed. Exact new-commit CI remains outstanding.

An independent issuer still needs fixed network fetches, response provenance, a protected signing key and reviewer decision. The broker then needs its own protected trust root, replay ledger, byte/object checks, transaction and owner handoff. The current source commit has no accepted three-job Windows CI witness.
