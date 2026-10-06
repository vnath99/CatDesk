# T-0060F-R2 Reboot-Hardened Candidate Readiness Review Bundle

## Immutable candidate identity

The supplied isolated candidate was inspected as read-only external evidence:

```text
target\t0060f-r1-candidate\release\catdesk.exe
SHA-256: 65e30b98358d26128af0f78598731efef4d69278c3bd62e244e0dd3be851abb8
size: 22,577,152 bytes
```

Its measured SHA-256 exactly matches the trusted CatDesk reload dry-run
fingerprint. This review did not rebuild, copy, rename, replace, execute, or
otherwise modify the candidate. The fingerprint is immutable candidate identity
evidence; source review below establishes the accepted implementation boundary.

## R1 reboot-hardening source review

- `scripts/start-catdesk-stack.ps1` invokes runtime status through the native
  bounded tunnel-client helper. It fixes the executable and argument vector,
  redirects and caps stdout/stderr, terminates a timed-out child, and caps each
  status command to the remaining single `ReadyTimeoutSeconds` deadline.
- `scripts/setup-secure-mcp.ps1` uses the same bounded native process pattern
  for its fixed status and connect operations; it does not pass arbitrary shell
  text to a child process.
- Loopback local-MCP and runtime health polling uses the scoped silent HTTP
  helper. It sets `ProgressPreference` only during the request and restores the
  caller preference in `finally`.
- `src/ngrok.rs` permits automatic reconnect for `DEGRADED` only when the
  existing official runtime is still running, local MCP is ready,
  `auto_recover` is enabled, credential and tunnel-ID environment references
  are present, and the existing cooldown/attempt-window guard allows it. The
  fixed recovery command remains `runtimes connect`; its regression checks
  reject stop, remove, and create forms.
- A `CONNECTED_VERIFIED` transition removes only the stale
  existing-runtime-not-ready warning; unrelated warnings remain intact.

## Preserved accepted control surfaces

- T-0060B remains a narrow root-facade intercept: only the fixed lifecycle and
  autostart command shapes are accepted; paths, flags, shell syntax, caller
  cwd, and arbitrary command text stay outside the direct PowerShell argv.
- T-0060C-R2 retains its durable promotion transaction and one validated prior
  binary/manifest pair. Interrupted, malformed, mismatched, oversized, or
  reparse-ambiguous recovery state fails closed; execute recovery restores and
  proves the prior pair before clearing its transaction evidence.
- T-0060D retains the fixed `catdesk_production_acceptance` action set
  (`preflight`, `capture_pre`, `capture_post`, `compare`) and its bounded,
  validated PowerShell invocation and evidence locations.

## Deterministic verification record

The following read-only or temporary-fixture verification completed in the
current workspace:

- `scripts/test-start-catdesk-stack.ps1` passed.
- `scripts/test-secure-mcp-route-validation.ps1` passed.
- `scripts/test-catdesk-lifecycle.ps1` passed.
- `scripts/test-catdesk-autostart-supervisor.ps1` passed.
- `scripts/test-promote-reviewed-catdesk-build.ps1` passed.
- `scripts/test-catdesk-production-acceptance.ps1` passed.
- `cargo fmt --check` passed.
- `cargo clippy --all-targets -- -D warnings` passed.
- `cargo test` completed: 457 passed, 18 ignored, 0 failed.
- `git diff --check` passed.

The ignored tests are explicitly marked Windows host prerequisites: four need
a Python interpreter for fake advisor sidecars, three need process-tree
termination which this host denies, and the remaining ignored tests are
operator-local live acceptance probes. Production cancellation still fails
closed on host termination denial.

## Promotion and rollback expectations

The one-command reviewed-build promotion entry point is:

```powershell
.\scripts\promote-reviewed-catdesk-build.ps1 -BuildPath .\target\t0060f-r1-candidate\release\catdesk.exe
```

After independent review and explicit CatDesk-host authorization, the execute
form adds `-Execute` to that exact candidate path. It first proves the
candidate-to-canonical PID-scoped handoff, records a flushed non-secret
transaction, retains one validated prior canonical binary/manifest pair, and
then hands the verified candidate back to the promoted canonical release. An
unfinished, malformed, mismatched, or ambiguous transaction requires recovery;
execute recovery restores and validates the known prior pair. Unproven rollback
or handback remains operator attention, not a retry loop or external-runtime
action.

## Remaining ordered live acceptance

1. Promote the exact reviewed candidate through the helper.
2. Verify canonical handback and the new CatDesk daemon.
3. Enable and check autostart status.
4. Deliberately stop only CatDesk and prove bounded self-healing; preserve
   external official Secure MCP runtime ownership.
5. Capture a passing current-chat pre-reboot snapshot.
6. Stop at the operator reboot boundary.
7. Have the operator reboot and sign in.
8. Capture the post-reboot snapshot and run fixed-path comparison.
9. Run one bounded Codex task proof.
10. Confirm the automatic wake reaches the configured exact chat.
11. Consolidate durable state and independent review evidence.

## Boundary

No source, script, configuration, candidate, canonical release, autonomous
state, daemon, lifecycle, tunnel, browser/wake, Scheduled Task, credential, or
Git publication action was performed by this review. CatDesk host retains
independent verification, live acceptance, and authoritative diff capture.
