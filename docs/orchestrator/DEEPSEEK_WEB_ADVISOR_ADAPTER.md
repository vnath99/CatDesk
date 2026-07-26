# DeepSeek Web Advisor Adapter

Status: T-0024B.1 experimental standalone proof
Date: 2026-07-25

## Purpose

The DeepSeek web advisor adapter is an experimental browser bridge for the
post-v1 intelligence-advisor phase. It is advisory only. It accepts a bounded
`AdviceRequestV1` and returns an `AdviceResponseV1`-shaped result.

The adapter is not connected to the CatDesk `AdvisorBroker` in T-0024B.1. That
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

The user performs login manually. CAPTCHA or security verification must return
`TAKEOVER_REQUIRED`; the adapter must not click or bypass those flows.

Startup failures transition to `DEGRADED` rather than remaining in
`STARTING`. The CLI defaults to headed operation; an `--unsafe-headless-dev`
switch exists only for local development and is not used by CatDesk.

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

## Advisory Turn

The `advise` command:

- validates `AdviceRequestV1` with `schemaVersion: 1`;
- rejects prohibited authority keys after normalized-key comparison;
- rejects unsupported extra operational instructions;
- verifies the exact configured DeepSeek origin before typing;
- builds a bounded advisory prompt;
- records the existing assistant-response baseline;
- selects the prompt control from external selector config;
- submits once;
- watches only the newest response after the baseline;
- tracks actual text changes and text stability;
- returns a bounded `AdviceResponseV1` with the newest response in `diagnosis`;
- returns `LOGIN_REQUIRED`, `TAKEOVER_REQUIRED`, `RATE_LIMITED`, `TIMED_OUT`,
  `CANCELLED`, `DEGRADED`, or `FAILED` without attempting bypasses.

## Completion Detection

Completion does not depend on a generated CSS class. The detector combines:

- assistant response-container count;
- stop/send button visibility;
- text stability interval;
- provider-specific completion markers from selector config;
- timeout.

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
