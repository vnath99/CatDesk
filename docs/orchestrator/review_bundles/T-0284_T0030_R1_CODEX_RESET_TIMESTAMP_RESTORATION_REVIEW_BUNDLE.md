# T-0284 / T-0030 R1: Codex reset timestamp restoration

## Scope and disposition

This bounded source ticket closes the T-0282 `RESET_TIMESTAMP_PARSE_UNIMPLEMENTED` blocker. It adds no provider, cloud, browser, host-lifecycle, Secure-MCP/tunnel, signing/provenance, or Git authority. The intentionally dirty worktree was preserved.

## Parser and classifier matrix

`parse_codex_credits_reset_after` first requires the existing explicit `CODEX_CREDITS_EXHAUSTED` classification. It then accepts exactly one case-insensitive `try again at h:mm AM/PM` phrase bounded by non-alphanumeric text. Its trusted input is a host-local Windows `SYSTEMTIME` plus the controller's durable Unix observation instant.

| Input | Result |
| --- | --- |
| `You've hit your usage limit. Try again at 10:54 AM.` | one attested boundary |
| 12:00 AM / 12:00 PM | correct midnight/noon conversion |
| target later today | same-day Unix boundary |
| target at or before current local time | next-local-day rollover |
| HTTP 429, absent phrase, invalid hour/minute, suffix text, multiple phrases | no boundary; fail-closed Qwen stickiness |

No fixed cooldown is fabricated: telemetry with no provider-attested boundary now persists `None` rather than a guessed eligibility time.

## Same-session transition

1. A confirmed Codex exhaustion writes the existing same-session handoff artifact.
2. In the same durable session update it records the parsed boundary and `CODEX_CREDITS_EXHAUSTED` route, then activates only contract-approved local Qwen.
3. Before the persisted boundary, no Codex restoration/probe occurs.
4. At a queued safe task boundary with no durable provider handle, the controller loads the handoff, requires its exact original Codex thread and pending task, and restores only that thread.
5. A fresh exhaustion replaces the prior boundary during the next handoff. Qwen unavailability remains `WAITING_FOR_CHATGPT`; transient 429 continues through its existing Codex backoff path.

The restoration test covers durable session reload state, pre-boundary Qwen stickiness, and exact `codex-cli-session` restoration. A separate re-exhaustion test proves a fresh attested boundary replaces the previous one. The parser and event aggregation reject ambiguity before state mutation.

## Attributable files

- `src/delegated/codex_cli.rs` - bounded phrase parser and trusted Windows local clock.
- `src/delegated/autonomous_controller.rs` - atomic route/boundary handoff and safe-boundary exact-thread restoration.
- `src/delegated/autonomy_state.rs` - removes guessed eligibility fallback.
- This bundle.

## Verification

| Gate | Result |
| --- | --- |
| Focused parser, event-boundary, Qwen/Codex restoration, and state tests | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS after the attributable safe-boundary handle correction |
| `cargo build --all-targets --all-features` | PASS |
| `git diff --check` | PASS |
| Separate project `rust_full` / `verify_project` command | Not configured/discoverable; not inferred |

The test runner emitted its pre-existing non-fatal `could not canonicalize path C:\\Users\\Volap` warning. No host mutation occurred.

## Independent review request

`READY_FOR_INDEPENDENT_REVIEW`. CatDesk/controller checks are not independent acceptance. Request ChatGPT independent final review before any later ticket; do not begin host-live activation or prohibited work from this task.
