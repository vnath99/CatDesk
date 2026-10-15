# T-0483 R28 — Create-once Sol launch reservation (source only)

## Change

The private prepared Sol path now consumes a non-cloneable reservation before native process creation. The host binds the selected task/turn authority and active nonce to the pinned ledger, output and executable-parent directory identities, the reviewed executable object and SHA-256, and the exact application, command line, environment and working-directory inputs. It creates one `launch-started-<nonce>.json` record under the pinned ledger, syncs the file, flushes the pinned parent directory, and reads the record back before permitting the creation-time Job launch. The native launcher rechecks the binding immediately before `CreateProcessW`.

The record is never removed by an error path. A second contender loses the create-once race. On restart, a selected turn reopens as `ATTENTION` whether the record is intact, missing, torn or corrupt; there is no relaunch or repository-use recovery path. Failed native creation can report a clean process failure only after an observed empty Job, but the turn's reservation remains spent.

## Verification

- Six reservation tests pass, including one-winner concurrency, interrupted create/parent-flush, corrupt record, restart attention and a real Windows pinned-directory reopen/flush. The lifecycle module has 29 passing tests and one intentionally ignored helper; changed launch inputs stop before child creation.
- The isolated shell/cwd test and the remaining all-features workspace Rust suite pass locally. The latter includes nested Wake-local tests. Strict all-target/all-features Clippy, formatting, all-bin build and staged diff checks pass.
- Independent source review found no concrete bypass or blocker in this increment. Source commit `4e4e1ecfb560d1d13f73990984d8041aacb21688` was pushed; immediate read-only GitHub readback showed zero workflow runs and zero check-runs for that SHA. This is not remote CI acceptance.

## Remaining gate

Parent-directory flushing was exercised on this Windows host; sudden hardware power loss is not proven, and the older nonce selection/issue entries do not yet have equivalent parent-entry durability proof. The fixed `CatDeskSolWorker` account is absent. The current native diagnostic launch uses the host user, and the drained Job witness is not connected to post-exit staging or repository use. A distinct restricted primary-token launch, escaped-descendant/inherited-handle and ancestor write-authority exclusion, stopped-worker handoff, crash-safe burn-to-stage transaction, real Sol/medium deliverable, natural `review_ready` event and exact Wake receipt remain open.
