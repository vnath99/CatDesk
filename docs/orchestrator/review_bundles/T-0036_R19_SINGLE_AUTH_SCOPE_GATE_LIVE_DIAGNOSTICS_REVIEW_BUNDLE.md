# T-0036-R19 Single Auth/Scope Gate Review Bundle

## Authority flow

R19 removes the obsolete production `gh api user --jq .login` call from
bootstrap preflight and confirmation. The one included-header REST account/scope
proof is now the sole authentication authority immediately before each
repository-state decision. It requires HTTP 200, exact `vnath99` login, and an
exact `repo` OAuth scope; repository 404 remains insufficient without it.

Raw stdout, stderr, headers, and bodies stay internal. Bounded non-secret
reason codes distinguish gh process failure, HTTP authentication failure, wrong
account, missing/unproven repo scope, malformed HTTP envelope, and repository
state probe failure. Nonempty stderr is fail-closed.

R14-R18 mappings, canonical workspace, deterministic trusted tool slots,
fingerprint drift checks, journal recovery, exact private repository/origin,
and current safe-branch push restrictions are unchanged.

## Verification and host sequence

Local verification runs format, strict clippy, Cargo tests, and diff check.
No live GitHub, external workspace, credential store, browser, daemon, tunnel,
Scheduler, release, or Git publication action was performed.

Host acceptance: isolated reviewed reload; `CONNECTED_VERIFIED`; exactly one
fresh BYOVD preflight; confirm only on positive evidence; exact-origin proof;
then an independent Bug Bounty sequence. Any reason code stops fail closed and
requires independent review.
