# T-0324 SR3M — reviewed-build bootstrap exposure

## Technical design before implementation

### Exact integration gap

SR3L identifies the corrected wake-target bridge source as the first reviewed
worker source basis. Current source already implements the protected
`catdesk_reviewed_build` control semantics in
`AutonomousSupervisorV1::reviewed_build`: `PREPARE` accepts only
`{action, recordId}` and derives authority through acknowledged-review
remeasurement; `CONFIRM` accepts only `{action, confirmationToken}`; `RESULT`
accepts only `{action}`. The current project-local operator facade has no route
for those three fixed shapes. Its existing project-control route is unrelated.

The connected legacy catalog response `buildPath is required` is host/catalog
skew: it must not cause a raw reload call, a caller-selected build path, or a
weaker compatibility input. The source gap is only a closed local transport to
the already-present handler.

### Fixed route and authority boundary

The proposed public grammar is exactly:

- `operator reviewed-build prepare --review-record-id <record-id>`;
- `operator reviewed-build confirm --confirmation-token <token>`; and
- `operator reviewed-build result`.

The parser will accept one nonempty bounded value only for the first two forms,
reject duplicate or unknown flags, additional arguments, mixed fields, shell
syntax, and every path/hash/tool/environment/project/command selector. It will
construct the only corresponding fixed JSON shape and call the existing
`handle_tool("catdesk_reviewed_build", ...)` dispatch. It will not call the
legacy daemon-reload compatibility branch, create an MCP schema, write a
reviewed artifact, or interpret target/release, Program Files, or main-image
material as provenance.

`PREPARE` remains authoritative only because the existing handler independently
remeasures the supplied review record and binds the existing reviewed-source
snapshot before invoking `prepare_reviewed_build`. `CONFIRM` remains
token-bound within the existing implementation. `RESULT` returns existing
readback only. Replay, stale review, unknown record, bad token, and worker
failure remain existing fail-closed outcomes; this facade adds no alternate
state transition.

## Implementation and classification

**`REVIEWED_BUILD_BOOTSTRAP_BRIDGE_READY`**

`src/operator_facade.rs` now adds a typed `ReviewedBuildOperatorAction` and no
other authority surface. Its parser accepts exactly the three documented
operator forms, limits a record/token to 128 ASCII alphanumeric, hyphen, or
underscore bytes, and emits only these exact supervisor requests:

- `{"action":"PREPARE","recordId":"..."}`;
- `{"action":"CONFIRM","confirmationToken":"..."}`; or
- `{"action":"RESULT"}`.

Execution canonicalizes the existing workspace and delegates directly to
`handle_tool("catdesk_reviewed_build", request, workspace)`. Its public result
is reduced to action/state and, for PREPARE only, the opaque confirmation token
that the existing implementation returned. The bridge does not use
`catdesk_daemon_reload`, the legacy raw `buildPath` schema, an arbitrary command
runner, or a new reviewed-build implementation.

## Threat and failure analysis

The parser rejects unknown actions, duplicate/unknown flags, missing values,
extra arguments, shell-like and path-like values, `buildPath`, source/tool/path
selectors, image hashes, environment, commands, project selectors, and mixed
fields. RESULT takes no values. PREPARE remains subject to existing acknowledged
review remeasurement and reviewed-source snapshot validation; CONFIRM remains
subject to the existing opaque-token/attempt owner and worker policy. Invalid,
stale, replayed, or unknown review/token evidence therefore reaches no new
success path. Mutable `target/release`, Program Files, reviewed-main-image, and
core-host material are never parsed by this facade.

## Focused tests

- `operator_facade::tests::parser_accepts_only_closed_operator_actions` now
  proves all three positive forms and rejects raw build paths, shell syntax,
  path-like values, environment, hashes, project selectors, and mixed fields.
- `operator_facade::tests::reviewed_build_bridge_constructs_only_fixed_supervisor_requests`
  proves the exact request objects and absence of every caller-selected
  build/source/tool/path/hash/environment/command/project field.
- Existing `reviewed_build` regressions continue to cover acknowledged review
  binding, reviewed-source snapshots, protected output/attestation policy, and
  worker failure handling.

## Attribution and prohibited actions

T-0324 SR3M-attributable changes are only:

- `src/operator_facade.rs` — closed parser, typed request projection, fixed
  current-source supervisor delegation, and two focused regression extensions;
- this review bundle.

All existing reviewed-build/supervisor/MCP code and unrelated dirty-worktree
changes remain otherwise unattributed. No PREPARE, CONFIRM, or RESULT request
was executed against host state. No reviewed-build artifact, ProgramData or
Program Files state, release store, worker/supervisor switch, wake/target,
browser, tunnel/Secure MCP, signing/UAC, Git, or external-project state changed.

## Verification and exact next action

- `cargo fmt --all -- --check` — PASS (only the pre-existing environment
  canonicalization warning was printed).
- `cargo test operator_facade -- --nocapture` — PASS: 9 focused facade tests.
- `cargo test reviewed_build -- --nocapture` — PASS: 67 reviewed-build tests;
  3 existing host-profile tests remained explicitly ignored.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  PASS.
- `cargo test --workspace --all-targets --all-features` — PASS; the primary
  unit-test run enumerated 923 tests and all target-specific test binaries
  completed successfully.
- `git diff --check` — PASS; pre-existing working-copy CRLF warnings only and
  no whitespace errors.

After independent final review accepts this bridge, the exact later host action
is `operator reviewed-build prepare --review-record-id <acknowledged-SR3L-record>`;
only the returned opaque token may be supplied to `operator reviewed-build
confirm --confirmation-token <token>`, followed by `operator reviewed-build
result`. That host invocation is outside T-0324 SR3M; it must not be replaced
with a legacy catalog call or any generic transport.
