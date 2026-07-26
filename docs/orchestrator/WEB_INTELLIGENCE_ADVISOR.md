# Web Intelligence Advisor

Status: T-0024C.2 boundary-hardening review
Date: 2026-07-26

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

T-0024C.2 implements only the automatic failure trigger:

- at least two failed bounded repair attempts;

Explicit structured Qwen advice requests and `NEEDS_SUPERVISOR` advisory
consultation remain deferred until a real structured mechanism and supervisor
policy are implemented. CatDesk does not expose model-visible advisor tools.

## Historical Script Review

`powerapps.py` was reviewed only as historical inspiration. It uses raw
Selenium and `undetected_chromedriver`, hard-coded account/session behavior,
and brittle selectors. None of that behavior is copied.

`deepseek_v2.py` was treated only as risk context. Future work must not copy
plaintext credentials, CAPTCHA automation, hard-coded account or conversation
data, generated CSS selectors, or infinite keep-alive behavior.

## T-0024B Guardrails

The standalone DeepSeek browser advisor adapter must:

- prefer SeleniumBase CDP mode;
- use a dedicated persistent browser profile;
- require manual login;
- return `TAKEOVER_REQUIRED` for CAPTCHA or security verification;
- communicate through a narrow authenticated local protocol;
- expose no repository or execution tools.

T-0024B remains the standalone browser adapter boundary. T-0024C connects it
to CatDesk only through the Rust `DeepSeekProcessAdvisor` JSONL process
adapter.

## T-0024C Guardrails

T-0024C integrates advisor consultation into the worker loop, but only as
untrusted context. It does not modify the released CatDesk worker, patch,
journal, MCP, authentication, provider, or verification boundaries.

The web advisor is disabled by default. The durable contract policy must enable
advisor ID `deepseek-web` with `REMOTE_ALLOWED` disclosure. Local Python,
script, selector, and persistent-profile paths are local CatDesk runtime
configuration supplied outside the execution contract and outside MCP request
arguments.

The MCP surface exposes only `advisorPolicy`. It does not expose
`advisorLocalConfig`, executable paths, adapter script paths, profile paths,
selector paths, headed/headless mode, or credential-login behavior. The trusted
DeepSeek runtime is loaded from operator-local CatDesk startup environment:

- `CATDESK_DEEPSEEK_ADVISOR_PYTHON`;
- `CATDESK_DEEPSEEK_ADVISOR_SCRIPT`;
- `CATDESK_DEEPSEEK_ADVISOR_PROFILE`;
- optional `CATDESK_DEEPSEEK_ADVISOR_SELECTORS`;
- optional `CATDESK_DEEPSEEK_ADVISOR_HEADED` defaulting to `true`;
- optional `CATDESK_DEEPSEEK_ADVISOR_ALLOW_ENV_LOGIN` defaulting to `false`.

Configured executable, script, selector, and profile paths are validated and
canonicalized locally. New and rehydrated runs use this same operator-local
runtime configuration; runtime paths are never restored from the execution
contract or journal. If no local runtime is configured, the advisor remains
unavailable. Optional advice continues locally; required advice escalates to
`NEEDS_SUPERVISOR`.

The Python process receives only a bounded `AdviceRequestV1` over an
authenticated local JSONL protocol and receives no CatDesk tool definitions or
execution authority. Returned advice is delimited as untrusted user/context
data for the next Qwen turn; Qwen must still inspect repository state and act
through ordinary CatDesk tools.

Ordinary journal advisor events contain structural metadata only. Bounded
request and response details are stored as local advice artifacts and referred
to by artifact reference.

Cancellation is handled inside the process-adapter owner while waiting for the
sidecar terminal frame. CatDesk cancellation sends a sidecar `cancel` command
without waiting for a mutex held by the advice request, joins the consultation
task, suppresses later advice delivery, and emits one terminal advisor event
for the active generation.
