# T-0036-R20 GH Included-Body Account/Scope Compatibility Review Bundle

## Root cause and fix

R19 correctly made the included-header REST account/scope proof the sole
bootstrap authentication authority, but its fixed request also passed
`--silent`. That suppresses the JSON body while the strict proof must read the
exact `vnath99` login. The resulting envelope rejection was fail-closed and
occurred before any bootstrap mutation.

R20 removes only `--silent` from the fixed account request. It still uses
`api user`, `--include`, fixed Accept/API-version headers, and fixed GET.
The strict HTTP-200, exact-login, exact-`repo`-scope, bounded UTF-8, unique
header/status, and fail-closed stderr rules are unchanged. Repository REST
authority remains independent: 404 is ABSENT only after this immediate proof;
200 requires exact policy repository identity.

## Evidence and host sequence

An exact-argv regression asserts `--include` is present and `--silent` is
absent. Existing R19 parser, scope, account, repository, resolver, journal and
publication constraints remain covered by the Cargo suite. No raw response or
credential material leaves CatDesk.

Local verification runs format, strict clippy, Cargo tests, and diff check.
No live GitHub, sibling workspace, browser, daemon, tunnel, Scheduler, release,
or Git publication action was performed.

Host acceptance: isolated reviewed reload, `CONNECTED_VERIFIED`, one fresh
BYOVD preflight, confirmation only on fully positive evidence, exact-origin
proof, then an independent Bug Bounty sequence. Any rejection remains
fail-closed for independent review.
