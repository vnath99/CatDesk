# T-0483 R1 — WakeHost dev.85 release readiness review

**Date:** 2026-10-14
**Scope:** source and build assessment only. Installation and live Wake acceptance remain open.
**Source branch/HEAD reviewed:** `orchestrator/chatgpt-codex-autonomous-loop` / `f4fae1aca47aba24c725b9db76ea7a049cd6d818`.
**Model identity:** this Codex Desktop session does not expose an authoritative effective model/effort readback to this review. This document is not evidence that the requested GPT-6.1 Sol/medium managed route ran.

## Source and CI evidence

- `09c0456` adds bounded read-only reconciliation for a `SUBMIT_TARGET_DRIFT_CHROME_ERROR` while the *same* event is already `SUBMITTING`. The Python bridge reopens only the exact target on the same CDP tab and reproves the exact USER turn. It has no second Send path in that recovery branch. Tests cover transient error, persistent error, foreign conversation, and exact receipt reproof.
- `7df27ee` immediately inspects a successful `cdp.get()` return before the remaining receipt deadline. It versions the candidate to `1.0.0-dev.85` in `wake/Cargo.toml`, `wake/Cargo.lock`, and root `Cargo.lock`.
- [Windows CI for `7df27ee`](https://github.com/vnath99/CatDesk/actions/runs/38443116318), [Windows CI for `b0a664e`](https://github.com/vnath99/CatDesk/actions/runs/38641559086), and [Windows CI for exact reviewed HEAD `f4fae1a`](https://github.com/vnath99/CatDesk/actions/runs/38642577662) each finished SUCCESS in Rust, Python Wake/advisor, and Independent WakeHost Rust.
- Local `cargo test --locked --offline --manifest-path wake/Cargo.toml --all-features` passed. The all-features flag is required because tests call `Store::open_scoped_for_test`, which is gated by `test-support`; plain `cargo test` fails compilation and is not a runtime regression. `wake/install.ps1 -BuildOnly -RebuildGui` built both release executables (GUI build 6m 16s) without activating them. Focused `pytest.exe tests/test_wake_bridge.py tests/test_stable_wake_browser_adapter.py -q` exited 0 with one skip. Root and Wake `cargo fmt --check`, Wake strict all-target/all-features Clippy, and `git diff --check` passed.

## Package identity and installed divergence

The source candidate is **not installed**. The reviewed-install-status interface on the installed binary reports current `1.0.0-dev.84`, and the running process is that immutable dev.84 executable. No dev.85 version directory existed at review start. Final local candidate hashes after the GUI rebuild:

| Artifact | Candidate SHA-256 | Installed dev.84 SHA-256 |
| --- | --- | --- |
| `CatDeskWakeHost.exe` | `6a0db30a85e35c455fa54502e0f48ec40b42fd1b65d3065a168411041664cc39` | `02431feba5ed73a5d40208b535d80cdc85c9a1c239058faf029f0311e51e89a2` |
| `adapter.py` | `f4e01f6e2fb392ae59d2a8ac173587a373a8b5fbb4106250a6e4302ba82ed20d` | same |
| `wake_bridge.py` | `a1816d2dec83d693300aa5d0fbb6b5c9b2fdfc0bc1b7303d66f1cbe57bdcbc9a` | `aa4c055990fabc2cea811cc9514bfbfb3e77085e6dbf7a3bab2fb88914e2697c` |
| `CatDeskBinagotchy.exe` | `a1f7466dc470708cab68e6ccc011c4f2bd62cb5c5f008e17fb46492d8f4072c0` | `1c6bcdfcedf02cc41868db3b883fc0d2fc5b65d5cccbe205734541c2ed81d3b2` |

`wake/install.ps1` prepares a unique inert staging directory and manifest with four SHA-256 values, calls Rust `publish-reviewed-install`, then `activate-reviewed-install` under the exclusive install lease. That path must be used for any approved activation. The old installed directory is the last-known-good rollback candidate; no active binary replacement or Secure MCP tunnel change is part of this review.

## Live-state and preflight hold

CatDesk's Binagotchy MCP `status` reported dev.84 RUNNING, last event `manual-wake-mcp-1791908511726` in `SUBMITTING`, `SUBMIT_TARGET_DRIFT_CHROME_ERROR`, and no receipt for that event. Its last receipt belongs to an older generation-33 event. The ambiguous generation-34 event must not be replayed. A direct invocation of the installed dev.84 binary's `status` returned an old dev.59/STOPPED/generation-1 record, while its `reviewed-install-status` and the process image both identify installed dev.84. This status-store discrepancy is unresolved; determine the authoritative runtime root and paired target through supported read-only interfaces before activation or new wake publication.

**Decision: HOLD activation and canary.** Reconcile the status-store/target authority discrepancy and obtain the protected reviewed-install approval required by the handoff. Then run only `wake/install.ps1` through the reviewed publication/activation path. Verify installed manifest hashes and target Chat53 generation 34 before exactly one *fresh* diagnostic event. Require that event's `EXACT_USER_MESSAGE_APPENDED`, `SENT`, timer `COMPLETE`, no duplicate USER, and browser cleanup. A diagnostic canary does not prove a natural `review_ready` cycle.
