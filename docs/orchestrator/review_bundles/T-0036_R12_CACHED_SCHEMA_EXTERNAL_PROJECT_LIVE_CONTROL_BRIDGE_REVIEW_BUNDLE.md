# T-0036-R12 Cached-Schema External Project Live-Control Bridge Review Bundle

## Scope

T-0036-R12 extends only the already exposed `autonomy_project_registry_bind` dispatcher for connectors that retain the cached argument surface. It creates no new registry, shell channel, executable/auth input, browser/wake operation, or direct registration shortcut. No live external workspace was probed or registered by this implementation.

Legacy direct thread binding, T-0097 `conversationUrl` target CAS, and T-0098 `decision=CHAT_TARGET_URL=<url>` plus `expectedSha256` target CAS remain separate, unchanged paths.

## Exact cached forms

Every `decision` is bounded, has no surrounding whitespace, and must exactly match one form. Each form has a closed top-level-field allowlist; unexpected, mixed, or authority-bearing fields reject.

| Form | Required cached fields | Delegated reviewed operation | Bounded result |
| --- | --- | --- | --- |
| `PROJECT_ORIGIN_WORKSPACE=<absolute workspace>` | `decision` only | exact-root bounded Git metadata probe | canonical workspace and exact `gitIdentity` |
| `PROJECT_REGISTRATION_PREFLIGHT_WORKSPACE=<absolute workspace>` | `projectId`, `decision`, `gitIdentity`, `verificationProfile` | two-phase registration preflight | token, fingerprint, expiry, canonical workspace/origin, Terra/High thread evidence |
| `PROJECT_REGISTRATION_CONFIRM` | `decision`, `confirmationToken`, optional `projectId` cross-check | registration confirmation | project/workspace/origin/profile/thread evidence |
| `PROJECT_THREAD_ADOPTION_PREFLIGHT_WORKSPACE=<absolute workspace>` | `projectId`, `decision` | existing-thread adoption preflight | token, expiry, opaque candidate handles and bounded fingerprints/metadata |
| `PROJECT_THREAD_ADOPTION_CONFIRM` | `decision`, `confirmationToken`, `candidateHandle`, optional `projectId` cross-check | existing-thread adoption confirmation | project/workspace/thread evidence |
| `CHAT_TARGET_INIT_URL=<exact URL>` | `projectId`, `decision` | synchronized one-time null-target initialization | project ID and target digest |

The adoption forms are intentionally retained only for a project that is already persisted with no thread. Normal registration preflight/confirmation observes an existing exact-CWD thread first and atomically persists it; zero-candidate bootstrap is still host-owned and eligible only under the reviewed registration policy. No cached form accepts a raw caller thread ID as adoption authority.

## Security and consistency invariants

The origin probe requires an absolute existing non-reparse/symlink input, canonicalizes it, requires `git rev-parse --show-toplevel` to canonically equal that exact root, and obtains `remote.origin.url` through the same bounded Git-only helper used by registration. It persists no candidate repository or registry state. The registration preflight then compares its separate `gitIdentity` input against a fresh use of that helper; a project/directory name never supplies an origin.

Registration confirmation continues to re-read Git and authoritative App-Server metadata before the reviewed atomic registry commit. Expired, stale, conflicting, duplicate, or replayed confirmation behavior is therefore unchanged. App-Server evidence still requires exact canonical CWD, `gpt-5.6-terra`, `high` reasoning, and idle/unowned direct-input-safe ownership; ambiguity, wrong CWD/model/effort, busy ownership, workspace/thread conflicts, or provider drift fail closed.

`CHAT_TARGET_INIT_URL` runs under the central project-registry lock. It requires an already registered project whose URL and digest are both absent, canonicalizes only approved exact ChatGPT conversation paths, and writes the canonical URL/digest exactly once. Any existing target, even an equal one, rejects. Query strings, fragments, userinfo, malformed paths, an expected digest, or every unrelated field reject. It creates no CatDesk/global fallback.

## Deterministic coverage

Focused local tests cover legacy compatibility, the prior cached target CAS, exact closed-mode parsing, null-target initialization, second-initialization rejection, malformed/query target rejection, whitespace and mixed-field rejection, raw-thread-field rejection in adoption confirmation, exact Git root/origin success, nested-root rejection, relative-path rejection, origin mismatch, and proof that the origin probe creates no registry state. Existing registry tests continue to cover registration/adoption expiry, stale evidence, replay, project/workspace/thread uniqueness, Terra/High evidence, and scheduler one-writer rules.

## Host-side live sequence

For each external project independently, the CatDesk host must first invoke the read-only origin form and record its returned canonical root and exact origin. It then supplies that exact origin to registration preflight with the selected project ID and reviewed verification profile, confirms the short-lived returned token, and reads the central registry. Use adoption preflight/confirm only if that readback shows a valid already-persisted unbound project; otherwise do not create a parallel adoption path.

After exact project/workspace/origin/thread readback, the host initializes the previously null project target once with `CHAT_TARGET_INIT_URL=<approved exact project conversation URL>`, then reads the registry again. BYOVD and Bug Bounty must have distinct roots, origins, threads, and target digests; neither may fall back to CatDesk or a global target. Any preflight or confirmation failure leaves the registry unchanged. On post-write readback ambiguity, stop and reconcile by read-only exact evidence; never destructively remove a pre-existing valid binding.

The next T-0037 acceptance must use only read-only/no-op or separately bounded artifacts to demonstrate cross-project concurrency, same-workspace writer refusal, canonical thread reuse, and project-scoped review/wake isolation. It must not mutate either external repository.
