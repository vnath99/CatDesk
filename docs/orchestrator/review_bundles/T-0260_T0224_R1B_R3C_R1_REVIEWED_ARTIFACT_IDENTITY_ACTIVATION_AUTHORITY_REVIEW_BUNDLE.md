# T-0260 / T-0224-R1B-R3C-R1 — reviewed-artifact identity and activation authority

## Scope and attribution

T-0259 was rejected as a false green because its activation design did not
establish reviewed-artifact identity and its required bundle was absent.  This
slice adds a source-only, fixed-path activation boundary.  It did not invoke
the activation binary, write a real selector, launch a browser, inspect a
browser profile, or modify the live `.catdesk` authority.

The workspace entered this task broadly dirty, including untracked stable-wake
sources and unrelated tracked edits.  The narrow T-0260 logical changes are:

| Path | T-0260 change |
| --- | --- |
| `src/windows_protected_fs.rs` | `ReviewedArtifactIdentity` and `bind_reviewed_regular_artifact`, built on a `PinnedDirectory` parent and a RootDirectory-relative regular-file open; `PinnedDirectory` now implements `PinnedParent`. |
| `src/stable_wake_delivery.rs` | Read-only `validate_cutover_safe` gate for schema-4/operator-attention/SUBMITTING ambiguity and canonical-inbox validity. |
| `src/stable_wake_owner_mode.rs` | Exact selector vocabulary, read-only preflight, expected-old-owner CAS, kernel-lock serialization, atomic selector write/readback, and source regressions. |
| `src/bin/catdesk-stable-wake-activate.rs` | Fixed no-argument dormant production activation tool; it is not scheduled or invoked by this task. |
| `src/bin/catdesk-stable-wake-owner.rs` | Includes the shared protected filesystem module required by the selector preflight build graph. |
| `src/mcp.rs` | Adjusted the legacy selector match to `LegacyPython`; no new activation MCP surface was added. |

## Reviewed identity primitive

The shared primitive is `windows_protected_fs::bind_reviewed_regular_artifact`.
It receives a pinned parent plus one validated component, opens the file through
`OBJECT_ATTRIBUTES.RootDirectory` with `FILE_OPEN_REPARSE_POINT`, rejects a
directory/reparse object, measures the already-open handle, checks its bounded
SHA-256 against reviewed evidence, captures volume/file-index identity, and
revalidates the parent handle before returning only `{sha256, byte_length,
object_identity}`.  It does not accept a caller-supplied full artifact pathname.

Activation uses fixed candidate components only:

- `target/release/catdesk-stable-wake-owner.exe`
- `scripts/stable_wake_browser_adapter.py`
- `.catdesk/wake-bridge/venv/Scripts/python.exe`

The digest evidence location and schema are fixed under the reviewed-build
control tree.  No CLI/MCP argument selects an executable, adapter, Python,
target, profile, inbox, state, selector, or root.

## Owner selector and preflight

`owner.json` schema 1 has exactly two accepted values: `legacy_python` and
`rust`.  Missing selector maps to `legacy_python`; malformed/unknown selector
maps to no owner.  `activate_reviewed_rust_owner(workspace, expected_old)`:

1. acquires the existing crash-recoverable delivery kernel mutex;
2. reads the canonical inbox and exact protected target without mutating them;
3. rejects malformed schema-4 state, operator attention, unresolved
   `SUBMITTING`, and canonical inbox ambiguity;
4. binds both fixed artifacts through the shared protected identity primitive;
5. checks the fixed Python regular object;
6. compares the actual protected selector with `expected_old`;
7. writes only `owner.json` with temp-file sync, Windows `MoveFileExW`
   replace/write-through, and protected readback.

An existing exact Rust selector is idempotent only after every gate is
revalidated.  A stale expected owner, malformed selector/evidence, drift,
unsafe object, or losing concurrent contender fails closed.  The transition is
legacy-only to Rust-only: there is no accepted `both` state.

## Verification evidence

| Command | Result |
| --- | --- |
| `cargo check --quiet` | PASS (only environment warning: could not canonicalize `C:\\Users\\Volap`). |
| `cargo fmt --check` | PASS. |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS. |
| `cargo test stable_wake_owner_mode` | PASS: 2 source/selector tests. |
| `cargo test windows_protected_fs` | PASS: 2 ownership/dependency tests. |
| `cargo test stable_wake_delivery` | PASS: 12 claim/receipt/history/ambiguity tests. |
| `cargo test` | Rust tests passed (746 passed, 21 ignored), then the existing fixed-project Python fixture failed because its venv points to missing `C:\\Users\\Volap\\AppData\\Local\\Programs\\Python\\Python312\\python.exe`. No repair of that host-owned venv was attempted. |
| `git diff --check` | PASS (no diff whitespace error; repository emitted pre-existing CRLF advisory warnings). |

`rust_full` has no independently runnable project command in the checked
source/configuration; it was not fabricated as a pass.  The full Cargo test
failure above blocks a green verification claim.

## Security and non-mutation proof

The new activation source contains no `Command::new`, no `wake_bridge.py`
launch, and no inbox write path.  `stable_wake_owner_mode` source tests assert
the fixed vocabulary and no browser/inbox mutation surface.  Existing delivery
tests retain the single-submit-owner, canonical-inbox, receipt, ambiguous
SUBMITTING, lock contention, and 128/129 retirement protections.

No live `owner.json`, schema-4 delivery state, canonical inbox, target/profile,
daemon/release, Scheduler/service, Secure MCP/tunnel, browser, Git, signing, or
provenance state was mutated by this provider turn.

## Mechanical gate status

- Shared protected reviewed-artifact identity: PASS in source/compile/clippy.
- Fixed-path expected-old selector API and readback: PASS in source/compile.
- Full isolated CAS/crash/concurrency fixture matrix: NOT YET COMPLETE.
- Full verification: BLOCKED by the pre-existing fixed-venv interpreter error.
- Host-live cutover/canary/restart: intentionally out of scope; residual
  T-0261 boundary after independent source acceptance and repaired verification.
