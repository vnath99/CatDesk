# T-0036-R21 GH HTTP Status/Process Reconciliation Review Bundle

R21 sets `GH_NO_UPDATE_NOTIFIER=1` only for the fixed trusted `gh` child while
preserving inherited authentication. No credential or environment value is
read or exposed.

The account/scope proof remains strict and process-first: successful child,
empty stderr, one bounded HTTP envelope, HTTP 200, exact `vnath99`, and exact
`repo` scope. Account and repository process failures have separate bounded
reason codes.

For the fixed policy repository GET only, CatDesk parses one included HTTP
envelope before process/stderr classification. After a successful same-gh scope
proof, one exact HTTP 404 is ABSENT even if `gh` reports its normal nonzero
HTTP-error result. A 200 still requires success, empty stderr, exact owner/name,
and valid opaque ID. Other status/framing/identity ambiguity fails closed.

R14-R20 workspace, resolver, fingerprint, policy, journal, origin, branch,
and no-publication restrictions remain unchanged. Local verification includes
format, strict clippy, Cargo tests, and diff check. No live external, GitHub,
browser, daemon, tunnel, Scheduler, release, or Git publication action occurred.

Host acceptance: isolated reviewed build/reload; `CONNECTED_VERIFIED`; exactly
one fresh BYOVD preflight; stop on any rejection; confirm only on positive
evidence; exact-origin proof; then separately repeat Bug Bounty.
