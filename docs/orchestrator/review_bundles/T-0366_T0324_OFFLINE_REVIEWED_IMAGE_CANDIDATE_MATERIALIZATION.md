# T-0366 — T-0324 offline reviewed-image candidate materialization

## Authority decision

T-0214's final-link gate remains mandatory for the ordinary reviewed-build producer path. T-0215 supplies a separate accepted product-root signed, exact-opened-object main-image authority specifically for the cross-generation/first-executable case; it does not derive authority from producer attestation, a workspace path, a post-hoc hash, or `target/release`. T-0365 independently designates the exact source domain below for candidate materialization only. This is not signing, bootstrap, rotation, promotion, or deployment authority.

## Frozen source and locked build evidence

The isolated source root is `.catdesk/candidates/t0366-source`; it contains exactly `Cargo.toml`, `Cargo.lock`, and recursive `src/**`. `build.rs` was absent, so none was included. Every copied item was a regular non-reparse file. Sorted `SHA-256  relative-path` records are in `.catdesk/candidates/t0366-source-manifest.txt`.

- file count: 90
- canonical source-manifest SHA-256: `8ac4015fb238c09cc1444f7867b280a6ba90f7529fa75feb1e1af75907e2b562`
- freeze-time workspace-to-copy comparison: exact match for all 90 files
- build: `cargo build --release --locked --target-dir ..\\t0366-build`, run from the frozen root only
- post-build workspace-to-copy comparison and manifest recomputation: exact match
- output: `.catdesk/candidates/t0366-build/release/catdesk.exe`
- output SHA-256: `d09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f`
- output length: `26302464`

The output is an offline candidate measurement only. It is not `target/release`, a reviewed image, an LKG, a promotion input, a bootstrap input, or a runtime selection source.

## Epoch, purpose, and payload decision

No canonical unsigned signing payload was created. T-0215 proves a signed historical epoch-1 bootstrap envelope; T-0217 records only an unsigned historical epoch-2 rotation request. Fixed Program Files accepted/rotation receipt state is `UNOBSERVABLE_FROM_CURRENT_SAFE_SURFACE`, so this ticket cannot prove the next monotonic epoch or whether bootstrap versus rotation is currently valid. Choosing epoch 2/3 or a purpose from workspace history would risk replay or policy misuse and is refused.

## Independent designation request

ChatGPT must independently review the T-0365 designated source scope, this exact frozen manifest and candidate measurement, and a safe current protected receipt/epoch determination before designating any purpose, epoch, or signing payload. Only the externally owned product-root signer may create a detached signature; only separately authorized administrator fixed-path consumption may attempt bootstrap/rotation afterward.

No private key/signature, host staging, bootstrap, rotation, promotion, reload, recovery, browser/wake, tunnel, Scheduler/service, protected-state, external-project, Git, clean, or reset action occurred. `git status` and `git diff --check` are the required attribution checks.
