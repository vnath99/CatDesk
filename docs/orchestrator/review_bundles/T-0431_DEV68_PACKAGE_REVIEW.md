# T-0431 — dev.68 independent package review

## Verdict

ACCEPT DEV.68 FOR IMMUTABLE PUBLISH/ACTIVATE TRANSACTION.

This review accepts the exact dev.68 Wake package candidate and does not itself
claim live browser acceptance.

## Source and verification authority

- T-0430 active-generation refresh/browser-close logic is independently accepted.
- T-0431 source verification session `adc-t0431-dev68-verification-r2-20260927`
  is `COMPLETED_VERIFIED`.
- Its approved root verification passed strict Clippy and the full 991-test suite,
  including the fixed T-0430 active-generation refresh regression.
- The prior R1 isolated-release failure is excluded from Wake package correctness:
  Codex lock-provenance review proved PID 35464 is currently executing the exact
  fixed isolated CatDesk path
  `.catdesk/verification-targets/autonomy-release/release/catdesk.exe`.
  Rebuilding that path would overwrite the serving daemon and is neither safe nor
  relevant to Wake dev.68 packaging.

## Dev.68 identity

Source/lock identity is consistently `1.0.0-dev.68` in:
- `wake/Cargo.toml`
- `wake/Cargo.lock`
- root `Cargo.lock` path dependency.

## Fresh build-only package inputs

Codex session `adc-t0431-dev68-buildonly-codex-20260927` completed
`COMPLETED_VERIFIED` after both reviewed locked/offline build commands passed.

Exact package inputs:

- CatDeskWakeHost.exe
  - SHA-256: `477a4b0f982c0ba75510a05bc9bd356571040c5adf8183e099de4f17884f0cc9`
  - length: 1104896
- CatDeskBinagotchy.exe
  - SHA-256: `247f506c99b9237f6fd96d2a4967b6c64ef2bd6f5136a4068937d2aa7f9024e0`
  - length: 27287040
- adapter.py
  - SHA-256: `d7afbf5bc44144756e41f7d06454027965616531580b88bb2fc3db08be4dafb3`
  - length: 11749
- wake_bridge.py
  - SHA-256: `02646440f25941aead9140b62b453ef6e0b46b3869e28ffc119c6fa9531331a5`
  - length: 87052

## Publication boundary

The only accepted promotion path is the zero-argument `wake/install.ps1` facade.
It rebuilds with locked/offline Cargo, hashes the four inputs, stages a unique
inert package directory, delegates publication serialization and artifact
validation to `publish-reviewed-install`, initializes/binds the source, then
performs the stop -> exact-pointer switch -> desired-state restore transaction
through `activate-reviewed-install`.

Do not use `-BuildOnly` as an operator/MCP path, direct version-directory
copying, direct `current.json`/`previous.json` editing, manual host
registration, legacy release recovery, or root isolated CatDesk rebuild.

## Remaining live acceptance

Successful package publication/activation is not the final Wake acceptance.
After exact dev.68 readback and T-0429 serving/current parity, resume the SAME
T-0425 and require a fresh natural event to prove:

USER once -> Stop/Pause active -> bounded 2-minute same-target periodic refresh
-> exact USER revalidated -> completion -> terminal SENT -> owned browser closes.

This review authorizes only the immutable dev.68 publish/activate transaction.
