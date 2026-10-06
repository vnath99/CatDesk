# T-0036-R22 GH 404 STDERR UTF-8 Hardening Review Bundle

R22 narrows only the R21 accepted repository-404 diagnostic path. A 404 after
the strict same-selected-gh account/scope proof may coexist with the normal
nonzero CLI result and stderr, but diagnostic bytes must be within the existing
bound, valid UTF-8, and NUL-free. Their text is never compared or used as
repository authority.

Account/scope remains process-successful with empty stderr and exact HTTP 200,
`vnath99`, and `repo` scope. Repository 200 remains success/empty-stderr plus
exact identity; all non-404 ambiguity fails closed. R14-R21 policy, workspace,
resolver, journal, origin, branch, and no-publication rules are unchanged.

Tests cover accepted bounded UTF-8 diagnostics and non-UTF8, NUL, and oversized
rejection. Local verification runs format, strict clippy, Cargo tests, and diff
check. No live GitHub, external workspace, browser, daemon, tunnel, Scheduler,
release, remote, push, or Git publication action was performed.

Host acceptance: reviewed isolated build/reload, `CONNECTED_VERIFIED`, one
fresh BYOVD preflight, confirmation only on positive evidence, exact-origin
proof, then an independent Bug Bounty sequence.
