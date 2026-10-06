# T-0195 — secure Rustup toolchain policy compatibility

## Before state and authority invariant

`trusted_toolchain()` previously admitted only the exact machine-wide pair
`C:\Program Files\Rust\bin\cargo.exe` and `rustc.exe`.  Those paths feed the
immutable attempt's `cargo` and `rustc` evidence (canonical path, SHA-256,
length, Windows volume/file-index identity, and version digest), are reopened
by `validate_attempt`, pinned by `open_attested_tool` before the worker
launch, and are revalidated after the build.  `fixed_policy()` also binds the
worker argv/environment digests.  No bare Cargo/Rustc/Rustup, `where`, shell,
PATH, workspace executable, or caller executable is an authority input.

## Implemented selection model

The policy version and environment-policy digest now carry the Rustup resolver
generation.  Tier 1 remains the exact Program Files pair and wins only when
both entries exist and fully pass the existing trusted-file validator.  One
missing entry, an unreadable entry, reparse, special object, or any other
ambiguous fixed-slot state fails closed; it cannot trigger a mixed-tier result.

Only when both exact Tier-1 entries are `NotFound` does Tier 2 call
`SHGetKnownFolderPath(FOLDERID_Profile)` for the current process token.  It
constructs only `<profile>\.cargo\bin\rustup.exe` and `<profile>\.rustup`,
checks the profile/home chains, attests the resolver as a regular non-reparse
file, and runs its exact absolute path with `env_clear`, fixed
`CARGO_HOME`/`RUSTUP_HOME`, null stdin/stderr, bounded stdout, and a ten-second
timeout.  Rustup output is discovery only: `rustup default` must produce one
bounded UTF-8 `name (default)` line; the final cargo/rustc paths are then
constructed beneath the exact selected `.rustup\toolchains\<name>\bin` root.
Both final files must pass the pre-existing exact handle/hash/identity/version
validator and are the only executable paths persisted in an attempt.

## Current host result and limitation

The production OS-token resolver was exercised while developing the bounded
host probe.  The current sandbox token resolves a different current-user
profile than the profile which owns this workspace's Rustup installation.  The
token profile has no profile-local Rustup resolver, so the production selector
correctly returns `REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE`.  Selecting the other
profile's Rustup via PATH, environment variables, workspace data, or a hard
coded user path would violate this ticket's authority rules and was not added.

Accordingly the host-compatibility probe is explicitly ignored with that
non-secret reason, and T-0195 is **not complete in this sandbox**.  T-0194
remains ignored and no replay-validator closure is claimed.

## Files and verification

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0195_T0154_R7D_R9_R4B_SECURE_RUSTUP_TOOLCHAIN_POLICY_COMPAT_REVIEW_BUNDLE.md`

The resolver preserves the closed static test forbidding bare tool, `where`,
and shell lookup. Focused `cargo test reviewed_build -- --nocapture` completed
with 20 passed and 2 explicitly ignored tests (the existing T-0194 validator
and this sandbox-token host probe); strict all-target/all-feature clippy also
passed. Full host compatibility and full rust_full cannot be claimed while the
OS-token/profile authority mismatch remains unresolved.
