# T-0060E Final Candidate Promotion Readiness Review Bundle

## Immutable candidate evidence

The isolated final candidate reviewed by this ticket is exactly:

`target\t0060-final-candidate\release\catdesk.exe`

Its measured SHA-256 is exactly:

`4afe4fb3a7aad3a8f1055a83169d5a37dab77803687c8073549c976ede33ffc1`

This matches the trusted CatDesk reload dry-run fingerprint supplied for this
review. The candidate is external immutable evidence for promotion review; it
was not rebuilt, replaced, loaded, or executed here. `target/release` was not
read as a promotion target or changed.

## Accepted implementation chain

- T-0060B is present in the reviewed source: only exact root `catdesk.ps1`
  lifecycle shapes are intercepted, then launched through a fixed direct
  PowerShell argument vector. Arbitrary command text, paths, flags, and cwd
  values do not cross that boundary.
- T-0060C-R2 is present in
  `scripts/promote-reviewed-catdesk-build.ps1`: execute mode writes a flushed
  bounded transaction before canonical mutation, and subsequent invocation
  checks that transaction before ordinary canonical validation. Interrupted,
  malformed, mismatched, or reparse-ambiguous state fails closed; recovery
  restores and validates the sole prior binary/manifest pair before clearing
  transaction evidence or performing a needed bounded canonical handoff.
- T-0060D is present in `src/mcp.rs`: the first-class
  `catdesk_production_acceptance` surface accepts only fixed actions and an
  opaque wake-target hash where required, invokes only the canonical
  PowerShell checker with a direct argv vector, and limits capture evidence to
  bounded atomic files under `.catdesk/production-acceptance`.

The T-0060B, T-0060C-R2, and T-0060D review bundles and canonical architecture
document describe the same boundaries. The PowerShell promotion helper remains
the sole one-command promotion path.

## Promotion and rollback expectation

The read-only promotion plan shape is:

```powershell
.\scripts\promote-reviewed-catdesk-build.ps1 -BuildPath .\target\t0060-final-candidate\release\catdesk.exe
```

After independent review and explicit host authorization, the corresponding
operator promotion shape adds `-Execute` to that exact candidate path. It first
requires a validated canonical binary/manifest pair and a verified candidate
handoff. It retains only one validated prior pair in
`.catdesk/promotion-recovery`, records the durable transaction before touching
the canonical pair, and returns the daemon to canonical bytes only through the
existing PID-scoped handoff. Any incomplete, tampered, missing, reparse, or
unproven state is fixed operator attention rather than silent continuation or
broad cleanup. An unfinished transaction even with a fully candidate-equivalent
canonical pair prefers rollback to the prior known-good pair.

## Source verification record

The isolated candidate hash check matched the trusted value above. The current
reviewed source passed `cargo fmt --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, and deterministic
lifecycle, promotion-recovery, and production-acceptance fixtures.

`cargo test` exercised the current 472-test baseline: 454 passed and 11 were
ignored. Seven existing host-only failures remain outside this readiness review:
three advisor tests require an unavailable local advisor executable, and four
process-cancellation paths are denied by the Windows host. Those failures do
not execute or modify the isolated candidate.

## Required host-operated live acceptance order

1. Promote the exact reviewed candidate through the one-command helper.
2. Verify the canonical release and the live new CatDesk daemon.
3. Enable autostart through the supported lifecycle surface.
4. Deliberately stop only CatDesk and prove bounded self-healing; do not touch
   the externally owned Secure MCP runtime.
5. Capture a passing pre-reboot production-acceptance snapshot.
6. Have the operator perform reboot/login.
7. Capture the post-reboot snapshot and run fixed-path comparison.
8. Run one bounded Codex task.
9. Confirm the automatic wake reaches the configured exact ChatGPT chat.
10. Consolidate the resulting durable state and independent review evidence.

## Ticket boundary

This ticket did not rebuild, promote, reload, register or modify Scheduled
Tasks, invoke a lifecycle mutation, launch a browser or wake, touch the
external Secure MCP tunnel, inspect credentials, write protected CatDesk state,
or publish Git state. CatDesk host independently owns final promotion, live
acceptance, verification, and authoritative diff capture.
