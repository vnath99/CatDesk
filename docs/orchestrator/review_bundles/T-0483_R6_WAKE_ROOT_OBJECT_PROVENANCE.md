# T-0483 R6 — Wake root object provenance and release hold

Date: 2026-10-14. Scope: read-only WakeHost source and inert build; no installation or Wake submission.

## Evidence that changed the diagnostic

Connected CatDesk reads WakeHost dev.84, Chat53 generation 34, PID 69708, and the old ambiguous `manual-wake-mcp-1791908511726` submission as `SUBMITTING`. The direct installed dev.84 CLI reads a dev.59/generation-1 store. Windows process inspection confirms PID 69708 is the installed dev.84 binary under the CatDesk daemon PID 22112; both processes report `LOCALAPPDATA=C:\Users\Volap\AppData\Local`. The daemon's workspace read tool sees this repository's current committed review bundle. Matching environment text and workspace do not explain the different Wake store objects. The connected status also reports `STATE_REPLACE_FAILED`. The old event has no current-generation exact receipt and remains quarantined.

The earlier path-only provenance field could hash the same displayed root even if different directory objects were opened. `wake/src/runtime.rs` now reports a SHA-256 of the Windows volume serial and file index obtained from a no-follow opened root directory handle. It emits no raw path, file ID, volume serial, or file contents. A Windows test renames the first directory and creates a replacement at the same path: the path hash remains equal while the object hash changes. Existing bounded config/current/status hashes remain intact.

## Local verification and inert candidate

- Independent WakeHost all-features suite: exit 0 (51 library tests, remaining integration binaries passed; one documented live-start test ignored).
- Root all-features Rust suite: exit 0; main binary 1,068 passed, 26 ignored; integration binaries passed. The expected AppContainer child access-denied diagnostic appears in output while the parent test passes.
- Wake and root formatting, strict all-target Clippy, and `git diff --check`: pass.
- `wake/install.ps1 -BuildOnly -RebuildGui`: exit 0. It built source artifacts only; no publish/activate action ran.
- Inert candidate ID derived from host and GUI hashes: `1.0.0-dev.85-0e936b150e0f-6d74342e3f48`.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `wake/target/release/CatDeskWakeHost.exe` | 1,135,104 | `0e936b150e0f5024a2a8f22c2d8172dcdc730f76a326e33c94852b7a45d8a5ae` |
| `wake/target/catdesk-gui/release/catdesk.exe` | 27,048,448 | `6d74342e3f489f2b6cbd7ad24761b347fd193c755bae0741bfe65bfa5f170194` |
| `wake/adapter.py` | 12,843 | `f4e01f6e2fb392ae59d2a8ac173587a373a8b5fbb4106250a6e4302ba82ed20d` |
| `scripts/wake_bridge.py` | 110,899 | `a1816d2dec83d693300aa5d0fbb6b5c9b2fdfc0bc1b7303d66f1cbe57bdcbc9a` |

Read-only direct candidate output: root path SHA-256 `27922b1f60055b6a77522ebf5d56e190dd31b363110b7806faba2c3df98c76eb`, root object SHA-256 `c70b11488090ea47b2c6bdfb5c751f846dd4b1f8441f4b0cde5fe482af814c28`. The connected daemon does not yet expose the new field, so there is no comparable live root-object readback.

## Independent CI and protected release gate

No exact-commit GitHub Windows CI run exists for the preceding source commits. Repository Actions permissions report `enabled:true`, but workflow dispatch returns HTTP 422, `Actions has been disabled for this repository`. [GitHub's repository Actions documentation](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-github-actions-settings-for-a-repository) says a repository or account can be in a separate GitHub-controlled disabled state; changing the normal Actions setting does not restore that state, and GitHub Support must review it. A normal permission-setting PUT was already attempted without effect. Do not claim independent CI or retry settings toggles as proof.

This source candidate requires an exact Windows CI run, reviewed immutable publication/activation, matching connected/direct path and object provenance, and explicit protected-install approval before a fresh Chat53 canary. No source-current binary was used to replace the installed package or touch the external Secure MCP tunnel.
