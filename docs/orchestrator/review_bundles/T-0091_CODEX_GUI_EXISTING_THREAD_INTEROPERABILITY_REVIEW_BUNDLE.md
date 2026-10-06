# T-0091 Codex Existing-Thread Interoperability Review Bundle

## Implementation

This change adds an operator-confirmable existing-thread adoption path to the central project registry. `autonomy_project_thread_adoption_preflight` obtains candidates only from CatDesk-owned current-user Codex app-server `thread/list`, then re-reads each exact-CWD candidate with `thread/read` and metadata-only `thread/resume`. It returns only a short-lived confirmation token, opaque candidate handle, bounded title, and cwd/thread SHA-256 fingerprints. Raw thread IDs remain internal to the host and durable confirmation record.

`autonomy_project_thread_adoption_confirm` accepts only the prior token and opaque handle. It re-reads the stored candidate ID through the host app-server, requires exact canonical cwd, `gpt-5.6-terra`, `high`, and idle/direct-input-safe ownership, and atomically binds the exact result to the existing project registry entry. Evidence drift, expiry, handle mismatch, duplicate ownership, project/thread conflict, concurrent confirmation, and partial persistence fail closed. Exact post-success replay converges only to the same binding.

Ordinary canonical continuity keeps its existing `sourceKinds: ["exec"]` resolver. Adoption intentionally uses the supported app-server-wide `thread/list` metadata surface without a source-kind assumption, allowing app-server-visible desktop/CLI history without GUI scraping, GUI source-kind invention, private storage, conversation contents, prompts, credentials, or auth/config reads. Candidate metadata is always revalidated before authority is granted.

Projects with no selected candidate retain the existing explicit bounded registration bootstrap. Adoption discovery itself never starts a bootstrap thread, so it cannot silently replace an operator-selected existing conversation. Existing project ChatGPT target fields and project/workspace/thread uniqueness validation remain unchanged.

## Deterministic coverage

Coverage includes metadata-only adoption discovery and Terra/High revalidation; opaque handle selection; no registry mutation at preflight; unknown-handle rejection; expiry; exact replay; no rebinding of an adopted project; duplicate-thread protection; existing project binding compatibility; and prior external-registration/bootstrap, scheduler one-writer, target CAS, and provider thread-drift regressions.

## Live acceptance procedure (CatDesk-owned)

For CatDesk, BYOVD, or Bug Bounty, CatDesk should first register the exact canonical Git workspace through the approved T-0036 flow if it is not already registered. For an existing unbound project, invoke adoption preflight, present only its returned bounded candidates to the operator, and confirm one handle. CatDesk must then execute a normal provider turn and require the provider-reported thread ID to match the durable binding. Any wrong cwd/model/effort, active ownership, stale confirmation, target conflict, or thread drift must remain fail-closed. This worker did not perform a live project registration, app-server probe, GUI operation, browser wake, or external repository mutation.
