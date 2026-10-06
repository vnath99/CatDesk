# T-0170 R7C-R1 Snapshot Producer/Consumer Closure

T-0168 wrote committed snapshots at
`.catdesk/reviewed-source-snapshots/<session>/manifest.json`, while the R7
consumer still inspected the rejected legacy `<session>.json` path. The
consumer now reads the canonical committed manifest path only. Legacy sibling
hash manifests remain non-authoritative and return
`REVIEWED_SOURCE_SNAPSHOT_REQUIRED`.

The producer and consumer now share the explicit v3 byte-snapshot envelope:
`schemaVersion=3`, `domain=CATDESK_REVIEWED_SOURCE_SNAPSHOT_V1`, and
`contentRoot=bytes`.  The consumer classifies `manifest.json` with
`symlink_metadata` before reading it, rejects links and non-regular files, and
requires its session/project identity to match the selected review.  This
keeps a legacy hash-only sibling, a malformed manifest, or a substituted
filesystem object outside the reviewed-source authority boundary.

Regression contract: a producer-created directory manifest must be the only
snapshot path observed by the downstream authority gate; a nonempty legacy
`<session>.json` sibling cannot satisfy that gate.

The completion producer remains ordered before completion artifacts/review
authority and stores exact source bytes under its committed `bytes/` root. No
build worker, candidate creation, toolchain selection, promotion, or live
lifecycle action was added.
