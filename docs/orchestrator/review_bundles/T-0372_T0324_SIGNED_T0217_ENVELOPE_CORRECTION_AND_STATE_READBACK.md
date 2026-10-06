# T-0372 / T-0324 — Signed T-0217 Envelope Correction + Protected Rotation-State Readback

Status: **READ-ONLY HOST READBACK REQUIRED**  
Date: 2026-09-10

## Trigger

T-0371 crossed the reviewed UAC boundary and failed closed while reconciling the fixed ProgramData rotation envelope. The observed object was:

- length: `519`
- SHA-256: `50194cf9613e8b9b541db597045fb2fc73a70511cbe90efcf1b512b19ce02644`

No Program Files image replacement or serving restart was performed.

## Corrected historical classification

The 519-byte object is not unknown and is not the 422-byte unsigned T-0217 signing payload. The exact historical T-0217 signing payload is 422 bytes with SHA-256 `eaad9c085257a9fa459d2f4771f69c360dda6930b120778c2efeaaa5d2e8a308`. Combining those canonical bytes with the historical T-0217 detached Ed25519 signature produces a canonical envelope of exactly 519 bytes and SHA-256 `50194cf9613e8b9b541db597045fb2fc73a70511cbe90efcf1b512b19ce02644`. The detached signature verifies against the compiled CatDesk product root `3bc6a2101687816dc8235efde562db93dbd0ad822d922286640ec5c985239545`.

Therefore the host object is positively identified as the **signed T-0217 epoch-2 rotation envelope**. Earlier project notes saying T-0217 remained unsigned are superseded by this evidence. The signed envelope was at least staged in the fixed ProgramData transport.

## Why this changes T-0366 epoch handling

T-0366 remains the exact reviewed executable candidate (`d09c677f8ce5f14b3600a5435929aff31b4128d9a041cdaceae3213b5f0af36f`, 26302464 bytes). Its currently signed envelope also uses epoch 2.

The reviewed rotation implementation rejects an incoming epoch that is not strictly newer than the authenticated predecessor and also rejects a pending envelope that differs from the current incoming envelope. Therefore T-0366 epoch 2 must not be forced into consumption until the protected Program Files rotation state is known.

Two materially different safe branches exist:

1. **No T-0217 pending/installed rotation receipt exists.** The signed T-0217 envelope is only stale incoming transport. T-0366 epoch 2 can remain valid after exact transport reconciliation.
2. **A T-0217 pending or installed rotation receipt exists.** T-0366 epoch 2 conflicts with already-consumed or crash-recovery epoch-2 authority. The protected state must not be deleted or overwritten. The correct recovery may require finishing the historical pending transaction first and then re-designating the same T-0366 executable under a fresh monotonic epoch.

## Current transport side effect from T-0371

T-0371 had already authenticated and archived the exact historical T-0217 incoming executable (`552cb917998d283e7d901593ffed5a9ceaa108223654aa69349fa7e567c21482`, 25172480 bytes) and staged the exact T-0366 executable before it encountered the signed-envelope mismatch. The incoming transport may therefore currently be mixed: T-0366 executable plus T-0217 signed envelope. This remains transport only, not accepted authority, and is deliberately left unchanged pending readback.

## T-0372 read-only inspector

Added `scripts/t0372_inspect_main_image_rotation_state.ps1`. It reads only these fixed locations:

- installed `C:\Program Files\CatDesk\CatDesk.exe`
- accepted bootstrap envelope
- pending rotation envelope
- installed rotation envelope
- rotation staging executable
- fixed incoming executable
- fixed incoming envelope

For each it records only existence, length, SHA-256, known-artifact classification, and non-secret canonical envelope metadata. Output is written to `.catdesk/t0372-rotation-state.json`.

The legacy CatDesk shell refused execution under allowlist mode (`SHELL_MODE_BLOCKED`). This accepted boundary must not be bypassed by switching to unrestricted shell. A normal local PowerShell run is therefore the remaining host-readback action; no UAC should be required.

## Safety state

Secure MCP remains `CONNECTED_VERIFIED`, local MCP `READY`, externally owned official runtime preserved, legacy 77-tool generation still serving. Delegated run list is empty. No Program Files/ProgramData mutation after the T-0371 failure, no serving restart/recovery, no pending/installed receipt deletion, no browser wake, no protected wake-target edit, no tunnel replacement, no Git publication, and no worktree cleanup were performed.
