# T-0485 R19 — read-only legacy owner candidate inventory (source only)

## Scope

`wake/src/install_broker_legacy_owner.rs` adds an argument-free read-only inventory of the fixed `CatDeskWakeHost.exe` process name and two fixed, current-token known-folder candidate roots: direct LocalAppData and Codex LocalCache. Toolhelp process candidates retain opened handles and recheck PID-lifetime evidence: process creation, liveness and a pathname-text fingerprint. Candidate roots and `host.lock` leaves are opened read-only with final-leaf no-follow and retained handles; volume/file IDs are compared on reopen. No `current.json` pointer is read.

The result is one of `NO_MATCHING_NAMED_PROCESS_IN_SNAPSHOT`, `AMBIGUOUS` or `UNPROVEN_LEGACY_OWNER`. Every result is forensic only. A matching name, path, hash, file ID or opened lock does not prove byte-range lock ownership or identify the connected store. No caller can supply a PID/root, and no API converts this inventory into `BrokerObservedStateV1`, an install lease, cleanup or activation.

## Evidence and limits

- Four focused Windows tests pass, including a valid-looking pointer with no process, duplicate/split candidates, identity drift/open-lock insufficiency and portable diagnostic inventory. Independent read-only review found no concrete FFI/handle or fail-open defect. Combined gates passed: root all-features Rust workspace (main 1,212 passed/27 ignored plus integrations), independent Wake all-features Rust workspace (170 library tests plus bins/integrations), full Python `pytest.exe -q --tb=short`, strict root/Wake all-target/all-feature Clippy, both format checks, root all-bin build and diff check.
- The process disposition means absence of the **named process in one Toolhelp snapshot** only. A renamed process, snapshot race or process outside access scope may exist. The image fingerprint hashes normalized path text, not executable bytes or the mapped image section. Final-leaf no-follow does not pin or validate every known-folder ancestor, and a redirected ancestor may lead to another file object. Current-token roots do not establish the old connected runtime root. These limitations remain non-authorizing.
- The installed dev.84 binary predates the current `HostRootPin` and nonce owner challenge. No WakeHost process was observed in the read-only local inventory at this checkpoint. The matching unsigned direct per-user dev.84 pointer/host hash cannot resolve the previously observed connected/direct `host.lock` file-ID split. Signed broker/bootstrap, a separately trusted predecessor anchor, exact CI and reviewed migration are still required before dev.85 installation or a new Chat53 canary.
- Published source commit `89ef5558655e77d3e0c0302cf4c5e0a33862a0ac` has zero GitHub workflow/check runs on immediate and bounded post-build readback. Its clean BuildOnly candidate is unsigned diagnostic `1.0.0-dev.85-daef712899bc-a75541a78920`; it is neither an approved package nor an owner proof.

## Next gate

Establish an independently trusted old-owner/store baseline and a reviewed migration path. If a live old host appears, require actual lock ownership and process/root identity evidence; this diagnostic cannot manufacture dev.84 cooperative proof.
