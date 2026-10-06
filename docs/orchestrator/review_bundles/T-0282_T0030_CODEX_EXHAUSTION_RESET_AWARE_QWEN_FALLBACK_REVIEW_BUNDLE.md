# T-0282 / T-0030 Codex exhaustion reset-aware Qwen fallback

## Disposition

The controller already contained the same-session local-Qwen handoff, persisted route, handoff artifact, and Codex telemetry boundary fields. The concrete live defect was that `You've hit your usage limit` was not recognized as credit exhaustion and therefore reached generic terminal-error escalation.

This source slice adds that exact normalized diagnostic alongside existing explicit allowance/credit phrases and keeps transient 429 classification separate. It also removes the fabricated 900-second eligibility timestamp from the exhaustion handoff: only previously persisted provider-attested telemetry may set `codex_eligible_after_unix`.

**Blocker: `RESET_TIMESTAMP_PARSE_UNIMPLEMENTED`.** The provider terminal diagnostic text still has no trusted local-date/time parser that converts `try again at 10:54 AM` into durable Unix time. The controller now fails closed by retaining no eligibility boundary and keeping the local route sticky, but this does not fulfill the requested automatic post-reset restoration. Independent review should create one bounded parser/state-integration follow-up rather than infer a timestamp.

## Classifier matrix

| Diagnostic | Result |
| --- | --- |
| `You've hit your usage limit` | `CODEX_CREDITS_EXHAUSTED` |
| `usage limit reached`, explicit credit/plan allowance exhaustion | `CODEX_CREDITS_EXHAUSTED` |
| `HTTP 429 too many requests` | transient Codex rate limit |
| `limit reached` | unclassified / no fallback |
| usage-limit settings/status text | unclassified / no fallback |

The exact existing handoff retains the same session and pending task, persists a Codex-to-Ollama handoff artifact, activates only contract-approved local Qwen, and escalates to ChatGPT when Qwen is unavailable. No cloud, paid, or browser fallback was introduced.

## Verification

| Gate | Result |
| --- | --- |
| Focused Codex exhaustion classifier | PASS |
| Strict clippy | PASS |
| Full routing/controller suite | Not rerun after the final narrow change; blocker recorded before claiming completion |
| `git diff --check` | Pending final independent verification |

## Boundaries

Only `src/delegated/codex_cli.rs`, `src/delegated/autonomous_controller.rs`, and this bundle are attributable to this bounded attempt. No host activation, browser/wake, Secure-MCP/tunnel, signing/provenance, dedicated-producer, Git, or external provider mutation occurred.

Request independent ChatGPT review. Do not start host-live activation or later lifecycle work from this ticket.
