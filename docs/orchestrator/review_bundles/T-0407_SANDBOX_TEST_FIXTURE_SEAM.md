# T-0407 Sandbox Test-Fixture Seam Review

## Classification

`SANDBOX_TEST_FIXTURE_SEAM_DEFECT`

The T-0406 fixture failures are repaired and their three regressions pass in
this provider sandbox.  The required full workspace command later fails on an
unrelated integration test that requires an inherited MCP token absent from this
provider sandbox.  Because the contract requires zero full-profile failures,
this review is fail closed and does not approve completion.

## Scoped repair and production boundary

`wake/src/store.rs::Store::open` is unchanged: it validates from the volume
root, retains its absolute/no-parent/non-link/non-reparse checks, and remains
the production constructor.  `wake/src/runtime.rs::default_root` is unchanged
and continues to derive only `%LOCALAPPDATA%\\CatDeskWake`.  Production callers
continue to use `Store::open(default_root())`.

The new `Store::open_scoped_for_test` is compiled only under the non-default
`test-support` feature.  It canonicalizes one already-existing trusted fixture
parent, validates the supplied child as a raw descendant of that parent, then
rebuilds the child from that validated relative suffix beneath the canonical
parent.  The existing descendant-only directory creation, non-link/non-reparse
checks, absolute-path requirement, and final canonical containment check remain
in force.  No production root selection, Store constructor, target, journal,
or runtime authority is widened.

The concrete repair corrects a Windows path-form mismatch: the former helper
compared a raw child path to a canonical (verbatim) parent, causing
`CONFIG_ROOT_OUTSIDE_TEST_PARENT`.  It now uses `trusted.join(relative)` before
the existing protected child traversal.  This is test-support-only plumbing.

The completed-review regression injects only its fixture cutoff closure; the
normal path remains `current_generation_accept_after`.  The stable-Wake staging
regression uses a unique fixture parent and opens the nested `store` child with
the scoped test helper.

## Verification evidence

| Profile | Result |
| --- | --- |
| V5 policy assertion regression | passed |
| Completed-review independent-Wake regression | passed |
| Stable-Wake target-staging regression | passed |
| Unit test binary in full profile | 952 passed, 0 failed, 21 ignored |
| `cargo test --workspace --all-targets --all-features` | failed later: `tests/local_mcp_wake_control.rs::bounded_local_control_probe` requires absent inherited MCP token (`NotPresent`) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed |

The missing MCP token is outside the fixture seam and was not accessed,
created, or substituted.  No credential, live Wake/tunnel/release/LKG/protected
build state, daemon, Git history, or external project was touched.

## Attribution and next step

The workspace remains dirty from inherited work.  The intended T-0407 changes
are the feature-gated fixture constructor normalization and this review bundle.
CatDesk should re-run the full profile in its approved environment with its
normal inherited test token; no source relaxation is justified by this
provider-sandbox credential absence.
