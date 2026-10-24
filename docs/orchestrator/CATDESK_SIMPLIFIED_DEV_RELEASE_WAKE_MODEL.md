## October 24, 2026 — focused simplification and compatibility proposal

This section supersedes the historical deployment plan below. The operator has
requested a smaller release model. Protected installation and recovery actions
remain separately authorized; this proposal grants no runtime activation.

### Consolidate existing components incrementally

| Existing component | Proposed disposition | Required compatibility condition |
| --- | --- | --- |
| Git branch history, exact source/build/test/review evidence | Keep GitHub as the source and development record. Bind immutable artifact manifests to exact commits and full hashes. | A commit or CI result alone does not authenticate unknown executable bytes. Preserve unverified CI labels. |
| Development `versions/`, package manifests, `current.json`, workspace authority | Reuse this functioning nonprivileged runtime. Consolidate package-selection metadata behind its existing transaction, with current/previous verified release as the target model. | Existing schema-1 selections, profile, Python runtime, targets and journals must remain readable. No production root or tunnel access. |
| `package-update-attempt.json` and staged immutable package | Reuse one selection/recovery journal. Remove dependence on the original build directory when completing an already validated staged update. | Revalidate exact workspace, package, pointer, authority and exclusive STOPPED ownership; unknown state remains held. First implementation below. |
| `user_worker_release.rs` current/previous immutable manifests and atomic active state | Reuse this existing current/previous representation for the eventual stable worker cutover; do not create another competing version registry. | Keep existing protected admission intact until the composed startup, rollback and migration tests pass. |
| Legacy canonical executable/sidecar and alternating LKG slots | Retain as compatibility inputs; stop treating them as the development runtime's release store. Retire their mutating role only after migration to a verified versioned release passes. | Preserve legacy bytes and receipts. `operational_verified` must not be relabeled as independently reviewed. |
| Reviewed build, promotion and October 18 release-pair procedure | Consolidate operator presentation into one admitted-release transaction. Reuse existing review/build validators and the bounded reconstruction route where needed. | Do not introduce a parallel trust framework or bypass existing protected policy. The current divergent production pair still needs approved authority. |
| Product-root signed installation / pending rotation | Keep only for the protected launcher/install boundary where required. Exclude it from ordinary source work, Wake URL updates and development-package selection. | Preserve existing signed artifacts and receipts; no rotation or protected cutover is authorized here. |
| Independent Wake, guarded paired URL binding, native Codex and external Secure MCP | Keep their existing independent ownership and lifecycle. | No implicit Wake launch, native writer takeover, tunnel replacement or chat replay during release selection. |

### Minimal end state and staged migration

A release manifest should identify exact source commit, artifact hashes, tests,
independent review and (separately) observed working health. Keep immutable current
and one independently verified working predecessor. Reuse the existing atomic
selection and journal; a folder's presence, newest timestamp or prior selection
alone must not make it trusted rollback material. Public startup should choose a
verified current release, or restore the verified predecessor after a bounded
failure. If neither is eligible, return one clear reconstruction-required result.

Reconstruction must bind one explicit operator-approved source/build candidate
and preserve the divergent old pair and escrow. Use the existing reviewed-build /
release-pair procedures; do not rewrite a fingerprint or require a newly invented
signing/authority mechanism. Existing protected requirements apply only where the
chosen production admission actually requires them.

Migration order:

1. Make the existing development selection recoverable from local immutable
   artifacts without any privileged promotion or retained build-output directory.
2. Add commit/review/health qualification and explicit current/previous selection
   to the existing versioned model. Preserve older schema readers. Verify corrupt
   current, valid predecessor, absent/corrupt predecessor, interrupted selection,
   cold restart and normal startup before automatic fallback is enabled.
3. Qualify the composed normal developer backend startup and rollback independently
   of protected production. Retain bounded ownership and approved-resource scope.
4. On an authorized production maintenance turn, admit a qualified version through
   the existing reconstruction/cutover route. Prove process identity, protocol,
   bindings and recovery. Only then retire redundant legacy writers; retain
   historical readers and evidence for compatibility.

No legacy mechanism is retired by the first increment. The October 27 historical
plan remains the production compatibility path until a migration is qualified.
No recurring process is enabled. Actual two-conversation Web coding remains the
next live priority once the approved serving path is restored.

### First independently eligible implementation

`operator web-dev-runtime recover-package` takes no path, version or hash input.
It finishes only an existing package-update journal against the already staged
immutable package, using the same completion validator as `update-package`.
This removes the unnecessary original-build-directory dependency and creates no
second LKG store, new approval type or production authority. It never starts a
host or browser. With no pending update, it validates the current stopped
selection without changing its bytes. Unknown pointers, changed package bytes,
active owners or incompatible state remain explicit refusals.

This increment passed 17 development-runtime and 12 operator tests, both strict lint
gates and formatting. It is source/fixture qualification, not current/previous
health fallback, installed recovery or normal unattended acceptance. The existing
selected development package and protected runtime are unchanged.

# T-0476 — CatDesk fast development, safe Wake recovery and simple production releases

## CURRENT 2026-10-11 — Chat52 three-lane development and recovery status (supersedes history below)

- **Wake continuity lane:** the guarded paired CatDesk project registry and independent WakeHost both identify Chat52 `https://chatgpt.com/c/6acba3bf-e740-8331-8896-8a25fefdbf16`, SHA `d82593bf8a7eb82936cd63d1ed52f77243c46afa3f9aa57ca2d9a642a31b73ce`, independent generation33. WakeHost dev84 RUNNING/queue0. Manual test `manual-wake-mcp-1791732042693` was terminal SENT/EXACT_USER_MESSAGE_APPENDED/COMPLETE; later test `manual-wake-mcp-1791737671996` reached visible Chat52 and now has an exact message-appended receipt, but check its terminal timer/delivery after response. **Manual != natural Wake acceptance.** All CatDesk hourly deadmen are disabled at user request; do not recreate without user instruction.
- **Source-only development lane:** T0478 R2 authenticated temp-root read/search fixture, R3A handle-pinned no-follow single-link and in-memory regex, R3B1 config hardlink/reparse preflight, R3B2 entirely in-memory fixture configuration accepted by GitHub CI `38157083333` 3/3 success. R3B3 source commit `04806d8` retains no-delete-share directory handle for full disposable fixture listener lifetime, real local HTTP negative rename/release test passes; its independent GitHub CI `38158118618` was still running when recorded. R3B4 **design only** `docs/orchestrator/review_bundles/T-0478_R3B4_SOURCE_SNAPSHOT_PROVENANCE_DESIGN.md` defines immutable Git-commit-derived source manifests, content-hash checking and negative integrity tests. Still **NOT** an autonomous coding agent, signed installation or production worker.
- **Protected production/recovery lane:** operational supervisor root `SUPERVISOR_ROOT_UNAVAILABLE`, startup policy `SUPERVISOR_STARTUP_POLICY_UNPROVEN`, startup definition read failed, port3201 unprobed. The serving binary still rejects some new typed recovery commands. Signed installed T0215 epoch1 and authentic pending T0366 epoch2 must remain untouched until reviewed protected release/LKG authority exists. Official Secure MCP runtime has external ownership, never instantiate a duplicate. No private signer online.
- Next: verify R3B3 GitHub CI, implement R3B4 trusted source snapshot and bounded owner PID/port stop/restart, then T0479 versioned stable worker/LKG. This supersedes older Chat48/Chat51 target and hourly-fallback statements in this architecture file; those paragraphs are historical only.

## 2026-10-11 actual live progress (supersedes historical states below)

- T0477 split Chat48 registry / Chat51 independent Wake was successfully reconciled via source-current guarded paired recovery. Both authoritative Chat51 URL/digest match; independent Wake target generation32. No signed-image rotation was needed.
- WakeHost dev84 is RUNNING; exactly one manual generation32 event `manual-wake-mcp-1791691944140` ended `SENT`, `EXACT_USER_MESSAGE_APPENDED`, timer `COMPLETE`, queue0. This is confirmed **manual**, not natural automatic Wake acceptance.
- The **production Recovery** milestone still has unverified stable supervisor startup definition/policy and unavailable supervisor root; old serving controller cannot execute the current typed nine-layer diagnose via MCP. It must not be marked COMPLETE.
- T0478 starts the parallel **nonprivileged source-only development loop** `scripts/catdesk-dev-lane.ps1 status|build|verify`. It never starts a daemon/browser or installs a production executable; `isolatedRuntimeReady=false` until R2 proves state/port/tunnel separation and safely enables a development worker.
- Retain all protected signing/rollback boundaries for actual production installs. Do not serialize ordinary source engineering on pending T0366 epoch2 rotation.



Date: 2026-10-10 local / 2026-10-11 UTC. Canonical human/hourly Chat51:
https://chatgpt.com/c/6acaae13-4b18-83e9-b6ca-3c5e00cf47a8.

Status: **ARCHITECTURE PIVOT APPROVED BY USER IN CHAT / FIRST SOURCE SAFETY PATCH IMPLEMENTED**.
This document is a forward design and specific acceptance plan, not authority to
change protected Program Files state or run browser Wake events.

## The architectural diagnosis

CatDesk currently treats six logically distinct operations as one serial
deployment gate: editing source -> Windows verification -> independently reviewed
build -> product-root signed immutable Program Files image -> installed
version/provenance and protected promotion -> serving daemon / WakeHost event
delivery and target rebinding. Signed main-image rotation is itself stuck:
T-0215 epoch 1 is installed and trusted, T-0366 epoch 2 is cryptographically
signed and persisted as pending with an intact incoming image, incoming signed
envelope and staging image, but no installed rotation receipt. The historical
T-0373 attempt returned a broad INSTALL_FAILED. T-0474 ordinary (nonadmin)
Win32 probes returned read-open0 for all three images, DELETE ACCESS 5 for the
two Program Files objects, and DELETE ACCESS 0 for ProgramData incoming.
This proves non-admin Program Files delete rights are absent (expected), *not*
that the elevated MoveFileExW would fail. In-place swapping a possibly running
executable is unnecessarily fragile; hard-coded production keys/epochs should
not block routine local development.

A **separate**, proven obstacle is that the live CatDesk control daemon is
older than current source, and its guarded current-chat target rollover rejected
the current Chat51 request. Independent WakeHost is STOPPED. The freshest operator-run independent
WakeHost readback now shows **Chat51 generation32**, but the CatDesk project
registry remains at **Chat48**; the older serving daemon's cached gen31 Wake
snapshot must not override the fresh independent readback. Until the split
pair is reconciled, only the hourly Chat51 deadman is a confirmed
continuation mechanism. Signing a new main image is not intrinsically required
to update a ChatGPT conversation target if the already-existing operator-owned,
guarded project+Wake target update is used. Conversely unpaired target setters,
hand-editing registry/Wake JSON, spoofed browser events and continuing on the
old target are not acceptable shortcuts.

## New operating model: 3 independent lanes

### A. Developer loop — default for ordinary milestones

- Source of truth: Git branch/commit, review notes, CI (Rust + Python + Wake),
  and the local tested binaries. Git provides source history and rollback; it
  does not by itself authenticate an arbitrary installed administrator binary.
- Developer runs current source in a separate non-elevated identity and
  nonproduction instance, on distinct state roots/listeners, with a clearly
  marked DEVELOPMENT identity. Never point this instance at the externally
  owned Secure MCP tunnel or overwrite production protected receipts.
- Use one command to build, test, start/stop that isolated worker, inspect its
  status, and revert to the last verified working *development* build. No
  Ed25519 private signer and no administrator access for each source edit.
- The development instance must fail closed on any production state root,
  external tunnel ownership, privileged install, or real production event
  delivery request. It may use fixture Wake profiles for browser adapter
  testing but must not emit Wake events to an unapproved live target.
- This lane's full implementation is NEXT SOURCE TASK, not yet present.
  First acceptance: verified isolated no-tunnel/no-protected-state startup,
  then bounded provider work/review and recoverable restart after crash.

### B. Independent Wake/continuity — not coupled to binary rotation

- WakeHost is its own versioned, hash-pinned host with an independent
  production profile. CatDesk source already supports a **paired CAS**
  transaction `mcp::operator_update_designated_chat_target`, exposed through
  Binagotchy `target set <url>`. It reads coherent old project/Wake identity,
  performs a guarded generation increment, compensates a failed second stage,
  and rejects mismatches. Current source also has a special correction for
  a known earlier registry-only digest defect.
- **Immediate T0476 code repair (THIS COMMIT):** opening
  `--catdesk-binagotchy-cli` no longer calls
  `start_installed_only_if_desired_running` at all. Status and a guarded
  `target set` must NEVER launch a stopped WakeHost; only explicit
  `wake start`/`wake resume` may do so. This directly removes the old
  accidental-start risk from the operator console's paired rollover path.
- Operator may run the **local source-built, tested CLI in normal PowerShell**
  without installing it as the protected production main image, only to
  perform this user-owned **narrow paired target operation**. The CLI explicitly
  does not start a second daemon or Secure MCP tunnel and does not invoke the
  signing/rotation machinery. This is not authority for unsigned production
  daemon deployment. The same safeguards (canonical URL, exact old CAS,
  source-current paired transaction, independent registry+Wake readback) apply.
- For Chat51, the expected target is
  `https://chatgpt.com/c/6acaae13-4b18-83e9-b6ca-3c5e00cf47a8`, hash
  `7faf9cc96669e240362da0dd4266bec8b2585f0ef3e03b4b56ae4c8f66db1b22`.
  The **registry** still shows the old Chat48 URL with SHA
  `3a1d4cc69dfa3d05cb2bc33ef1197ac5a7433a120cfb4214a6996edb5a9d29e9`,
  while independent Wake is **already Chat51 generation32**. T0477's guarded
  wake-first recovery source is specifically designed to commit only the
  outstanding registry side when Wake and the caller's exact requested
  target/digest already agree; it must NOT advance Wake to generation33.
  A separate preexisting helper still handles same-URL/damaged-registry-SHA
  correction. No blind retry of an old paired rollover is permitted.
- Precise live gated steps: focused tests + all Windows CI green for the
  **T0477 R2 partial-recovery fix** -> assistant first tries the currently
  supported, guarded paired CatDesk tool (old serving gateway has rejected
  this exact split state) -> only if still unavailable, a **rare operator**
  one-line locally rebuilt source CLI issues `target set <Chat51>`, `target`,
  `wake status`, `exit`. It must return the exact success receipt and
  current Wake generation32 without advancing it, plus the assistant's
  independent CatDesk registry readback must show Chat51 URL/hash. Do not
  rely on old-serving `catdesk_transport_status` if its Wake snapshot is
  stale. Only when both authorities have been proven equal with WakeHost STOPPED ->
  **separate explicit authorization** to start WakeHost if auth/session ready ->
  one MANUAL canary, explicit receipt, then actual natural event acceptance.
  Never infer success from CLI output alone. A singleton Binagotchy mutex can
  make a one-shot command exit harmlessly without executing; require
  explicit success and full independent verification.
- Failure condition is STOPPED with exact diagnostic; don't repair by writing
  either protected store directly or running old installed source.
- Production-ready future improvement: move the signed/guarded paired target
  authority into the independent Wake control plane, not the app-image updater,
  with exact two-store compatibility/rollback tests.

### C. Production update — intentionally infrequent, simple and guarded

- Retain signature verification where genuinely necessary to protect privileged
  production execution. GitHub commit/CI identifies candidate source, but an
  installed binary needs its own measured executable/manifest assurance.
  For a one-person operator system, exact pinned hash + local explicit
  approval is sufficient for development; don't demand an offline root-key
  signature for ordinary source testing.
- Converge on the **already designed stable supervisor** T-0223: a small,
  infrequently updated trusted launcher/control plane with versioned worker
  directories. Do not replace an executable image currently mapped by
  Windows. Stage a new version beside the old one; review + verify hash;
  quiesce only the CatDesk-owned worker; atomically select the new version;
  restart; health-check; restore previous version automatically if not ready.
- Use one operator-facing release command and one retained last-known-good
  release. State shows `currentCommit, currentImageSha256, priorImageSha256,
  status, rollbackAvailable, wakeTargetGeneration`. A single bounded failure
  reason is better than multiple ambiguous review/epoch/promotion states.
- Protected authority remains: no private Ed25519 key accessible to CatDesk,
  no signing fakes, no direct Program Files image overwrite or pending receipt
  deletion, no Secure MCP tunnel lifecycle claim. Existing signed pending
  T0366 epoch2 must be reconciled safely as historic provenance, not secretly
  discarded; this is a separate production maintenance task with a reviewed
  quiesce/upgrade flow and bounded Windows failure diagnostics.
- Consolidate redundant build/attestation/promotion/supervisor gates behind
  one transaction which produces source SHA, built SHA, tests, review approval,
  installation receipt, health/rollback receipt. Internal integrity checks
  remain, but the user doesn't need a new manual ceremony for each substage.

## Priority reversal and measurable next milestones

1. **T0476 — No implicit Wake activation.** Completed source fix and all
   three GitHub Windows CI jobs passed (commit `4213ba8`,
   run `38105874271`); the non-daemon local CLI cannot start WakeHost
   merely by opening.
2. **T0477 — Complete the interrupted Chat51 target pair.** Source commit
   `58c4bfc` keeps Wake generation32 as-is while reconciling old registry
   Chat48 to Chat51 under exact CAS, preserving legacy same-URL digest-only
   repair. Windows CI run `38108654054` PASSED all three Windows jobs, including
   both designated-chat repair regression tests, before live recovery;
   require explicit updated CLI receipt and independently verified paired
   readbacks, then separate Wake start/canary approval.
3. **T0478 — Isolated dev runner.** One explicit `catdesk dev ...` interface,
   separate state and ports, no production transport or signer, build/test,
   stop/restart and measurable task progression, no accidental session
   duplication. CI and safety tests prove forbidden production access.
4. **T0479 — Stable supervisor versioned worker release.** Inventory what
   T-0223 already implements; implement missing operator activation, versioned
   source-current rollout, LKG and bounded recovery. Avoid new overlapping
   release systems or a second tunnel owner.
5. **T0480 — Production signed-state cleanup.** Review T0366 pending stage,
   file-sharing/ACL evidence, identify the real elevated replacement failure;
   preserve signed receipts, provide reviewed one-shot completion or
   monotonic supersession before any new signed release. Stop the current
   unbounded series of bespoke T03xx scripts.
6. Resume Recovery nine-layer doctor and core milestones, then multi-project
   scheduling / Binagotchy UI / natural Wake maturity. Source implementation
   is allowed to progress via dev lane while production signing is parked.

## Explicit rollback and gate criteria

- Target binding success requires **both** project registry and independent
  Wake target exact canonical Chat51 + SHA, Wake generation **32 retained**
  through the recovery (not incremented again), WakeHost STOPPED,
  no old Chat48 submission, no duplicate.
- The first live Wake MANUAL canary is not natural acceptance; check one exact
  new generation receipt and stopped/ready status before declaring reliability.
- The old source/current official Secure MCP client remains externally owned
  and healthy; no duplicate service or tunnel introduced.
- Do not claim the new dev runner, stable supervisor or one-command production
  release already exist; this document defines their implementation standard.
- If source-only CLI rollover is blocked on the operator machine, the branch
  remains safe and the hourly deadman remains the fallback; continue source
  milestones without waiting for signed production rotation.

## Evidence

- T0474 operator Win32 non-admin probe result at
  `.catdesk/t0474-rotation-open-probes.json`: installed/staging READ0 DELETE5;
  incoming READ0 DELETE0; `tokenIsAdministrator=false`. No Win32 32
  observed *for those exact access modes*. This cannot prove elevated
  MoveFileExW success/failure.
- Existing T0473 fresh inventory at `.catdesk/t0372-rotation-state.json`
  and signed T0472 readback verified T0215 installed1/T0366 pending2.
- Source `binagotchy_cli.rs` has a paired updater. T0476 removed
  `start_installed_only_if_desired_running` from console startup.
  Independent Wake was later observed already changed to Chat51 generation32
  without a corresponding CatDesk registry commit; no WakeHost launch/browser
  event, protected main-image rotation, production daemon reload or private
  signer use was performed by this recovery work.
