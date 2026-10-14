# T-0483 R3 — Wake store authority provenance

Date: 2026-10-14. Scope: read-only Wake store diagnosis before the dev.85 immutable installation.

## Observed contradiction

- The connected CatDesk MCP `catdesk_transport_status` and Binagotchy `status` report an actively updated dev.84 WakeHost, PID 69708, Chat53 generation 34 and digest `b9c3c16a978e424ff4122f868450d4f7b8c540578dda2cc652cd3e04c557a3e3`. The previous current-generation event `manual-wake-mcp-1791908511726` remains `SUBMITTING` with `SUBMIT_TARGET_DRIFT_CHROME_ERROR` and no event-specific exact receipt. It must not be replayed.
- Executing the installed dev.84 `CatDeskWakeHost.exe status` directly under the same user returns a dev.59 STOPPED record and Chat52 generation 1. The direct `%LOCALAPPDATA%\CatDeskWake\status.json` is dated September 24; its `config.json` has generation 1. `reviewed-install-status` still reads the dev.84 `current.json`.
- The daemon and WakeHost processes report the same `LOCALAPPDATA` value as the diagnostic shell, and both are non-AppContainer, non-virtualized, non-elevated tokens. The connected CatDesk tool read a fresh nonce created in the same repository immediately, proving that repository file access is shared. A bounded Windows handle-name/file-ID probe found the WakeHost's open `host.lock` at the displayed local root, but its file ID differed from the file ID opened through that pathname by the diagnostic shell. This is evidence of an authority or file-identity divergence, **not yet an explanation of its cause**. No process environment contents other than `LOCALAPPDATA` or browser/profile contents were exposed.

## Source increment

- `wake/src/runtime.rs` now computes SHA-256 provenance for the canonical root path and the bounded `config.json`, `current.json`, and `status.json` files. It rejects non-files, symlinks/reparse points and files over 64 KiB. It returns only hashes or absence, never file bodies, targets, paths or credentials.
- `CatDeskWakeHost.exe store-provenance` exposes the same readback through a fixed, argument-free CLI command.
- `catdesk_transport_status` gains `independentWakeStoreProvenance` using that shared readback. The response is read-only and reports an error code if the root/file cannot be inspected.
- Tests verify stable hashes, a changed config hash without unrelated hash changes, absence of raw file contents, oversize rejection, and the CLI's read-only missing-file behavior. The CLI dispatches this command before its normal `Store::open` so an absent root stays absent; an integration test checks that case.

This source increment has **not** been deployed into the already-running CatDesk daemon or installed WakeHost. It does not prove that both currently observe the same physical store. The dev.85 package hashes from R1 must be recomputed after this Wake source change; old R1 binary hashes are not the new candidate's identity.

## Local verification

- WakeHost `cargo test --locked --offline --manifest-path wake/Cargo.toml --all-features -q`: PASS, including 50 library tests and the fixed CLI integration test; one manual startup diagnostic and one Python bridge test are intentionally ignored.
- Focused Python `pytest.exe tests/test_wake_bridge.py tests/test_stable_wake_browser_adapter.py -q`: exit 0, one skip.
- CatDesk `cargo test --locked --offline --bin catdesk transport_status -q`: exit 0 (two selected existing tests). This confirms main binary compilation; those tests do not independently validate the new provenance field.
- Strict `cargo clippy --locked --offline --workspace --all-targets --all-features -- -D warnings`, strict Wake Clippy, root/Wake formatting and `git diff --check`: PASS. Final exact-commit Windows CI remains required.
- The rebuilt dev.85 **candidate** `CatDeskWakeHost.exe store-provenance` returned schema 1 and hashes matching independent PowerShell SHA-256 readback of all three directly visible local files: config `839c0363fe10c5abe9161c2565925b9a8ebffa74a9571660b65b32fce6c51590`, current `25b8430dc25fdb7cf2be3465d5f6c5431f6ff274d0dfcf3e09f9c2aa3cb50185`, status `2aaa989889b2cf21746f957423a90ed089e106f0b4b7577b5c016c7c84aa5848`. This proves the diagnostic reports the direct store faithfully; it does not reconcile the connected daemon's different live view.

## Rebuilt inert dev.85 candidate

`wake/install.ps1 -BuildOnly -RebuildGui` built the candidate without publication or activation. After the final bounded-read correction, both release host and GUI were rebuilt from the final source. The hash-derived prospective package ID is `1.0.0-dev.85-8e96f5157414-529cd47bc211`:

| Artifact | SHA-256 |
| --- | --- |
| `CatDeskWakeHost.exe` | `8e96f51574140c3d2ea96a46295b114534e4c648da5eb1c708187981216ca9de` |
| `CatDeskBinagotchy.exe` | `529cd47bc211b9f38302cf4a57128c9583a22abcb41d549f59924322327e3607` |
| `adapter.py` | `f4e01f6e2fb392ae59d2a8ac173587a373a8b5fbb4106250a6e4302ba82ed20d` |
| `wake_bridge.py` | `a1816d2dec83d693300aa5d0fbb6b5c9b2fdfc0bc1b7303d66f1cbe57bdcbc9a` |

These are build artifacts only. There is no reviewed dev.85 package in the installed versions directory and no active-pointer handoff.

## Activation gate

1. Obtain readback from the exact serving CatDesk daemon and the exact installed/candidate WakeHost through supported controls, comparing root, config, current and status hashes at a stable observation point. Investigate any mismatch; do not activate against an uncertain target.
2. Independently verify the new exact source commit's three Windows CI jobs, rebuild and review the changed immutable dev.85 candidate package/hash manifest, and obtain the reviewed-install approval required by the handoff.
3. Only then use the immutable installer. Verify version/hash readback and run one **new** Chat53 generation-34 canary with exact message receipt, `SENT`, timer `COMPLETE`, and no duplicate submission. The old ambiguous event remains forensic.

No installation, daemon reload, browser submission, tunnel change or hourly automation was made for this increment.
