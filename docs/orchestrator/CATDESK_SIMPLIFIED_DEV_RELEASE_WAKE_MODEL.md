# T-0476 — CatDesk fast development, safe Wake recovery and simple production releases

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
