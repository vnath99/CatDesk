# T-0255 / T-0224 R1B-R3A — deployed profile-path compatibility review bundle

## Scope and inherited defect

T-0254 produced a useful source-level owner/adapter seam but was rejected for
the separate host-live cutover boundary. Its adapter required the literal
relative profile string `.catdesk/wake-bridge/browser-profile`, while the
protected deployed config uses the same dedicated profile as a normal absolute
Windows path.

This slice corrects only that representation mismatch. It does not inspect
browser-profile contents, cookies, browser storage, credentials, browser state,
or the real wake config. It does not create an owner selector, activate Rust
ownership, invoke a browser, send a wake, or modify the legacy bridge.

The non-secret deployed shape represented by isolated fixtures is:

```json
{"profile_dir":"<absolute workspace>/.catdesk/wake-bridge/browser-profile"}
```

No actual profile path, conversation URL, target digest, or config content is
recorded here.

## Corrected exact-identity rule

`scripts/stable_wake_browser_adapter.py` now has one production validator,
`validate_exact_profile`.

1. The allowed object is derived, not supplied:
   `<workspace>/.catdesk/wake-bridge/browser-profile`.
2. A relative value is accepted only for the exact normalized product spelling
   `.catdesk/wake-bridge/browser-profile`. Dot aliases, traversal, rooted or
   prefixed values, and alternate relative names are rejected.
3. An absolute normal local path is accepted only after component-by-component
   no-follow directory validation and `os.path.samefile` equality with the
   validated fixed profile. Textual prefixes and generic workspace containment
   are never authority.
4. UNC, device/verbatim/prefixed forms, empty/non-string/oversized/control
   values, sibling/outside/different in-root profiles, files/special final
   objects, symlink/junction/reparse hops, and metadata ambiguity fail closed.

The normal absolute Windows spelling is allowed only when its actual filesystem
identity is the sole protected profile. Platform `samefile` semantics preserve
Windows canonical/case behavior without making aliases new authority.

Target handling is unchanged: the existing canonical target must pass the
existing exact target SHA-256 comparison. This change neither selects nor
infers a target.

## Changed files and attribution

The workspace was already broadly dirty with earlier T-025x sources untracked,
so whole-worktree Git output is not task attribution. This session changed:

| Path | T-0255 change |
| --- | --- |
| `scripts/stable_wake_browser_adapter.py` | Exact protected-profile identity validator and config preflight use. |
| `tests/test_stable_wake_browser_adapter.py` | Offline isolated relative/absolute/negative/no-browser preflight matrix. |
| `src/stable_wake_owner.rs` | Regression assertions for exact-profile validator and identity comparison. |
| this bundle | Review evidence. |

No source was changed to create a second state writer or browser-submit owner.
The legacy Python bridge remains the only live submit owner; Rust mode remains
unselected on the real host.

## Offline fixture matrix

`tests/test_stable_wake_browser_adapter.py` uses temporary roots only and
never calls `CdpSink`, Selenium, or a browser. It covers:

| Fixture | Expected result |
| --- | --- |
| Exact canonical relative profile | Accepted. |
| Exact absolute profile identity (deployed shape) | Accepted. |
| Absolute sibling / arbitrary different in-root profile | Rejected. |
| Outside profile | Rejected. |
| Dot/traversal/alternate relative form | Rejected. |
| Empty, malformed/non-string, oversized profile | Rejected. |
| UNC/device/verbatim/prefixed alternatives | Rejected. |
| Final profile is a file | Rejected. |
| Profile link escape (where fixture links are available) | Rejected; otherwise only that OS capability skips. |
| Wrong target hash / target drift | Rejected before browser code. |
| Absolute deployed-shape config preflight | Returns fixture target/profile/timeouts; config bytes remain identical and `seleniumbase` is not imported. |

The production adapter regression in `stable_wake_owner` continues to reject
direct state/inbox write patterns and caller-selected workspace/target
arguments. It additionally requires the exact validator, exact relative
condition, and `samefile` identity comparison to remain present.

## Preserved authority

This is profile validation only. It preserves canonical immutable
`.catdesk/autonomy/review-inbox.json`, schema-4 delivery state, receipt schema
1, exact record/message/target receipt binding, `CLAIMED -> SUBMITTING ->
SENT`, non-retryable unresolved `SUBMITTING`, T-0251 kernel-lock/crash safety,
T-0252 terminal-history retirement, and T-0253 stateless adapter boundary.

## Verification evidence

| Command | Result |
| --- | --- |
| `cargo test stable_wake_owner` | Passed: 7 focused owner/parser/source regressions. |
| `cargo test stable_wake_delivery` | Passed: 12 T-0251/T-0252 claim/history/receipt tests. |
| `cargo test --test stable_wake_delivery_lock` | Passed: 3 hard-kill/contention child-process tests. |
| `cargo fmt --check` | Passed. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test` | Passed: 745 passed, 21 ignored, 0 failed; existing host and process suites passed. |
| `git diff --check` | Passed. |
| Python syntax/import + offline Python fixture | **Not run**: the local shell has no `python`/`py` interpreter. The test is present for the established host Python harness; no browser was invoked. |
| Release build | Not rerun in this narrow corrective slice; T-0254's fixed-ceiling timeout remains an explicit non-pass. |

Independent CatDesk verification remains authoritative.

## Prohibited work and residual boundary

No real `.catdesk` file, target, profile, inbox, delivery state, browser,
daemon, release, service, scheduler, tunnel, external project, Git, signing,
or provenance state was mutated. No live owner selection or canary occurred.

T-0256-style work remains: independent review of this exact-profile correction,
then separately authorized host-live single-owner cutover, fresh normal canary,
durable W13 receipt check, and bounded restart/replay-suppression proof. If a
host preflight fails, leave legacy ownership live and open a narrow corrective
ticket rather than forcing a browser attempt.
