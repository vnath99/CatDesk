# Web Intelligence Advisor

Status: T-0024A experimental architecture
Date: 2026-07-25

## Purpose

The post-v1 advisor phase lets the local Qwen worker request bounded advice
from a stronger web-accessible model during difficult work. The advisor is
not a worker, provider replacement, fallback provider, or execution authority.

The advisor receives only a redacted, bounded `AdviceRequestV1` and returns an
untrusted `AdviceResponseV1`. Qwen must independently inspect repository state
through ordinary CatDesk tools before acting on or rejecting the advice.

## Boundary

The advisor must never receive:

- CatDesk tool definitions;
- direct filesystem, shell, Git, patch, verification, job, or MCP authority;
- the complete repository;
- unredacted secrets or credentials;
- login automation instructions or CAPTCHA/security bypass behavior.

## T-0024A Scope

T-0024A implements only deterministic local primitives:

- versioned request/response schemas;
- advisor statuses;
- `AdvisorAdapter` trait;
- deterministic `FakeAdvisor`;
- `AdvisorBroker`;
- disclosure enforcement;
- redaction and size caps;
- request/response journaling;
- untrusted-context handoff back to Qwen;
- tests proving no direct advisor tool authority.

No browser is launched, no website is contacted, and no SeleniumBase dependency
is introduced in T-0024A.

## Advisor Triggers

Only these triggers are valid:

- explicit Qwen request for advice;
- at least two failed bounded repair attempts;
- `NEEDS_SUPERVISOR` when advisory consultation is explicitly allowed.

## Historical Script Review

`powerapps.py` was reviewed only as historical inspiration. It uses raw
Selenium and `undetected_chromedriver`, hard-coded account/session behavior,
and brittle selectors. None of that behavior is copied.

`deepseek_v2.py` was treated only as risk context. Future work must not copy
plaintext credentials, CAPTCHA automation, hard-coded account or conversation
data, generated CSS selectors, or infinite keep-alive behavior.

## Future T-0024B Guardrails

A real browser advisor adapter must:

- prefer SeleniumBase CDP mode;
- use a dedicated persistent browser profile;
- require manual login;
- return `TAKEOVER_REQUIRED` for CAPTCHA or security verification;
- communicate through a narrow authenticated local protocol;
- expose no repository or execution tools.

## Future T-0024C Guardrails

T-0024C may integrate advisor consultation into the worker loop, but only as
untrusted context. It must not modify the released CatDesk worker, patch,
journal, MCP, authentication, provider, or verification boundaries.
