# T-0324 lineage-repair serving-controller candidate

## Classification

`SERVING_CONTROLLER_CANDIDATE_READY` for separate independent review only.

This is a serving-parity candidate, not an activation, reviewed-build
designation, ordinary-worker release, wake, tunnel, or host-authority record.
CatDesk must still perform its independent verification and diff capture.

## Accepted source authority

The source basis is the independently accepted
`REVIEWED_BUILD_NEW_AUTHORITY_LINEAGE_REPAIRED` implementation documented in
`docs/orchestrator/review_bundles/T-0324_REVIEWED_BUILD_NEW_AUTHORITY_LINEAGE_REPAIR_REVIEW_BUNDLE.md`.

Current `src/reviewed_build.rs` retains the bounded active-lineage transition:
it revalidates an exact terminal failed family, preserves its immutable audit
history before publishing a fresh active lineage, converges exact replay, and
rejects competing authority bindings. This candidate session made no product
source or test change.

## Fixed isolated candidate

The fixed `CARGO_BUILD_RELEASE_ISOLATED` profile completed without a Windows
output lock:

```text
cargo build --release --locked --target-dir .catdesk/verification-targets/autonomy-release
```

Bounded readback of the fixed output after that successful profile:

```text
path: .catdesk/verification-targets/autonomy-release/release/catdesk.exe
SHA-256: 054617d87a3994fb1367a7a45b551300f1b1ed400762363d0b7f5bc01c3cbe7f
byte length: 26408960
last-write UTC: 2026-09-13T04:02:39.7769071Z
```

The output timestamp is later than the accepted repair source measurement
(`src/reviewed_build.rs`, `2026-09-13T03:02:07.1768636Z`), establishing this
as a post-repair isolated candidate rather than the prior stale/locked output.
The candidate was not executed or reloaded.

## Local verification

```text
cargo fmt --all -- --check                                      PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings  PASS
cargo test --workspace --all-targets --all-features             PASS
CARGO_BUILD_RELEASE_ISOLATED                                    PASS
git diff --check                                                 PASS
```

The full test profile completed the 939-test main unit binary with 918 passed
and 21 explicitly ignored tests, plus all configured target suites. `git diff
--check` produced only inherited CRLF working-copy warnings and no whitespace
error.

## Attribution and prohibited-action audit

The only intended source-tree mutation attributable to this session is this
review bundle. The workspace has extensive inherited dirty and untracked
material, including the accepted lineage repair; none was reset, staged,
committed, or modified by this candidate-materialization session. The fixed
target directory contains verification artifacts only.

No daemon reload, candidate execution, live PREPARE/CONFIRM/RESULT, release or
protected-state mutation, wake/target change, Secure MCP/tunnel action,
Scheduler/service action, signing/UAC, Git publication, or external-project
mutation occurred.

## Next boundary

The only next action is separate independent review of this exact measured
candidate and attributable diff. Nothing in this bundle authorizes reload,
promotion, activation, target migration, or any host mutation.
