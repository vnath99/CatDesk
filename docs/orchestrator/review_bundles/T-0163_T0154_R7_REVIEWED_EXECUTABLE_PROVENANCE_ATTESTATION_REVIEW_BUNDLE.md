# T-0163 / T-0154-R7 — Reviewed Executable Provenance Attestation

## Reproduced gap

R6 review authority identified an acknowledged completed task, but promotion
preflight still accepted any safe workspace executable selected by caller path
and SHA. Path, SHA, PID, transport, and review-bundle presence do not prove it
was built from reviewed inputs.

## Fail-closed attestation boundary

Promotion preflight now requires the protected, bounded
`.catdesk/promotion-control/reviewed-build-attestation.json` before it will
create promotion authority. Its exact schema binds the acknowledged review
session/record/R6 authority digest, source-input digest, toolchain digest,
candidate relative identity, and candidate SHA. All digests are exact SHA-256.
The attestation itself is fingerprinted into protected preflight authority and
is rehashed at confirmation. Missing, malformed, changed, or mismatched
attestation rejects before authorization persistence or worker spawn.

This deliberately does not bless historical reviews by hashing mutable current
source after acknowledgement. Historical reviews lacking an immutable
CatDesk-owned release attestation require a fresh reviewed source/release
assembly step; dirty unrelated workspace files are neither a gate nor source
authority. A future closed host-owned assembly surface must produce this record
only after complete Rust build-input coverage and fixed trusted toolchain build.

## Preserved boundaries

R6 authority revalidation, R6A transaction/claim ownership, R6B truthful
replay, direct-script refusal, LKG ordering, redaction, and external tunnel
non-ownership remain unchanged. Source-to-candidate provenance is no longer
inferred from caller fields, but the host-owned attestation producer remains a
fresh-review requirement rather than a mutable-workspace fallback.

Changed file: `src/daemon_reload.rs`; no live build, promotion, reload,
recovery, tunnel, browser, Scheduler, external-project, or Git publication
action occurred. `cargo fmt` and `cargo check` passed.
