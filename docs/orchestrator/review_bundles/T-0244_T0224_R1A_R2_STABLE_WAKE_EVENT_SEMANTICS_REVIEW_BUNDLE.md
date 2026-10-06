# T-0244 stable wake event semantics

`stable_wake_bootstrap` now parses the compatible camelCase review-record
shape: schemaVersion, recordId, projectId, sessionId, state, nextAction,
reference, createdAtUnix and unread. Discovery is bounded, read-only, project
scoped, canonical-root contained, symlink/non-regular rejecting, and classifies
unread COMPLETED_VERIFIED/independent_final_review records as pending; other
valid records are stale. Exact duplicates collapse; semantic duplicate conflict
fails closed. No browser, target, daemon, MCP, release, install, claim/receipt,
tunnel, or live-host mutation is present. T-0243 fixed the absolute-root bug;
R1B retains claim/receipt/browser/install ownership. Existing dirty files are
not attribution evidence; this slice changes only the stable wake module and
this bundle. Full verification remains for independent execution.
