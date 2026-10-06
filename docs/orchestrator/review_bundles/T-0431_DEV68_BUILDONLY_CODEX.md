# T-0431 - dev.68 Wake build-only evidence (Codex)

## Classification

`BUILD_ONLY_ARTIFACTS_MATERIALIZED`

This is build evidence only. It does not publish an immutable package, select
an installed package, change `current.json` or `previous.json`, or start,
stop, restart, initialize, or activate WakeHost.

## Commands run

Both requested locked/offline build commands completed successfully:

```text
cargo build --release --locked --offline --manifest-path wake/Cargo.toml
cargo build --release --locked --offline --manifest-path Cargo.toml --bin catdesk --target-dir wake/target/catdesk-gui
```

The GUI build reported a short wait for its build-directory file lock, then
completed successfully. Both commands emitted the inherited warning that
`<USER_PROFILE>
failure.

## Package identity

Readback of `wake/Cargo.toml` confirms the source package version is exactly:

```text
1.0.0-dev.68
```

## Fresh package-input measurements

```text
CatDeskWakeHost.exe
  path:        wake/target/release/CatDeskWakeHost.exe
  byte length: 1104896
  SHA-256:     477a4b0f982c0ba75510a05bc9bd356571040c5adf8183e099de4f17884f0cc9

CatDeskBinagotchy.exe
  path:        wake/target/catdesk-gui/release/catdesk.exe
  byte length: 27287040
  SHA-256:     247f506c99b9237f6fd96d2a4967b6c64ef2bd6f5136a4068937d2aa7f9024e0

adapter.py
  path:        wake/adapter.py
  byte length: 11749
  SHA-256:     d7afbf5bc44144756e41f7d06454027965616531580b88bb2fc3db08be4dafb3

wake_bridge.py
  path:        scripts/wake_bridge.py
  byte length: 87052
  SHA-256:     02646440f25941aead9140b62b453ef6e0b46b3869e28ffc119c6fa9531331a5
```

The two executable paths are the build-only locations used by the reviewed
Wake installer. They are not installed artifacts and confer no activation or
delivery authority.

## Boundaries preserved

No `wake/install.ps1` invocation, `publish-reviewed-install`, `init`,
`set-source`, `activate-reviewed-install`, host lifecycle operation, target
operation, Secure MCP operation, recovery/LKG operation, or Git publication
occurred. The only source-tree artifact attributable to this task is this
review bundle; build outputs are generated verification artifacts.
