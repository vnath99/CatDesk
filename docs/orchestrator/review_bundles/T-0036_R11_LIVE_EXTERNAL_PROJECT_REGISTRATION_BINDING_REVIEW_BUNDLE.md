# T-0036-R11 Live External Project Registration and Binding Review Bundle

## Scope and reviewed decisions

This is a design-only procedure. T-0099/R10 is accepted in the CatDesk-local plan and uses the canonical CatDesk project/thread; CatDesk's accepted target digest is `aa334a335780a3895f1426c32a66e2a3407eac17ded4ad308482d1e80d53379c`.

Intended external bindings are distinct:

| Project | Exact workspace candidate | Approved ChatGPT target |
| --- | --- | --- |
| `BYOVD_DRIVER_PIPELINE` | `<USER_PROFILE>\OneDrive\Desktop\Projects\BYOVD_DRIVER_PIPELINE` | `https://chatgpt.com/g/g-p-6a7631815f2c81918713e6630b55e62d/c/69e7a205-23c4-83ea-a93a-38344c7f9ff7` |
| `BUG_BOUNTY_RECON_PLATFORM` | `<USER_PROFILE>\OneDrive\Desktop\Projects\BUG_BOUNTY_RECON_PLATFORM` | `https://chatgpt.com/g/g-p-6a7631815f2c81918713e6630b55e62d/c/6a763063-adcc-83ea-9400-ea5689b5c910` |

No sibling repository was opened for this review. The two target URLs must remain distinct, canonical project targets; neither may route through CatDesk or a global fallback.

## Reviewed controls and exact fields

`autonomy_project_registry_preflight` accepts `projectId`, `workspace`, `gitIdentity`, and `verificationProfile`. It returns bounded `confirmationToken`, `confirmationFingerprint`, expiry, canonical workspace, verified Git identity, canonical thread ID, model, and reasoning effort. `autonomy_project_registry_confirm` accepts only the token and re-reads Git/thread evidence before atomically persisting project, workspace, Git identity, profile, and thread.

`autonomy_project_thread_adoption_preflight` accepts registered-unbound `projectId` and exact `workspace`; it returns a short-lived token plus opaque candidate handles, bounded title, fingerprints, model, effort, and expiry. `autonomy_project_thread_adoption_confirm` accepts only token plus handle, re-reads the internal candidate ID, and returns the bound project/workspace/thread. This control is for an already registered project with no thread; it is not a replacement for a completed registration confirmation that already atomically bound a thread.

Target CAS is `autonomy_project_registry_bind` target mode: either the reviewed new shape (`projectId`, `conversationUrl`, optional `expectedCurrentTargetSha256`) or the cached connector compatibility shape (`projectId`, `decision=CHAT_TARGET_URL=<exact canonical URL>`, `expectedSha256=<current digest>`). Both delegate to the same validator/CAS and return only project ID plus target digest. They reject all workspace/thread/Git/profile fields and mixed shapes.

## Authoritative origin gate

The reviewed preflight does **not** discover an origin from a project or directory name. It requires `gitIdentity` as input, then its host-owned bounded Git path verifies it by running only:

1. `git -C <candidate> rev-parse --show-toplevel`, requiring that canonical result equal the candidate's canonical path; and
2. `git -C <candidate> config --get remote.origin.url`, requiring exact equality with `gitIdentity`.

It rejects missing/oversized metadata, raw symlink/reparse input, canonicalization failure, nested-root mismatch, and origin mismatch. Because the exact external origins are not provided by this ticket and no sibling repository may be inspected, the narrow pre-mutation gate is: CatDesk host obtains each bounded `remote.origin.url` using the preceding host-owned command, presents/records it as non-secret origin evidence, and uses that exact returned value as `gitIdentity` in the immediately following preflight. If that host resolution/confirmation channel is unavailable, stop before every registry mutation; do not guess an origin.

## Existing-thread-first policy

For each exact canonical root, host app-server metadata must prove one and only one candidate with exact CWD, `gpt-5.6-terra`, `high` reasoning, idle/direct-input-safe unowned status. One project owns one workspace and one thread; one thread belongs to one project. The registration preflight already observes the existing exact-CWD candidate first. A fixed read-only no-tools bootstrap is eligible only after an explicit zero-candidate result. Ambiguity, active ownership, wrong CWD/model/effort, candidate drift, stale/expired confirmation, duplicate workspace/thread, or later provider-reported thread drift fails closed.

## Host execution order

Perform the following independently for BYOVD then Bug Bounty; no operation may reuse the other project's workspace, thread, token, target, or digest.

1. Resolve the external origin through the authoritative host gate above and canonicalize the exact root.
2. Run registration preflight with project ID, canonical workspace candidate, exact host-resolved origin, and the approved verification profile. Require one eligible existing thread or explicit zero-candidate bootstrap; reject every other result.
3. Confirm the returned registration token before expiry. Require readback equality for project ID, canonical workspace, origin, profile, canonical thread, Terra/High, and unowned direct-input evidence. This is the normal atomic registration/thread path.
4. Only if a project was independently and validly persisted unbound (not after step 3), use adoption preflight, obtain one opaque candidate handle, and confirm it before expiry. Re-read exact CWD/Terra/High/idle evidence and require atomic registry binding. Do not bootstrap or replace an operator-selected candidate during adoption.
5. Read the central registry. Verify the two projects have distinct canonical workspaces and distinct thread IDs, neither equals CatDesk, and neither currently has an unintended target.
6. Bind the project-specific approved ChatGPT target with target CAS, then read the full central registry again. Require exact canonical URL/digest per project and no CatDesk/global fallback.

## Target-CAS unresolved gate

For a newly registered project with no target, T-0098's cached connector shape deliberately requires `expectedSha256` and therefore cannot initialize a null target. The reviewed local operator facade can invoke the new `conversationUrl` CAS shape, but this ticket records that the local executable is not a live connector path. Therefore before step 6 CatDesk needs one approved host/operator path capable of the reviewed new target shape with an absent expected digest, or an approved CAS seed procedure. Until that exists, stop after registration/readback; do not substitute CatDesk's target, omit the required digest from cached mode, or invent a target update.

## Failure handling, reconciliation, and idempotency

Every preflight failure leaves registry unchanged. Confirmation revalidates origin/thread evidence under the registry lock; exact completed replay converges, while expiry, concurrent confirmation, stale evidence, duplicate project/workspace/thread, or partial-write error fails closed. Target CAS mismatch leaves target unchanged. Provider thread drift blocks future work and never overwrites the durable binding. A failed post-write readback is an operator-attention reconciliation state: re-read the full registry, accept only an exact known-good idempotent binding, otherwise stop. Never delete, reset, or roll back a preexisting valid project/thread/target merely to recover an uncertain partial operation.

## T-0037 bounded scheduler acceptance

After both bindings are independently accepted, use read-only/no-op or separately approved temporary artifacts only. Demonstrate that the global scheduler grants simultaneous work for the two distinct canonical workspaces within provider limits, rejects a second writer for either same workspace, preserves each canonical thread on a no-op provider turn, and keeps reviews/wakes selected by the respective durable `projectId`/target. Do not mutate BYOVD or Bug Bounty source. Record distinct per-project accounting and receipt-target isolation; CatDesk and either external project must never share a routing fallback.

## Evidence checklist and independent review

Required non-secret evidence: canonical roots, exact verified origins, verification profile, preflight/confirmation tokens and expiry outcomes, opaque adoption handle/fingerprints if applicable, Terra/High/idle ownership result, registry readbacks, target digests, scheduler decisions, and project-scoped receipt identities. Do not record conversation content, prompts, credentials, profile contents, or raw browser state.

This review created no live binding. CatDesk should independently verify this procedure and approve the unresolved origin-resolution and null-target-CAS gates before any host mutation.
