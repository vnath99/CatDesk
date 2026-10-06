# T-0036 R2 project-scoped ChatGPT wake routing review bundle

## Implementation

The central project registry is migration-compatible: each existing project
continues to deserialize without a chat target, while a project may now carry
an optional canonical ChatGPT target URL and its non-secret SHA-256 digest.
Project/workspace/thread uniqueness is unchanged. The new
`autonomy_project_registry_chat_target_bind` operation applies a bounded
compare-and-swap against the current target digest and persists URL plus digest
together.

Canonical target validation accepts only HTTPS `chatgpt.com` or
`chat.openai.com` exact conversation URLs in either `/c/<conversation-id>` or
`/g/<project-id>/c/<conversation-id>` form. Query strings, fragments,
userinfo, ports, host drift, malformed IDs, trailing/extra path segments, and
other routes fail closed. The Rust MCP/runtime and the W13 bridge use the same
forms.

Automatic wake now resolves the actionable review's durable `projectId` in the
central registry. It requires the selected project target and matching digest;
there is no fallback to the global CatDesk config target. Missing/invalid
project target returns terminal without launching the bridge, leaving the
review actionable. The selected target is passed ephemerally to the bridge and
schema-4/receipt-1 validation compares receipt `target_sha256` against that
exact selected target, preserving W13 one-submit and R7 retry boundaries.

## Coverage and live procedure

Tests cover optional-field migration, target CAS, direct and project-style URL
positives/negatives, receipt exact-target validation, existing registry
conflict guards, and the MCP tool inventory. The full Rust suite retains the
existing wake one-writer/retry/receipt coverage.

After independent review, CatDesk should bind only a verified unambiguous
project target using the returned digest, then prove a review for each project
can wake only its own conversation and rejects a cross-project receipt. The
current CatDesk target remains unchanged. No Bug Bounty or driver target was
persisted here; in particular the ambiguous identical driver target was not
bound. No browser, tunnel, scheduler, release, external repository, or Git
publication action occurred.

## Local verification

`cargo fmt -- --check`, strict clippy, and `git diff --check` passed. Full
Rust verification reported 521 passed, 18 ignored, and 0 failed. CatDesk owns
independent review and all live binding/acceptance actions.
