# T-0036-R23 Exact GH 404 Live-Shape Regression Review Bundle

R23 adds test-only coverage; production repository authority code did not
change. `repository_state_accepts_real_404_shape_only_after_scope_proof` first
models the strict included HTTP-200 `vnath99` plus `repo` scope response, then
models `success=false`, one included HTTP-404 envelope, and bounded UTF-8
diagnostic stderr. ABSENT succeeds. A different bounded diagnostic also
succeeds, proving diagnostic text is not authority.

The same test rejects non-UTF8 and NUL stderr and a 200 repository response
with stderr. Existing R22 coverage rejects oversized diagnostics and malformed
HTTP envelopes. Account scope, canonical workspace, trusted slot/path/SHA,
two-project policy, journal, exact origin, and no-publication invariants remain
unchanged.

Local verification runs format, strict clippy, full Cargo tests, and diff check.
No live GitHub, sibling workspace, browser, daemon, tunnel, Scheduler, release,
remote, push, or Git publication action occurred.

Host acceptance: isolated reviewed build/reload, `CONNECTED_VERIFIED`, exactly
one fresh BYOVD preflight, confirm only on positive evidence, exact-origin
proof, then independently repeat Bug Bounty.
