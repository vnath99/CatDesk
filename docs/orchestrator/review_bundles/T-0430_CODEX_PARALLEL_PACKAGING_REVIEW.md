# T-0430 - Codex parallel Wake packaging review

## Scope and conclusion

This is a source-only packaging review.  No implementation, test, installed
Wake, target, recovery/LKG, Secure MCP, or Git state was changed.  The current
Wake manifest is `1.0.0-dev.67`; the T-0430 ticket records
`IMPLEMENTED_PENDING_WAKE_VERIFICATION`.  Therefore no T-0430 package is ready
to publish or activate yet.

The existing installed dev.67 immutable package evidence is
`1.0.0-dev.67-21592c86cff7-57791d5851a3`.  It is historical serving evidence,
not authority to overwrite or relabel a T-0430 candidate.

## Reviewed packaging authority

`wake/install.ps1` is the supported packaging facade.  It first builds the
Wake host with `--release --locked --offline`, and builds Binagotchy under its
isolated `wake/target/catdesk-gui` target.  It hashes exactly these staged
artifacts:

- `CatDeskWakeHost.exe`
- `CatDeskBinagotchy.exe`
- `adapter.py`
- `wake_bridge.py`

The install identity is
`<manifest-version>-<first-12-host-sha256>-<first-12-binagotchy-sha256>`.
The complete SHA-256 values, schema version, protocol version, and
`DEVELOPMENT_NOT_ACTIVATED` acceptance value are written to the staging
`manifest.json`.

PowerShell is intentionally not the publication authority.  It copies to one
unique `.staging-*` directory and readbacks every copied hash, then invokes the
candidate host's `publish-reviewed-install`.  Rust takes the exclusive
`install.lock`, rejects malformed components, validates the manifest version
against the compiled `VERSION`, recomputes every staged artifact hash, excludes
staging directories from candidates, and either atomically materializes the
exact identity or accepts an exact idempotent replay.  Same-version/different
identity and same-identity/different-artifact cases fail closed.

`current.json` is a content-bound pointer, not merely a version selector.  On
activation it contains the final directory plus the host, Binagotchy, adapter,
and bridge SHA-256 values.  `start_installed` and the adapter spawn boundary
revalidate the installed host and Python artifacts against both this pointer
and `manifest.json` before use.

## Activation and restart boundary

Only `activate-reviewed-install` performs the handoff.  Under the same Rust
install lease it validates the sole reviewed candidate, records the prior
desired state in `reviewed-install-handoff.json`, stops the old owner, waits
for STOPPED and singleton-lease release, saves the old pointer to
`previous.json`, atomically switches `current.json`, and restores RUNNING,
PAUSED, or STOPPED intent.  RUNNING is restarted only through the new installed
host after installed-artifact verification.  Interrupted exact replays resume
the recorded candidate and original desired state; competing candidates fail
closed.

The installer removes the legacy `CatDesk Wake.lnk` only as a
presentation-only best-effort action and publishes the Binagotchy shortcut in
a bounded child process.  Shortcut failure is not package/activation
authority.

## Safe next-package checklist

1. Finish T-0430's focused Wake Python/Rust verification, release build, and
   independent source review; do not treat the current pending ticket as
   package approval.
2. Allocate a new immutable Wake package version.  Since the current manifest
   is dev.67, the expected next family member is dev.68 unless the authoritative
   manifest at packaging time has advanced; never reuse dev.67 or infer a
   version from a directory.
3. Update the package manifest/lock only as part of the accepted T-0430 source
   change, then build with the reviewed locked/offline commands.  Re-measure all
   four package artifacts before staging.
4. Use only the reviewed zero-argument `wake/install.ps1` facade.  Its
   allowlist intentionally refuses caller flags such as `-BuildOnly`; do not
   copy files directly into `CatDeskWake/versions`, handwrite a manifest, or
   invoke a host registration command as a substitute.
5. Require publication readback to return the exact new version and computed
   install identity.  On an identity, manifest, artifact, or install-lease
   conflict, stop and preserve the failure evidence; never replace a version
   directory.
6. Bind the reviewed source with the candidate host's supported `init` and
   `set-source` calls, then call only `activate-reviewed-install`.  Preserve
   the existing target and operator desired state; activation owns the
   stop/wait/pointer-switch/conditional-restart transaction.
7. Read back the installed status/current identity and all four pointer hashes
   before enabling ordinary delivery.  If prior intent was PAUSED, keep it
   PAUSED through readback rather than manually starting a host.
8. Run live acceptance only as a separately authorized phase with a fresh
   current-generation event.  Require the T-0430 sequence (one USER append,
   bounded same-target active-generation refresh, completion proof, terminal
   SENT, and owned-browser close).  Never replay the historical dev.66/67
   diagnostic records.

## Do-not-reuse paths and residual risks

- Do not use `catdesk_release_recovery` or its legacy release/LKG scripts for
  Wake packaging.  Those govern the CatDesk controller; current evidence says
  the legacy MCP recovery surface is closed and not an activation substitute.
- Do not use generic daemon reload/restart, a direct `current.json` edit,
  direct executable replacement, or a stale installer/staging directory as a
  Wake handoff.  None proves the immutable Wake artifact set or performs the
  owner handoff transaction.
- The T-0430 source delta must still demonstrate its active-generation refresh
  behavior without weakening timeout-retry, exact-target, latest-USER, or
  no-duplicate-submission boundaries.
- Package publication establishes artifact identity, not live browser
  acceptance.  The first live event remains a separate risk boundary and must
  preserve existing forensic SUBMITTING/ATTENTION records.
- This workspace is intentionally broadly dirty.  `git diff --check` passed;
  `git status --short` shows inherited changes across the repository.  The
  only output attributable to this review is this file.

## Review evidence

- Inspected `wake/install.ps1`, `wake/src/runtime.rs`, the Wake host CLI
  publication/activation commands, `T-0425` through `T-0428` review evidence,
  `WAKE_CHAT34_POSTLOGIN_MATURITY.md`, and the current T-0430 ticket.
- Ran `git status --short` and `git diff --check`; the latter exited cleanly.
- No install, restart, publication, browser action, or Wake Store operation
  was invoked.
