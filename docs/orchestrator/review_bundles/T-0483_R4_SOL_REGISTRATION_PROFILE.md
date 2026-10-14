# T-0483 R4 — Exact Sol/medium registration profile

## Scope

Registration preflight accepts optional `modelId`; omission retains the historical `gpt-5.6-terra`/high behavior. The only additional accepted value is `gpt-6.1-sol`, which requires medium effort. The host selects the model before app-server discovery or a read-only bootstrap. Discovery resumes each exact-CWD, idle thread and checks its authoritative model and effort. Confirmation repeats the observation with the model retained in the preflight and compares all evidence before writing the registry.

The operator command accepts `--model-id gpt-6.1-sol` for registration preflight. Arbitrary model identifiers and mismatched effort fail closed. Existing Terra registrations are not silently migrated. A registered project already bound to a different canonical thread still requires an explicit reviewed rotation path before a Sol managed task can run.

## Evidence and limits

- Unit tests cover Sol/medium registration, effort drift, exact-profile app-server discovery, and legacy rejection of a Sol thread.
- Local `cargo test --all-features --quiet` exited 0, including the main 1,087-test binary (1,061 passed, 26 ignored) and integration binaries. A child fixture printed an expected access-denied failure while its parent test passed. `cargo clippy --all-features --all-targets -- -D warnings` exited 0. The focused model-argument test passed after the final argument-validation edit.
- No installed daemon, registered project, live Codex thread, or Wake target was changed by this source patch.
- This is a P1 prerequisite, not evidence of a managed Codex → review_ready → Wake cycle.
- The new source requires an exact Windows CI check and a reviewed install before live use. GitHub Actions dispatch currently responds `422 Actions has been disabled for this repository`, despite the repository actions permissions endpoint reporting enabled.
