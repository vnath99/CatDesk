# T-0036-R13 External Project Git-Gate Diagnostics Compatibility Review Bundle

## Scope and decision

T-0036-R13 narrows the R12 read-only `PROJECT_ORIGIN_WORKSPACE=<absolute workspace>` diagnostic. It does not inspect or mutate BYOVD_DRIVER_PIPELINE, BUG_BOUNTY_RECON_PLATFORM, browser/wake state, Scheduler, daemon, tunnel, or release state.

The accepted registration identity remains `GIT_REMOTE_ORIGIN`: an exact canonical non-reparse Git top-level root plus its exact `remote.origin.url`. Remote-origin registration and its preflight/confirmation revalidation are unchanged.

Originless Git roots and non-Git directories remain intentionally unsupported. An originless path by itself cannot provide the reviewed collision-resistant, replacement-detecting, host-observed repository identity that would need to survive preflight, confirmation, durable serialization, scheduler ownership, and later readback. No caller-supplied path, directory name, or arbitrary content hash is substituted as authority. The operator action is to obtain/configure an approved non-secret `remote.origin.url` outside CatDesk, then rerun the two read-only probes; CatDesk never initializes Git, adds a remote, or guesses an origin. A non-Git directory requires a separately reviewed identity design before it can be registered.

## Bounded diagnostic contract

The origin form always performs fixed host-owned Git metadata commands only. On success it returns `accepted=true`, `identityKind=GIT_REMOTE_ORIGIN`, canonical workspace, and exact Git identity. On rejection it returns only `accepted=false` and one stable `reasonCode`; it never returns stderr, command output, repository contents, credentials, or arbitrary path diagnostics.

| Reason code | Meaning / operator action |
| --- | --- |
| `WORKSPACE_UNBOUNDED` | Supply one bounded workspace value. |
| `WORKSPACE_NOT_ABSOLUTE` | Supply an absolute candidate path. |
| `WORKSPACE_UNAVAILABLE` | Resolve availability without guessing or creating a directory. |
| `WORKSPACE_REPARSE_OR_SYMLINK` | Use the actual non-reparse repository root. |
| `WORKSPACE_CANONICALIZATION_FAILED` | Stop; repair the local path outside this control plane. |
| `NOT_GIT_ROOT` | Candidate is not an exact usable Git root; do not initialize Git here. |
| `NESTED_GIT_ROOT` | Supply the enclosing exact Git top-level, not a nested directory. |
| `ORIGIN_MISSING` | Add/confirm an approved remote origin outside CatDesk, then repeat the probe. |
| `GIT_EXECUTABLE_UNAVAILABLE` | Restore the host Git installation/path and retry the read-only probe. |
| `GIT_COMMAND_FAILED` | The fixed Git top-level command failed after a Git marker was present; stop for host/operator investigation. |
| `GIT_METADATA_OVERSIZED` | Stop; Git metadata exceeded the fixed bound. |
| `GIT_METADATA_NON_UTF8` | Stop; metadata is not supported bounded UTF-8. |
| `GIT_METADATA_INVALID` | Stop; empty, NUL-bearing, or otherwise invalid metadata. |
| `GIT_TOPLEVEL_CANONICALIZATION_FAILED` | Stop; the reported top-level cannot be safely canonicalized. |

Internal code retains the metadata stage (`top-level` or `origin`) while mapping failures to the public code above. This gives deterministic host behavior without exposing diagnostics that could carry repository or credential material.

## Identity/revalidation invariants

For `GIT_REMOTE_ORIGIN`, the probe first rejects unsafe raw paths, canonicalizes the root, requires `git rev-parse --show-toplevel` to canonicalize to the same exact directory, and reads only `git config --get remote.origin.url`. Registration preflight and confirmation run the same checks again and compare the persisted Git identity, canonical workspace, canonical thread ID, model, and reasoning effort under the existing registry lock. Existing project/workspace/thread uniqueness and one-writer-per-workspace leasing therefore remain unchanged.

The App-Server trust boundary remains exact-CWD, `gpt-5.6-terra`, `high`, idle/unowned/direct-input-safe host metadata. Existing eligible threads are preferred; only an explicit zero-candidate result permits the fixed read-only bootstrap. Ambiguous, busy, wrong-CWD/model/effort, stale/expired, duplicate, or thread-drift evidence fails closed.

No local identity mode was added, so no serialization migration, scheduler identity alteration, or path-replacement relaxation is introduced. Historical registry serde, target CAS, R12 cached registration/adoption dispatch, and T-0097/T-0098 target modes remain compatible.

## Deterministic coverage

Local temporary-fixture coverage verifies exact Git root with origin, plain non-Git root (`NOT_GIT_ROOT`), exact Git root missing origin (`ORIGIN_MISSING`), nested Git root (`NESTED_GIT_ROOT`), unavailable-Git reason mapping, command-stage rejection, oversized metadata, non-UTF-8 metadata, and the absence of stderr/detail in the exposed diagnostic response. Existing registry coverage continues to verify registration expiry/drift/replay, duplicate project/workspace/thread conflicts, Terra/High candidate requirements, scheduler writer isolation, historical registry handling, and the R12/T-0097/T-0098 compatibility paths.

## Next host-side sequence

Before any registration write, the host performs exactly two fresh read-only origin probes: one for BYOVD_DRIVER_PIPELINE and one for BUG_BOUNTY_RECON_PLATFORM. It records only each `accepted` result, reason code or canonical root/origin, and no repository content. If either response is not `accepted=true`, stop and apply the stated operator action; do not invoke registration preflight. Only after two successful fresh probes may a separately approved host turn use the returned exact identities in the existing two-phase registration flow.
