# T-0304 Codex Usage-Limit / Qwen Fallback Live-Shape Review Bundle

## Scope and reproduction

This is a repository-only repair of the provider-routing defect exposed while
T-0303 was running. It does not resume T-0303, launch a browser, invoke wake,
modify a target, reload a daemon, activate a supervisor, or change tunnel,
host, external-project, or Git state.

The bounded live sequence was:

1. Codex app-server/CLI normalization emitted the explicit, redacted provider
   diagnostic `You've hit your usage limit ... try again at Sep 2nd, 2026
   12:50 AM`.
2. The provider turn then ended with a generic terminal error.
3. The previous controller predicate inspected only `TerminalError.text` for
   the exhaustion phrase. It therefore persisted a bounded terminal diagnostic
   and escalated `provider_terminal_error` instead of taking the already
   approved same-session Qwen handoff.

The fault was correlation at the runtime/controller seam, not a broad failure
classification problem: the conservative Codex classifier already recognized
`You've hit your usage limit`, but the useful text was on the preceding
normalized event rather than the terminal error.

## Repair

`codex_credit_exhaustion_diagnostic_from_events` now accepts only a terminal
Codex batch containing both:

- one Codex `TerminalError`, and
- exactly one Codex text independently classified as
  `CreditsExhausted` by the existing bounded classifier.

It then passes that one attested text to the existing handoff transaction. A
generic terminal error alone remains a ChatGPT escalation. A transient 429 is
not a credit diagnostic and remains the existing bounded Codex rate-limit
backoff. Multiple exhaustion texts remain ambiguous and fail closed.

The reset parser still runs only after positive exhaustion classification. In
addition to its existing time-only grammar, it now strictly accepts one
provider-local explicit date form such as `Sep 2nd, 2026 12:50 AM`; the month,
calendar day, ordinal, and a future local instant must all validate against the
trusted host-local observation. Missing, malformed, duplicate, ambiguous, or
past explicit dates produce no eligibility boundary. In that case Qwen stays
sticky and Codex is never reprobed merely because a cooldown was guessed.

## Provider and continuity semantics

| Input/result | Required behavior |
| --- | --- |
| Exact usage-limit diagnostic followed by generic Codex terminal error | Persist the existing same-logical-task handoff and route once to contract-approved local Qwen. |
| One valid future reset timestamp | Persist `codex_eligible_after_unix`; retain Qwen before it; restore only the durable canonical Codex thread at a safe task boundary at/after it. |
| Missing/malformed/multiple/past reset timestamp | Keep Qwen sticky with no invented Codex retry time. |
| `HTTP 429` / rate-limit terminal error | Preserve Codex bounded backoff; do not hand off to Qwen as credits exhaustion. |
| Generic terminal error | Preserve `WAITING_FOR_CHATGPT` escalation; do not classify it as credits exhaustion. |
| Local Qwen unavailable | Preserve the existing fail-closed ChatGPT escalation; no cloud, paid, or browser fallback. |

The durable handoff, rather than the active Qwen session field, retains the
canonical Codex thread. Existing safe-boundary restore and re-exhaustion tests
remain the authority for one-time handoff, no pre-reset replay, refreshed
boundaries, and exact thread restoration.

## Deterministic regressions

- `live_usage_diagnostic_then_generic_terminal_hands_off_once_to_qwen` uses
  the exact diagnostic concept without an upgrade URL. It proves one Codex
  launch, one local-Qwen continuation, same task, and preserved handoff thread.
- `reset_boundary_is_extracted_only_from_one_positive_exhaustion_diagnostic`
  proves a preceding text diagnostic plus generic terminal event is recognized,
  while a generic terminal error and a transient 429 are not.
- `credit_exhaustion_reset_parser_handles_12_hour_edges_and_next_day_rollover`
  now includes the explicit `Sep 2nd, 2026 12:50 AM` shape.
- `credit_exhaustion_reset_parser_rejects_unattested_malformed_or_ambiguous_phrases`
  includes invalid calendar/ordinal and duplicate dated phrases.
- Existing classifier, Qwen handoff, sticky-before-reset, exact-thread restore,
  refreshed-boundary, Qwen-unavailable, and rate-limit tests remain in place.

All diagnostics stay within the existing redaction and byte-bound limits. The
new correlation examines normalized in-memory events only; it creates no
browser, target, path, shell, cloud-provider, or alternate-routing authority.

## T-0303 and critical-path state

ChatGPT independently reran T-0303 focused acceptance-readiness checks and
they passed. T-0303 remains paused pending independent review of this isolated
provider repair; this ticket does not continue its T-0152 work. The core order
remains `T-0223 -> T-0222/T-0139 -> T-0152 -> T-0155`, with T-0223 still
`OPERATOR_BOOTSTRAP_REQUIRED`.

Safe resume after independent T-0304 review: continue the same logical T-0303
session/checkpoint through the contract-approved local Qwen fallback if Codex
is still ineligible; do not create a replacement task or replay Codex before
its durable boundary. No host-live work is authorized by that resume.

## Attributable files

- `src/delegated/autonomous_controller.rs`
- `src/delegated/codex_cli.rs`
- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- This bundle

No unrelated dirty-worktree changes were staged, reset, cleaned, reverted, or
attributed to T-0304.

## Verification

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| Focused Codex parser/classifier/controller routing tests | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --workspace --all-targets --all-features --no-fail-fast` | PASS (887 tests; configured ignored tests remained ignored) |
| `cargo build --workspace --all-targets --all-features` | PASS |
| `git diff --check` | PASS |

One bounded repair cycle corrected the test's initial same-day/later-time
"past" fixture to the genuinely past `Aug 27th` date. The final full command
above passed after that test-only correction; no further repair cycle ran.

The test runner retained its pre-existing non-fatal `could not canonicalize
path C:\\Users\\Volap` warning. No verification command performed a live
browser, wake, target, tunnel, daemon, supervisor, or Git-publication action.

## T-0304-R2 release-profile finalization

R1's escalation is `WAITING_FOR_CHATGPT` with
`repair_budget_exhausted` after two repair attempts. R2 therefore reviewed the
same candidate and made **no additional product-source change**. The known
T-0304 product scope remains limited to
`src/delegated/autonomous_controller.rs` and `src/delegated/codex_cli.rs`;
the milestone/current-plan entries and this bundle are documentation only.
Unrelated dirty-worktree changes remain unaltered and unattributed.

The required locked release profile was run with a 600-second timeout:

| Command | Result | Classification |
| --- | --- | --- |
| `cargo build --release --locked` | Did not complete: `failed to remove file ...\\target\\release\\catdesk.exe` — `Access is denied. (os error 5)` | Fail closed: host file-lock/access condition, not an attributable T-0304 provider-routing/compiler defect. |

An initial invocation was interrupted while its Cargo build-directory lock was
held; a fresh invocation then reached `Compiling catdesk` and returned the
access-denied result above. Neither result supplies a successful release-build
attestation. R2 does not alter product source to work around a live
`target\\release\\catdesk.exe` lock, does not terminate or replace the process
holding it, and does not use a detached build, raw executable, shell workaround,
or deployment action.

Read-only R2 process evidence identifies the lock boundary: `catdesk.exe` was
running from that exact `target\\release\\catdesk.exe` path (PID `36836`, started
2026-08-25). This observation changes no host state and is not a license to
stop, reload, or replace the process.

Consequently T-0304 is **not independently finalizable yet**. The R1
live-shape correlation, strict explicit reset-date parser, generic-terminal
and 429 negative cases, same-task Qwen handoff, canonical Codex-thread
continuity, pre-reset stickiness, and no-cloud/paid/browser-fallback policy
remain the reviewed candidate evidence; the outstanding prerequisite is only a
clean successful `cargo build --release --locked` after the host releases the
file lock. Host, tunnel, browser, wake, target, and supervisor state were not
changed by R2.

The exact safe T-0303 resume remains unchanged after that independent review
and a successful release profile: continue the same logical T-0303 checkpoint
through the contract-approved local Qwen path if Codex remains ineligible; do
not allocate a replacement task or replay Codex before its durable boundary.

## Independent review request

`RELEASE_BUILD_HOST_LOCK_BLOCKER`: independent review should verify the
batch-level live-shape correlation, strict explicit-date parser, generic
terminal/429 negative cases, provider-policy invariants, unchanged
host/tunnel/browser state, and the exact T-0303 resume step; final acceptance
remains blocked until the locked release profile passes without the host file
lock.

## T-0304-R3 isolated release verification finalization

R3 reconfirmed the R1/R2 attribution: the only T-0304 product changes are
`src/delegated/autonomous_controller.rs` and `src/delegated/codex_cli.rs`; R2
made no product-source change. The default locked release profile remained
blocked only because its output name is the live
`target\\release\\catdesk.exe`, which Windows refused to replace. The live
process and its default release path were not terminated, suspended, reloaded,
copied over, renamed, or otherwise disturbed.

The exact contract-fixed isolated build passed:

| Command | Result |
| --- | --- |
| `cargo build --release --locked --target-dir .catdesk/verification-targets/t0304-r3` | PASS — release profile completed in 5m25s. |

`.catdesk/verification-targets/t0304-r3` is workspace-contained disposable
verification output. It is release-equivalent compile/link evidence for this
same source candidate only: it is not a reviewed image, producer attestation,
promotion authority, deployable runtime, or authority to change the live
daemon/default release executable.

Read-only post-build evidence: the isolated artifact is
`.catdesk/verification-targets/t0304-r3/release/catdesk.exe` (26,119,168 bytes,
2026-09-02 06:50:10); the default `target/release/catdesk.exe` remains
23,779,328 bytes with its prior 2026-08-16 02:16:43 timestamp; and the same
live default-path `catdesk.exe` PID `36836` remains running since 2026-08-25.
These observations prove the verification output did not replace the live
binary and did not mutate its process.

R3 additionally passed:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test --workspace --all-targets --all-features credit_exhaustion_reset_parser` | PASS (2 parser regressions) |
| `cargo test --workspace --all-targets --all-features reset_boundary_is_extracted_only_from_one_positive_exhaustion_diagnostic` | PASS |
| `cargo test --workspace --all-targets --all-features live_usage_diagnostic_then_generic_terminal_hands_off_once_to_qwen` | PASS |
| `git diff --check` | PASS |

The existing `could not canonicalize path C:\\Users\\Volap` warning remained
non-fatal. No R3 source repair was needed; no browser, wake, target, tunnel,
daemon, supervisor, or default-release executable was mutated. The only R3
attributable changes are this bundle and the minimal milestone/current-plan
status reconciliation; all other dirty-worktree changes remain unattributed.

T-0304 is ready for independent acceptance. Only after that acceptance may
ChatGPT resume the existing T-0303 checkpoint: retain the same logical task,
continue on contract-approved local Qwen while Codex is ineligible, and never
replay Codex before its durable reset boundary. R3 neither resumes nor creates
T-0303.

## R3 independent review request

`READY_FOR_INDEPENDENT_REVIEW`: verify that the isolated target build is
release-equivalent compile/link evidence only; the live/default release
executable remained untouched; the usage-limit batch correlation and explicit
reset-date parser preserve generic-terminal/429 fail-closed distinctions and
the same-task Qwen policy; no unrelated dirty changes are attributed; and the
listed T-0303 resume step remains exact.
