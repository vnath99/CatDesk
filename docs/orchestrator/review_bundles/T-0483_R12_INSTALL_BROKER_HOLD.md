# T-0483 R12 — Wake installation broker hold

Date: 2026-10-15. Status: source safety hold; dev.85 is not installed.

## Change

- The WakeHost `guarded_install_store` entry now requires a protected installation broker after its live host-owner challenge and before opening the mutable Store. `install-init`, `install-set-source`, `publish-reviewed-install`, and `activate-reviewed-install` all use that entry. A caller that passes owner proof then receives `WAKE_INSTALL_BROKER_UNPROVISIONED`; installed dev.84 can fail at the earlier owner challenge. No self-declared hashes or same-user CLI arguments can authorize an install.
- `wake/install.ps1` refuses a normal install before a build or staging-directory creation. `-BuildOnly` remains available for inert source/package verification. The `test-support` exception is limited to the exact canonical temporary `catdesk-wake-install-authority-test-*/CatDeskWake` fixture.
- Read-only status and owner probes are unchanged.

## Verification and boundary

The isolated default and all-features Wake authority tests and broker-gate tests pass. A normal `wake/install.ps1` invocation returned `WAKE_INSTALL_BROKER_UNPROVISIONED`; a repeat left the same 81 names in the invoking process's `versions` view. This is not a live-store readback. The full independent Wake all-features suite, strict Clippy, rustfmt and PowerShell syntax passed. Its first full run exposed an older integration assertion expecting the retired preflight error; the assertion now verifies the broker hold before build and the full rerun passed. `-BuildOnly` completed with inert dev.85 candidate ID `1.0.0-dev.85-145af26f6e10-30033273eeaa`:

| Artifact | SHA-256 |
|---|---|
| `CatDeskWakeHost.exe` | `145af26f6e10ddc4e620a8b3b7887b5d645d50cad5371741e68a60264ec70b8b` |
| `CatDeskBinagotchy.exe` | `30033273eeaa63049709adf563e8401a98b753b80f2cbffa688211e03508c86b` |
| `adapter.py` | `f4e01f6e2fb392ae59d2a8ac173587a373a8b5fbb4106250a6e4302ba82ed20d` |
| `wake_bridge.py` | `a1816d2dec83d693300aa5d0fbb6b5c9b2fdfc0bc1b7303d66f1cbe57bdcbc9a` |

These are build artifacts, not installed package readback. Exact-commit GitHub Windows CI remains unavailable.

The existing `%LOCALAPPDATA%` Wake store is user-owned. The same interactive user can still write it outside these CLI entrypoints. P0 therefore needs an OS-protected broker/namespace and independent package approval, a reviewed migration from the installed dev.84 owner protocol, exact three-job Windows CI, and then a fresh, uniquely identified Chat53 Wake with exact receipt. The old ambiguous manual event must not be replayed. No signed image, browser profile, tunnel, installed WakeHost, or live event changed in this increment.
