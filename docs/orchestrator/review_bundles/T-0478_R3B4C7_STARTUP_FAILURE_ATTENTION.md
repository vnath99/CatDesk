# T0478 R3B4-C7 — Pre-serving startup failure attention

Date: 2026-10-14. Source-only reviewed-candidate lifecycle increment; no installed runtime, WakeHost or tunnel changed.

## Change

After the live witness and fixed Git-object stage return an owner-bound snapshot, the launcher now records fixed-code `startup_failed` attention when in-memory app setup, snapshot router creation, listener bind/inspection, or the serving-record write fails. The create-only, file-synced `attention.json` belongs to the existing owner-only journal. Before writing, the snapshot revalidates the manifest, canonical intent, owner-only journal descriptor and exact opened root identity. An attention-write failure appears in the returned launch error.

This record does not authorize scanning by generated name, deleting a candidate root, or accepting an unverified listener. A process crash before the error handler runs can still leave only intent/root evidence.

## Verification

- Focused Windows test `staged_startup_failure_persists_exact_bound_attention_windows` passed. It creates the owner-only journal and root, records startup failure, rejects a second writer, closes the handles, then reopens and verifies the exact owner binding and attention code.
- Strict Clippy passed. Full local `cargo test --all-features --quiet` exited 0: main binary 1,084 passed/26 ignored, and subsequent integration groups passed. Rustfmt and diff whitespace checks passed.
- Positive launch cannot be exercised without a live exact-commit three-job Windows CI witness; the feature branch has no current check runs while workflow dispatch reports Actions disabled.

## Open gates

Persist retirement failure and terminal outcome, safely retire the independent journal, establish bounded PID/port and cross-process ownership, then prove successful witnessed authenticated HTTP and stop/cleanup. P0 WakeHost installation and fresh canary, P1 Codex review cycle, later multi-project scheduling and T0484 remain open.
