# T-0401R2 Reviewed-Build Environment Diagnostic

## Classification

`REVIEWED_BUILD_ENV_DIAGNOSTIC_COMPLETE`

This is an evidence-only diagnosis. No product source, reviewed-build control
state, Wake/runtime/release state, or external project was changed.

## Authoritative generation and frozen source

- Active V4 generation: `2a3b873818c547538f5c4109dcdbb9fd`.
- Result: `BUILD_FAILED_OR_AMBIGUOUS`, `REVIEWED_BUILD_FAILED`, phase
  `CARGO_BUILD`, exit code `101`; the persisted bounded diagnostic is
  `CARGO_EXIT_NONZERO` with a 4096-byte truncated capture.
- The attempt binds review authority
  `7143d8691c555fb493266680b27fa5b77a316516a29aea8ef794bcc21f2d8710`
  and frozen T-0399 snapshot
  `092a05c1b798bf05e6b79da060ab041b3c5e4e5852e87f44ea51dbc19707f744`.
  Its authority and manifest digests are respectively
  `8f66647a4bfc4ea32d8ba281ddeaa51ee582f1c9fe878d4a26bec57bbe919ad2`
  and `75ce4bb7acc0f7c83272ea7786486aa74f2dbb7ea60a402e97178a3432a4da60`.
  Those values exactly match the T-0399 frozen manifest.
- The attempt uses the attested Cargo and Rustc paths/evidence persisted in
  that attempt and V4 argv `build --release --locked`.

## Worker environment readback

The reviewed worker clears the inherited environment, then supplies only:
`CARGO_TARGET_DIR`, `RUSTC`, `CARGO_HOME`, `PATH` (the attested Cargo parent),
`SystemDrive`, `TEMP`, and `TMP`. `CARGO_HOME` is the protected
generation-local `cargo-home` directory. No inherited profile Cargo home,
registry cache, or network configuration is retained.

Both the failed generation's `cargo-home` and the disposable reproduction's
Cargo home had exactly three entries: `registry/`, a zero-byte
`.package-cache`, and `registry/CACHEDIR.TAG` (177 bytes). Neither contains a
registry index or crate cache.

## Disposable reproduction

The reproduction used only frozen
`.catdesk/reviewed-source-snapshots/adc-t0399-reviewed-build-worker-fix-authority-20260922/bytes`
as its working source. Its target, temporary directory, Cargo home, and
captured stdout/stderr were all under the disposable
`target-verify/t0401r2-v4-scrubbed-6b8e1de40d134798bf20f89e28ff9d14`
directory. It ran the attested Cargo binary with the fixed V4 argv and the
same scrubbed environment construction.

Result: exit `101`; stdout `0` bytes; captured stderr `20980` bytes; SHA-256
`d91b2386a81ca7fc10efc827545739bdf3aed8a0c8c37d791b2d2ce7ce0c6d54`.
The capture is UTF-16 PowerShell error-stream formatting around Cargo's stderr;
it is diagnostic-only and is not compared to the worker's raw bounded-capture
digest. Its bounded causal content is:

```text
error: failed to get `axum` as a dependency of package `catdesk v0.1.6`
failed to load source for dependency `axum`
unable to update registry `crates-io`
download of config.json failed
[6] Could not resolve hostname: index.crates.io
```

The frozen source declares `axum = "0.8"`. No differential environment probe
was needed: the exact isolated-Cargo-home precondition and the DNS failure are
both directly established.

## Root cause and minimal safe repair recommendation

The protected worker's empty generation-local `CARGO_HOME` leaves Cargo unable
to resolve the locked dependency graph without reaching crates.io. In the
observed environment DNS cannot resolve `index.crates.io`, so Cargo fails
before compiling CatDesk or producing an output candidate. This is a dependency
availability/isolated-Cargo-home failure, not a snapshot-materialization,
Rustc, output-path, or attestation failure.

Do not restore the mutable inherited user Cargo home. The smallest safe repair
direction is to provide a fixed, protected, integrity-validated registry/index
and crate cache for the exact locked dependency graph inside the reviewed
worker's isolated Cargo-home authority, and make the reviewed invocation use
that fixed offline dependency material. The cache identity must be bound to
the reviewed lockfile/source policy; a caller-selected home, ambient cache, or
network fallback would weaken the existing trust boundary.

## Verification and boundary audit

`git diff --check` is recorded after this bundle. No live
PREPARE/CONFIRM/RESULT, daemon/reload/release action, target/Wake action,
Secure MCP/tunnel operation, Git publication, credential access, or external
project mutation occurred.
