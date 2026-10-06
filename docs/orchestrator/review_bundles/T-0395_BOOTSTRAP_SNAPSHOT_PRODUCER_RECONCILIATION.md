# T-0395 — Bootstrap Snapshot Producer Reconciliation

Date: 2026-09-21

## Scope

This bounded review reconciles CatDesk's reviewed-release bootstrap without treating a temporary daemon reload, runtime health, or an isolated verification build as promotion/LKG authority.

## Observed bootstrap state

- Canonical `target/release/catdesk.exe` SHA-256: `51479d67e4b263ee73080c9227a2bad532cb96072e50034b2d310dc76a6e085e`.
- Canonical fingerprint file still names `a264c8dc85fa55311e74114158f4f4cd598f6e9e9d0a6ca535e14d18aa649939`; therefore the canonical pair is mismatched.
- The only durable release-recovery slot is generation 1 with source `operational_verified`, which current policy correctly refuses as reviewed rollback authority.
- The former serving daemon produced immutable T-0393 reviewed-source bytes that omitted `wake/Cargo.toml` and `wake/src/*` even though the materialized root `Cargo.toml` requires `catdesk-wake = { path = "wake" }`. Protected reviewed build therefore failed during `cargo build --release --locked`.
- Current source `collect_release_inputs` explicitly requires `wake/Cargo.toml`, recursively collects the Wake source tree, and has regression assertions for `wake/src/lib.rs` and `wake/src/bin/CatDeskWakeHost.rs`.
- No durable reviewed-build generation was found in `BUILD_ATTESTED` state.

## Temporary bootstrap boundary

The daemon was temporarily reloaded from the existing isolated verification image
`.catdesk/verification-targets/autonomy-release/release/catdesk.exe`,
measured SHA-256
`8b62a56925ca598ed3a23ba45aac9503fcf1e682c6514b0432d53ed33e687dc1`.

The reload was exact-hash bound through the existing daemon-reload preflight/confirmation transaction. It left the external Secure MCP tunnel untouched. This temporary image is **not** reviewed promotion authority, does not mint LKG, and must be replaced by a normally attested and promoted reviewed-build candidate.

After reload the local MCP self-check increased from 88 to 89 tools and remained `CONNECTED_VERIFIED / READY`, confirming that a newer daemon generation is serving.

## Reviewed-build API compatibility correction

Current source had an MCP contract mismatch: the public schema advertises `PREFLIGHT` for `catdesk_reviewed_build`, while the internal handler previously accepted only legacy `PREPARE`. The bounded correction:

- accepts `PREPARE | PREFLIGHT` internally for backward compatibility;
- makes the operator facade emit `PREFLIGHT`;
- does not widen paths, hashes, review authority, confirmation tokens, promotion authority, shell authority, or tunnel authority.

The older compatibility bridge `REVIEWED_BUILD_PREPARE/CONFIRM/RESULT` remains available and was used to prove the protected terminal-generation retry mechanism before this review.

## Required verification

This ticket requires the fixed, closed-world verification profiles:

- `CARGO_FMT`
- `CARGO_TEST`
- `CARGO_BUILD_RELEASE_ISOLATED`
- `GIT_DIFF`

The isolated release output is verification evidence only. It must not be interpreted as reviewed image or promotion authority.

## Acceptance

T-0395 is acceptable only if CatDesk completes normal verification and the fresh immutable reviewed-source snapshot created by the newer serving producer contains at minimum:

- `Cargo.toml`
- `Cargo.lock`
- `wake/Cargo.toml`
- `wake/src/lib.rs`
- `wake/src/bin/CatDeskWakeHost.rs`

Only after that snapshot proof may its fresh independent final review be used to prepare a protected reviewed build. Only a resulting `BUILD_ATTESTED` candidate may enter reviewed-promotion preflight. Successful reviewed promotion must establish the canonical binary/fingerprint pair and durable `reviewed_promotion` LKG authority before one-command recovery is considered repaired.

## Non-goals

No Wake target/profile/event is changed here. The generation-13 T-0394 Wake attempt remains separate and unaccepted. No Git publication, external project mutation, arbitrary shell authority, or trust-policy relaxation is authorized by this ticket.
