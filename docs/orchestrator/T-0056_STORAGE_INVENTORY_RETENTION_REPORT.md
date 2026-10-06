# T-0056 Storage Inventory and Retention Report

Generated 2026-08-12 by the read-only `scripts/audit-catdesk-storage.ps1`.
The machine-readable companion is `T-0056_STORAGE_INVENTORY_RETENTION_REPORT.json`.
It records relative paths and metadata only; no protected file contents were read.

## Measured facts

| Consumer | Size (GiB) | Classification | T-0057 eligibility |
| --- | ---: | --- | --- |
| Workspace total | 71.416 | Mixed | No automatic action |
| `.catdesk` | 58.478 | PROTECTED_RUNTIME | Keep / separately migrate |
| `target` | 12.917 | Mixed build output | Review child allowlist only |
| `target\debug` | 6.858 | REBUILDABLE | Candidate after approval |
| `target\release` | 1.010 | KEEP | Retain canonical release identity |
| each historical target-dir shown below | 1.010 | REBUILDABLE | Candidate after approval |
| `.catdesk\wake-bridge` | 0.341 | PROTECTED_RUNTIME | Keep |

The largest protected runtime subtrees are historical CatDesk runtime/control-plane
directories (several are about 2.8 GiB each). Their size alone is not deletion
authority: they may contain recovery, review, or accepted-evidence state.

The principal measured historical build directories are `target\t0051-r2-release`,
`target\t0053-deploy`, `target\t0040-r7-final`, `target\t0054-final`, and
`target\t0051-deploy`; each measured about 1.010 GiB. They are Cargo target-dir
outputs and can be recreated by the documented build/test tooling.

## Proposed T-0057 review allowlist

This is a proposal only; T-0056 performed no cleanup.

- Review-delete only Cargo intermediates and isolated historical `target\*`
  directories classified REBUILDABLE, after excluding `target\release`.
- The currently measured upper bound for such build output is about 11.907 GiB
  (`target` less the canonical `target\release` identity). The exact T-0057
  operation must re-measure and name exact targets before removing anything.
- Cargo can recreate approved target output through normal build/test commands.

## Protected/keep list

- All `.catdesk` paths, including wake-bridge browser/profile/configuration,
  autonomous control-plane/review state, diagnostics, migration backups, and
  recovery evidence.
- `target\release` until an approved replacement release is provisioned and
  fingerprinted.
- Source, scripts, tests, docs, repository metadata, configuration, review
  bundles, and any UNKNOWN classification.

## Audit limits and safety

The audit canonicalizes the workspace, traverses metadata only, uses 64-bit byte
counters, skips reparse points, tolerates access errors, and never follows a
junction or symlink outside the workspace. Classification is conservative:
UNKNOWN is never deletion eligible. This inventory is an evidence artifact, not
a cleanup command.
