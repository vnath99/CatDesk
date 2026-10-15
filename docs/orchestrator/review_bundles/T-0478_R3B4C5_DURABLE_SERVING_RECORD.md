# T0478 R3B4-C5 — Durable reviewed-candidate serving record

Date: 2026-10-14. Source-only increment; no installed component or protected tunnel changed.

## Change

The reviewed-candidate launcher now holds the owner-only journal alongside its staged snapshot. After binding the non-inheritable ephemeral loopback listener, it writes a create-only, file-synced `serving.json` before printing ready JSON. The canonical record binds the snapshot ID, intent SHA-256, opened root object identity, new UUID runtime generation, current owner PID, and actual bound loopback port. Startup fails closed if persistence or readback fails. Ready JSON includes the generation and PID.

The reader first revalidates owner-only journal descriptor, intent, manifest, root binding and exact opened root identity, then checks the canonical serving record. The record is forensic evidence; a PID and port alone do not prove a currently live or authorized worker and grant no cleanup authority.

## Verification

- Focused Windows test `owner_journal_survives_pre_root_and_bound_root_crash_windows` passed. It drops/reopens both journal and root handles after serving, verifies the record, rejects a second writer, and rejects changed root identity in `serving.json`.
- Strict Clippy passed. The full local `cargo test --all-features --quiet` exited 0: main binary 1,083 passed/26 ignored, followed by passing integration groups. Rustfmt and diff whitespace checks passed.
- Existing live negative witness canary remains relevant only to the earlier launch gate. With GitHub Actions disabled, a positive exact three-job CI witnessed startup cannot be exercised.

## Open acceptance gates

Persist post-start failure and stop outcomes, retire the independent journal without pathname-based cleanup, add bounded PID/port ownership and cross-process exclusion, restore exact three-job Windows CI, and run a successful witnessed launch/authenticated HTTP/stop cycle. P0 WakeHost installation and canary, P1 Codex review cycle, later multi-project scheduling and T0484 remain open.
