# T-0483 R11 — WakeStore owner challenge and guarded installation source

Date: 2026-10-15. Status: source-only; live dev.85 installation held.

## Source increment

- The installer and WakeHost pin root, `host.lock`, and `versions` object identities, reject reparse/junction redirection, and avoid recursive pathname cleanup on a failed stage.
- While holding its store lease, a protocol-capable WakeHost serves a fresh named-pipe nonce challenge. The native guard verifies the kernel-reported server PID, pinned objects, owner process liveness, reviewed image and current pointer. Split-lock-owner and spoofed-pipe tests reject false ownership.
- Immutable publication and activation require all four full artifact hashes. Activation checks the candidate host image and owner proof before retiring its handoff transaction; failed proof leaves recovery evidence. The installer cannot silently rebind an existing review source. Legacy unguarded install aliases and direct-install examples were retired.

## Verification and release hold

The isolated Wake all-target/all-features tests, strict Clippy, release build and PowerShell installer parsing passed. The full source integration still requires final exact-commit Windows CI. No live installer, fresh Wake, protected binary, browser profile or official tunnel was changed.

`wake/install.ps1 -BuildOnly -RebuildGui` completed locally. The four inert build input hashes were:

| Artifact | SHA-256 |
| --- | --- |
| `CatDeskWakeHost.exe` | `d2e9582ac2b48edee256f243ac9d9ffe3e5ec0bda007c10f9405154e60cdc776` |
| `CatDeskBinagotchy.exe` (built as `catdesk.exe`) | `30033273eeaa63049709adf563e8401a98b753b80f2cbffa688211e03508c86b` |
| `adapter.py` | `f4e01f6e2fb392ae59d2a8ac173587a373a8b5fbb4106250a6e4302ba82ed20d` |
| `wake_bridge.py` | `a1816d2dec83d693300aa5d0fbb6b5c9b2fdfc0bc1b7303d66f1cbe57bdcbc9a` |

These imply an inert candidate directory name `1.0.0-dev.85-d2e9582ac2b4-30033273eeaa`. No staging directory, package publication or activation was attempted.

Installed dev.84 cannot answer the new owner challenge. The live host's root/lock physical objects also differ from a direct-shell open of the displayed path; an isolated installed-style copy reproduced a package-context split. The guard correctly refuses that ambiguous case. The host challenge does not authorize a same-user caller or protect a user-owned WakeRoot from direct writes. [R10](T-0483_R10_WAKE_CALLER_APPROVAL_BOUNDARY_HOLD.md) records the required protected broker, worker identity and signed-approval migration. Do not activate dev.85 until those boundaries and exact CI are proven. Do not replay the historical ambiguous Chat53 event.
