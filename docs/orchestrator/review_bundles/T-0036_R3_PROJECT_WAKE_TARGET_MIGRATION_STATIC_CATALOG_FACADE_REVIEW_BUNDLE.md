# T-0036-R3 Project Wake Target Migration and Static-Catalog Facade Review Bundle

## Delivery-path finding and migration

T-0036-R2 correctly removed global wake fallback. The resulting no-delivery path was an actionable CatDesk review whose `projectId` selected the CatDesk registry record, but that record lacked `chatgptTargetUrl`/digest; `project_wake_target` consequently returned no target and the dispatcher terminated without invoking W13.

This change adds a one-time compatibility migration, not a runtime fallback. When (and only when) resolving the `catdesk` project, CatDesk reads the fixed project-local `.catdesk/wake-bridge/config.json` as bounded JSON, validates its exact approved ChatGPT conversation URL and safe relative profile path, then atomically writes that target and its exact digest only if the matching canonical CatDesk registry project has no target. Existing registry targets are preserved. External projects are never migrated; malformed, symlinked, escaping, oversized, or invalid config produces no target.

The selected durable project target still drives W13 invocation and the schema-4 receipt target digest check. No receipt/global-fallback semantics changed.

## Registration and existing-thread sequencing

External registration now first performs bounded app-server-wide exact-CWD metadata discovery. A single authoritative `gpt-5.6-terra`/`high`, idle, unowned direct-input candidate is adopted for the existing two-phase registration preflight/confirmation. Zero candidates permits the prior fixed read-only host bootstrap. Multiple candidates, active ownership, malformed metadata, wrong CWD/model/effort, or drift fail closed and do not bootstrap a parallel thread. No GUI scrape, source-kind invention, private session/config read, caller thread ID, prompt, executable, or auth input is accepted.

## Local operator facade

`catdesk operator project` now exposes a fixed local catalog only:

- `chat-target bind`
- `registration preflight` / `confirm`
- `thread-adoption preflight` / `confirm`

It delegates to the reviewed supervisor operations with bounded named arguments, returns opaque confirmation tokens/handles and non-secret target fingerprints only, and redacts raw thread IDs. It cannot run commands, choose executables, access authentication/profile/protected state, or invoke browser/tunnel/Scheduler actions.

## Deterministic coverage and live canary

Coverage verifies CatDesk migration success/idempotency, existing-target preservation, unsafe-config rejection, no external-project migration, exact target digest consistency, static facade parsing, adoption/registration atomicity and replay, and existing W13/R7 schema-4 receipt tests.

For CatDesk live acceptance, the host should load the reviewed daemon, verify the durable CatDesk project target digest against the fixed local config, then allow normal automatic dispatch and inspect the resulting schema-4/SENT receipt. For BYOVD or Bug Bounty, use only the local fixed facade: registration preflight first; confirm the single existing candidate if returned, or allow explicit bootstrap only after a zero-candidate result; then bind the project-specific ChatGPT target through CAS. This worker did not perform browser, app-server, external-repository, tunnel, Scheduler, release, or Git publication actions.
