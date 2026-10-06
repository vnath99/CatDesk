# T-0256 / T-0224-R1B-R3A-R1 review bundle

## Outcome

**BLOCKED — do not accept as complete.** T-0255 was rejected because it only
checked symbolic links and never executed the critical Python fixture. This
slice adds the missing reparse checks and an intentionally fixed Cargo test
bridge, but the project-local venv cannot start: its launcher reports that its
configured base interpreter is unavailable. The bridge fails rather than
skipping or selecting a fallback interpreter, as required.

## Attributable files

- `scripts/stable_wake_browser_adapter.py`
- `tests/test_stable_wake_browser_adapter.py`
- `tests/stable_wake_adapter_python.rs`
- `src/stable_wake_owner.rs`
- this bundle

The working tree was already broadly dirty. These paths are the narrow T-0256
changes; no `.catdesk` file, legacy bridge script, target, profile, browser, or
delivery state was modified.

## Fixed profile identity rule

The only relative authority remains the exact literal
`.catdesk/wake-bridge/browser-profile`. An absolute value is accepted only if
the fixed expected directory and every caller path component are safe
directories and `os.path.samefile(candidate_identity, expected)` proves exact
object identity. There is no basename, prefix, containment, suffix, or alias
fallback.

`_assert_directory_component_is_safe` now checks every component and the final
resolved object with `os.lstat`. It fails closed on non-directory or symbolic
link. On Windows it additionally requires usable metadata and rejects:

- `os.path.isjunction(path)`;
- `metadata.st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT`;
- missing/inconsistent Windows metadata or an unavailable junction predicate.

This preserves rejection of alternate copies, sibling/outside paths, dot and
parent aliases, embedded NUL/control data, UNC, device, verbatim/prefixed,
missing, and non-directory values.

## Offline tests and bounded execution bridge

The Python fixture covers exact relative and absolute success, alternate and
outside copies, traversal/prefix/device forms, wrong type, target drift,
symlink escape where constructible, and deterministic Windows junction,
generic reparse, and missing-metadata predicate branches. The deterministic
predicate seams do not require creating a live Windows junction.

`tests/stable_wake_adapter_python.rs` can invoke only these constructed fixed
paths:

1. `<workspace>/.catdesk/wake-bridge/venv/Scripts/python.exe`
2. `<workspace>/tests/test_stable_wake_browser_adapter.py`

It accepts no caller arguments or shell command, verifies both are regular
non-link files, uses null stdin, bounds each output stream to 32 KiB, and kills
after 60 seconds. It does not inspect browser-profile contents, credentials,
or live `.catdesk` delivery data.

Actual execution command and result:

```text
cargo test --test stable_wake_adapter_python
FAILED: No Python at
'C:\\Users\\Volap\\AppData\\Local\\Programs\\Python\\Python312\\python.exe'
```

The fixed venv's `pyvenv.cfg` identifies version `3.12.10` and that base
interpreter location. It conflicts with the plan's stated `3.14.2` preflight
and is unavailable in this execution environment. No fallback `python` or
`py` command was available, and this task forbids substituting either one.

## Verification evidence

| Command | Result |
| --- | --- |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test stable_wake_owner` | PASS (7 focused tests) |
| `cargo test --test stable_wake_adapter_python` | FAIL CLOSED: fixed venv base interpreter unavailable |
| `cargo test` | FAIL CLOSED only at the same required bridge; preceding main suite: 745 passed, 21 ignored |
| `git diff --check` | PASS (tracked diff; T-0256 files are new/untracked in the existing dirty workspace) |
| `rust_full` | not run after the blocking required fixture result |

## Ownership and prohibited mutation checks

The Rust source regression still asserts the adapter has no `state.json` or
`review-inbox.json` write authority, no generic workspace/target arguments,
and fixed script/profile authority. The legacy Python bridge remains the sole
live owner. This slice did not activate Rust ownership, launch/type/click/send
a browser, modify real `.catdesk`, change target/profile, alter daemon/release
or Secure MCP/tunnel state, install host services, publish Git, or inspect
browser storage/cookies/credentials.

## Mechanical gate

- Reparse hardening source/tests: **PASS**
- Fixed bounded fixture bridge source: **PASS**
- Required actual fixture execution: **FAIL CLOSED / BLOCKED**
- Overall T-0256 acceptance: **FAIL — environment repair of the fixed venv is
  required outside this provider turn, followed by rerunning the exact bridge
  and remaining verification.**

## Residual boundary

T-0257-style live owner cutover/canary/restart remains out of scope. Legacy
Python remains the sole live browser-submit owner.
