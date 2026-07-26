# DeepSeek Web Advisor Adapter

Status: T-0024B mutation-observer workflow correction
Date: 2026-07-26

## Purpose

The DeepSeek web advisor adapter is an experimental browser bridge for the
post-v1 intelligence-advisor phase. It is advisory only. It accepts a bounded
`AdviceRequestV1` and returns an `AdviceResponseV1`-shaped result.

The adapter is not connected to the CatDesk `AdvisorBroker` in T-0024B. That
integration remains deferred to T-0024C.

## Boundary

The adapter must not receive or expose:

- CatDesk tool definitions;
- direct filesystem, shell, Git, patch, verification, job, or MCP authority;
- repository-wide content;
- credentials, cookies, tokens, or session secrets;
- login automation;
- CAPTCHA, Turnstile, or security-verification bypass behavior.

## Browser Posture

The live browser path is opt-in and headed. It uses SeleniumBase CDP mode via
`SB(...).activate_cdp_mode(...)` with a dedicated persistent Chrome profile
directory supplied by the operator.

The preferred login path remains a persistent browser session with manual
login. T-0024B.3 also adds an explicit `--allow-env-login` option that reads
`CATDESK_ADVISOR_DEEPSEEK_EMAIL` and `CATDESK_ADVISOR_DEEPSEEK_PASSWORD` only
from the process environment and uses configured login selectors. Credentials
are removed from the adapter process environment immediately after reading,
are not printed or persisted, and are sent only to the DeepSeek login form when
that opt-in path is used.

CAPTCHA, Turnstile, or security verification returns `TAKEOVER_REQUIRED`; the
adapter does not click or bypass those flows.

Startup failures transition to `DEGRADED` rather than remaining in
`STARTING`. The CLI defaults to headed operation; an `--unsafe-headless-dev`
switch exists only for local development and is not used by CatDesk.

The trusted browser origin is independently fixed to
`https://chat.deepseek.com`. Selector configuration may choose a path on that
origin, but it cannot redefine the site that receives advisory data.

Cookie-banner handling is selector-only and origin-gated. The adapter clicks a
configured reject-non-essential selector before any configured accept selector,
and never clicks generic or unknown banner elements.

## Protocol

T-0024B uses an authenticated JSON Lines protocol over stdin/stdout:

- `hello`;
- `status`;
- `start`;
- `advise`;
- `cancel`;
- `shutdown`.

The authentication token is supplied by `CATDESK_ADVISOR_AUTH_TOKEN`. It is not
printed, stored, or sent to the browser page.

Stdout is reserved for JSONL protocol frames. Browser/framework output is
redirected to stderr around adapter operations.

`advise` runs in one managed worker thread so the JSONL control loop remains
responsive. Only one advisory request may be active. `status`, `cancel`, and
`shutdown` remain available while advice is running, and completion is emitted
as a correlated `advice_completed` JSONL event.

T-0024B.3 accepts advice only when `refresh_state()` is `READY`. Requests are
rejected while another request is active or while the adapter is
`RATE_LIMITED`, `CANCELLED`, `DEGRADED`, `TAKEOVER_REQUIRED`, or
`LOGIN_REQUIRED`. Cancellation uses the active request ID, returns exactly one
terminal cancellation result for that generation, and suppresses later
completion, failure, or rate-limit events from the worker.

## Advisory Turn

The `advise` command:

- validates `AdviceRequestV1` with `schemaVersion: 1`;
- requires `disclosureClassification: REMOTE_ALLOWED`;
- rejects prohibited authority keys after normalized-key comparison;
- rejects unsupported extra operational instructions;
- verifies the fixed DeepSeek origin before typing;
- builds a bounded advisory prompt;
- records the existing assistant-response baseline;
- in the live browser workflow, requires the visible
  `.ds-virtual-list-visible-items` root before typing and captures direct-turn
  baseline keys, assistant count, latest assistant key, and latest assistant
  hash;
- installs one root `MutationObserver` before submission and, after associating
  the new assistant turn, one assistant-content observer on
  `.ds-markdown.ds-assistant-message-main-content`;
- selects the prompt control from external selector config;
- clears the prompt control before insertion and fails rather than appending to
  retained draft text;
- inserts text through paced input-event delivery with bounded randomized
  intervals, longer newline pauses, a total typing timeout, and cancellation
  checks;
- submits once through the exact composer-local send control and requires a
  new user turn, new assistant turn, exact stop control, or root-observed
  generation activity before waiting for a response;
- watches only the assistant final-answer container associated with the active
  generation;
- tracks actual text changes, SHA-256 hashes, mutation counts, and text
  stability;
- requires the generation/stop control to be absent, send to be visible and
  enabled, and multiple stable samples;
- returns exactly the newest bounded assistant final-answer `innerText` in
  `diagnosis`;
- rejects old, empty, baseline, or stale responses;
- returns `LOGIN_REQUIRED`, `TAKEOVER_REQUIRED`, `RATE_LIMITED`, `TIMED_OUT`,
  `CANCELLED`, `DEGRADED`, or `FAILED` without attempting bypasses.

Browser-operation exceptions produce bounded `FAILED` responses and do not
leave the adapter stuck in `SENDING` or `WAITING_FOR_RESPONSE`.

## Completion Detection

Completion does not depend on a generated CSS class. The detector combines:

- visible assistant final-answer baseline and newest-response text;
- stop/generation-control absence;
- visible and enabled send control;
- actual text changes and SHA-256 hash stability across multiple samples;
- timeout.

For the mutation-observer live path, completion requires a new/current
assistant final-answer container associated with the active generation,
non-empty normalized text, a post-submission text or DOM change, no exact stop
control, exact send visible and enabled, no assistant-content mutation for at
least five seconds, and at least three consecutive matching hashes.

Rate limits are detected only through visible provider error/toast selectors,
not arbitrary matching of page text.

Selector configuration lives in
`experimental/advisors/deepseek_selectors.json`, outside the core adapter
logic.

## Diagnostics

Ordinary diagnostics include only:

- current state;
- failed selector;
- page title with secret-like values redacted;
- URL origin only;
- bounded DOM tag/attribute evidence.

Diagnostics omit cookies, tokens, prompts, responses, credentials, and full
page URLs.

## Testing

T-0024B includes deterministic offline tests using synthetic HTML fixtures.
Live DeepSeek testing remains opt-in and headed.

The isolated setup file is
`experimental/advisors/requirements-deepseek-advisor.txt`. It is not part of
the released CatDesk dependencies.

## T-0024B.2 Live Smoke

An opt-in headed smoke was run with a dedicated `.tmp` Chrome profile and a
synthetic `AdviceRequestV1`. The adapter reached `READY`, accepted the
advisory request, emitted a correlated `advice_completed` event with bounded
`RATE_LIMITED` status, accepted a second synthetic request, returned
`cancel` with `ok: true`, and shut down cleanly. No CatDesk tool definitions,
repository content, credentials, cookies, or execution authority were supplied
to the browser advisor.

## T-0024B.3 Live Smoke

T-0024B.3 requires a headed synthetic smoke against a persistent/manual-login
session. If DeepSeek reports a visible provider rate limit, the adapter must
return `RATE_LIMITED` and stop rather than retry aggressively.

Observed local result for this correction pass: the adapter reached `READY`,
accepted the first synthetic request, rejected a too-early second request with
`ADVICE_ALREADY_ACTIVE`, and shut down cleanly. The first request timed out with
no visible assistant response in the conversation body, so the cancel leg was
not run. The adapter did not fabricate a completion from stale, empty, or
baseline page content.

## T-0024B.4 Live Evidence

T-0024B.4 calibrated the live page with redacted structural metadata only:
element tag, role, aria label, data-testid, stable class tokens, bounded
ancestor structure, visibility/enabled state, geometry, and text lengths where
needed. It did not capture prompts, responses, cookies, tokens, credentials, or
page HTML.

Observed local result: the adapter reached `READY`, cookie banner state was
`absent`, the configured composer was `textarea.ds-scroll-area`, the configured
send control became enabled after paced input-event insertion, and a synthetic
request was accepted. The terminal advisory result remained `TIMED_OUT`: after
the prompt was emptied, no visible assistant response, user-message block,
stop/generation control, or provider rate-limit/error selector was available
for extraction. The adapter therefore kept the result fail-closed and did not
return stale page content or the submitted prompt as advice.

## Mutation-Observer Live Evidence

The mutation-observer correction adds a per-generation tracker with a single
active request, direct-turn baseline keys, root and assistant observers,
response hashing, virtual-list replacement handling by turn key, exact
composer-local send checks, and final-answer extraction from
`.ds-markdown.ds-assistant-message-main-content`. Reasoning content
`.ds-think-content`, action rows, whole-conversation parents, old assistant
turns, and submitted user prompts are rejected.

Observed local result for the required headed proof: the adapter reached
`READY` on `https://chat.deepseek.com`, cookie banner state was `absent`, and
the exact composer `textarea[placeholder="Message DeepSeek"]` was visible.
Before typing, the required `.ds-virtual-list-visible-items` root was absent
(`rootCount: 0`, `visibleRootCount: 0`). The adapter therefore returned
`DEGRADED` before submitting the prompt, preserved the browser session, and
captured only redacted structural metadata. No additional broad selectors or
speculative heuristics were added.
