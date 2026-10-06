# T-0166 / T-0154-R7A — Producer Gap Assessment

## Reproduced R7 gap

The R7 promotion reader accepts `reviewed-build-attestation.json` based on its
syntactic fields and matching candidate/review values.  It has no protected
CatDesk-owned producer, immutable reviewed source snapshot, fixed Cargo/rustc
resolver, isolated-build worker, or durable build-attempt owner.  Therefore a
JSON record could name source/toolchain digests without proving how they were
created.

## Fail-closed finding

It would be unsafe to add a caller-facing digest, `reviewed=true` flag, or a
Rust reader-only schema field: each would retain the same forgery path.  The
required R7A success path needs a new protected reviewed-source snapshot and
host-owned fixed toolchain build worker, neither of which exists in the current
control plane.  Historical reviews must remain rejected with
`REVIEWED_SOURCE_SNAPSHOT_REQUIRED`; no mutable dirty-workspace source hashing
or review-bundle inference is authorized.

No live build, promotion, reload, recovery, tunnel, browser, Scheduler,
external-project, or Git-publication action occurred.
