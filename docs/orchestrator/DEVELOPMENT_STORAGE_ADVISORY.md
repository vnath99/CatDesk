# Ordinary development storage advisory

Source-only command, added November 3, 2026:

```powershell
.\scripts\catdesk-dev-lane.ps1 storage-status
.\scripts\catdesk-dev-lane.ps1 storage-status -StorageWarningFreeBytes 34359738368
```

The default warning threshold is 64 GiB (68,719,476,736 bytes); the second example uses 32 GiB. A warning is true strictly below the threshold. Zero disables low-space warnings. A negative threshold is invalid input.

This opt-in operation reports one timestamped local-volume snapshot, its volume GUID/root, free and capacity bytes, and the source workspace's default `target` reference path. `effectiveCargoTargetVerified=false`: it does not resolve Cargo configuration, environment overrides, junction destinations or actual build allocation. It issues one volume metadata query and performs no directory scan, compiler invocation, repair, cleanup or scheduling.

`state=AVAILABLE` includes the advisory boolean `warning`. A counter failure or unsupported/ambiguous volume produces `state=UNAVAILABLE`, `warning=null` and a fixed reason without exposing the provider exception. Neither result is permission to delete files or a build/benchmark admission decision. No automatic action follows a warning.

Callers can retain separate snapshots before and after an independently authorized operation. Compare only the same volume identity, and label any free-space difference as **background-inclusive volume change**, never target allocation or storage consumed by that operation alone. Capacity/free counters include concurrent activity.

Compatibility review found the original development receipt contract specifies a fixed field set. Consequently `status`, `build` and `verify` retain their existing receipt fields, command arguments, exception behavior and verification environment restoration. Storage observations are deliberately a separate opt-in command; they are not injected into those existing receipts or failed command paths. No existing historical receipt is rewritten.

Qualification is synthetic source coverage only: focused tests exercise volume failures, threshold boundaries/configuration, legacy source/receipt preservation and verification failure/environment restoration. No real storage facade invocation or Cargo workload was used for this change. The installed canonical scripts/executable and protected reviewed-build worker remain separately qualified versions.

Run the focused synthetic test with an existing supported PowerShell host:

```powershell
.\scripts\test-catdesk-dev-lane-storage.ps1
```

Evidence: `docs/verification/DEVELOPMENT_STORAGE_ADVISORY_SOURCE_2026_11_03.json`.
