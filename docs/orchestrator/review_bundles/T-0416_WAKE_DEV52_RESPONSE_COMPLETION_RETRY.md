# T-0416 dev.52 response-completion retry review

## Classification

`DEV52_RESPONSE_COMPLETION_REVIEW_BLOCKED`

## Evidence inspected

- `scripts/wake_bridge.py` contains the bounded response-completion path and
  fixed exact timeout text `Message delivery timed out. Please try again.`.
  Its dedicated tests cover generation-active to idle completion, same-target
  reopen, associated Retry, completion after reopen, sequence drift,
  ambiguous retry controls, retry exhaustion, and browser-close ordering.
- `wake/adapter.py` keeps durable `EXACT_USER_MESSAGE_APPENDED` receipt
  handling distinct from assistant-response observation. Its reconciliation
  path states that it does not type, press Enter, or click USER Send.
- `wake/src/runtime.rs` accepts the response stages, including bounded
  `GENERATION_ACTIVE` heartbeat and response retry/completion stages, while
  retaining the accepted pre-submit-only `CHATGPT_NOT_IDLE` behavior.
- `cargo test --manifest-path wake/Cargo.toml --features test-support --no-run`
  completed successfully after scoped formatting. It did not execute a live
  browser, target mutation, installed WakeHost, or Wake send.

## Blocking finding

The cancelled turn-lease experiment remains textually present in the dirty
workspace behind disabled conditional compilation. The current contract
requires every TurnLease/TurnPhase/continuation-watchdog addition, including
status, CLI, and tests, to be deleted rather than hidden. This artifact does
not treat disabled residue as an acceptable repair. No live Wake, browser,
target, tunnel, recovery, Git, or external-project action was taken.

The required full Wake/release/root verification is therefore not represented
as green. Independent review must reject this snapshot until the cancelled
source is completely removed and the bounded dev.52 test and verification
profiles are rerun.
