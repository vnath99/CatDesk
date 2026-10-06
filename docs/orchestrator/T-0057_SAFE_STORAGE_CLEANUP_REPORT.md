# T-0057 Safe Storage Cleanup Report

Generated 2026-08-12 for the approved T-0057 cleanup contract. The operation
used a frozen, digest-bound manifest and did not start, stop, reload, or
otherwise alter a CatDesk runtime, browser, tunnel, or daemon.

## Result

| Measure | Value |
| --- | ---: |
| Structurally proven rebuildable roots removed | 26 |
| Reclaimed bytes | 67,476,925,835 |
| Reclaimed GiB | 62.84 |
| Post-cleanup workspace GiB | 8.617 |
| Locked/access failures | 0 |

The exact pre-cleanup allowlist and byte measurements are in
`T-0057_SAFE_STORAGE_CLEANUP_MANIFEST_PRE.json`; the per-root removal receipts
are in `T-0057_SAFE_STORAGE_CLEANUP_MANIFEST_EXECUTION.json`. Both are relative
path evidence and the pre-manifest records a workspace fingerprint rather than
an absolute user path.

## Admission gate

`scripts/cleanup-catdesk-storage.ps1` considers only immediate historical
`target` directories (never `target\release`) and the specified historical
`.catdesk\t0046*`, `.catdesk\t0047*`, `.catdesk\t0048*`, and `reload-target`
build roots. Every removed root had multiple Cargo-output signatures
(`CACHEDIR.TAG`, `.rustc_info.json`, and a build output directory), had no
reparse point, no protected marker anywhere below it, and matched the frozen
byte count immediately before removal.

The command explicitly rejects `.git`, `target\release`, `.catdesk\autonomy`,
`.catdesk\wake-bridge`, `.catdesk\restart-handoff`, active plan/decision/state
files, projects, and logs. It also rejects unknown data. The Windows process
inspection used to decide whether `target\debug` is live was access-denied in
this environment, so the gate failed closed and retained that tree.

## Post-cleanup retention check

The post-cleanup metadata audit is
`T-0057_STORAGE_INVENTORY_RETENTION_POST.json`. It confirms that all required
protected roots remain present:

- `.git`
- `target\release` (about 1.010 GiB)
- `.catdesk\autonomy`
- `.catdesk\wake-bridge`
- `.catdesk\restart-handoff`

No protected runtime/configuration root, review evidence, source tree, user
identity, credential, account, or browser/profile data was selected for this
cleanup. `target\debug` remains (about 6.858 GiB) pending an independently
authorized maintenance window that can positively establish it is not live.

The retained canonical `target\release\catdesk.exe` matched its retained
`catdesk.exe.sha256` sidecar after cleanup:
`532e05f4f772d12c68990ba313ebe5f9a9854ecb5723703cfbeeddbe99be3689`.
