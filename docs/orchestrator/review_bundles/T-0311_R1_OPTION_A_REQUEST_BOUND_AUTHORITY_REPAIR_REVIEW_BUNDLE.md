# T-0311-R1 — Option-A Request-Bound Authority Repair

## Independent findings and repair

Original T-0311 had two defects: its acknowledged completion authority could
be combined with a different locally supplied request, and its persistence API
accepted `workspace: &Path`. Both are removed.

The repaired chain is: immutable independently reviewed fixed artifact
`src/core-host-gate-approval-request-v1.json` -> canonical typed request
SHA-256 -> exact acknowledged/revalidated review authority -> typed `APPROVE`
receipt. The artifact is schema-1 `CatDesk`/`catdesk`/
`core-host-gate-approval-v1`, embeds the request and its digest, and must be an
exact current output in the immutable completion artifact set. The resolver
checks membership, safe output hash before/after read, canonical bytes, and
exact request equality. ACK-only, generic `independent_final_review`, absent
artifact, unrelated promotion/source/build authority, changed request digest,
or changed gate/observation/T-0224 tuple/target/build/generation/revision fails.

`persist_core_host_gate_approval` and all product approval storage were removed.
T-0311-R1 is pure authority/request/receipt validation; T-0312 alone may add
the fixed protected ledger/capture/finalizer integration.

## Tests and verification

Hostile regressions cover exact canonical artifact binding; ACK-only and
generic-completion rejection; absent artifact; changed gate; changed observation,
T-0224 session/record, target, build, generation, and revision; wrong domain;
noncanonical/wildcard artifact; and receipt request-digest mismatch. The
resolver test also proves current completion output remeasurement.

Passed: focused approval/resolver/preflight tests; strict clippy; full
`cargo test --workspace --all-targets --all-features` (890 tests); fmt; and
all-target build; `cargo build --release --locked --target-dir
.catdesk/verification-targets/t0311-r1` — PASS. The latter is workspace-only
release compile/link evidence, not a reviewed candidate, deployment authority,
or runtime replacement. `git diff --check` passed after final documentation.

## Boundaries

T-0311 remains unchecked until independent final review explicitly confirms
that a non-request-bound completion cannot approve and no caller-selected
production persistence path remains. No live gate is accepted; no host,
browser, wake, target, tunnel, supervisor, ProgramData, signing, or Git action
occurred. T-0312 remains blocked.
