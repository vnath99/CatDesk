# T-0485 — Protected Wake installation broker and dev.84 migration

## 2026-10-21 retained store inventory binding (source only)

The fixed-policy offline V2 diagnostic now requires the retained old-store
lock set as well as the role pins. Before and after signature verification it
checks the exact selected inventory digest and membership of the signed
predecessor root/lock pair. Its canonical UTF-8 JSON uses schemaVersion 2,
purpose `catdesk-wake-offline-selected-store-inventory`, and a `stores` array
of `root`/`hostLock` full and legacy identities. Distinct pairs sort by full
root volume/ID then lock volume/ID; physical aliases share one entry while
every original alias remains retained and revalidated. These exact bytes are
the anchor's `closedStoreInventorySha256` input.

This closes the missing comparison between signed inventory bytes and the
actual retained lock set. It does not independently establish the historical
inventory's completeness, launcher quarantine or owner exit. Those still need
reviewed evidence before first-generation ledger admission. No signature
domain, service transaction, ledger authority, install guard or runtime start
is enabled. Disposable fixtures cover aliases/order, extra/omitted stores,
digest/role mismatches, contention and release. Evidence is in the parent
`checkpoints/offline-store-inventory-oct21` dated audit.

## 2026-10-20 source wiring audit (no installation)

At `87c8ebd`, `run_one_service_submission` still unconditionally returns
`WAKE_BROKER_TRANSACTION_GATES_UNAVAILABLE` after its admitted frame. Ordinary
`wake/install.ps1` refuses before work unless BuildOnly. Offline V2 remains a
diagnostic with no first-generation ledger/install consumer; production ledger
admission requires unproved migration-anchor and old-owner evidence before CI
and INTENT. Thus provisioning alone cannot finish the release. Typed staging,
publication and pointer writers already exist and should be reused after
admission, not recreated. Remaining work includes the independently accepted
closed store/launcher inventory/anchor, V2 admission and transaction integration,
protected reader/bootstrap cutover, approved provisioning and exact remote CI.
No guard, trust key, approval flag, service, pointer or production package was
changed. Bounded development Web acceptance can proceed independently once its
current explicit host-start hold is lifted.

## 2026-10-20 retained paired-target readback (source only)

The publisher and pointer writer already exist; earlier test-only/no-publisher
statements below are historical. First-generation admission is still closed.
V2 role revalidation now reads the selected old root's pinned fixed config.json
and the retained CatDesk registry file, checks their signed exact raw-byte
snapshot hashes, and requires matching project URL/digest/conversation and Wake
generation. The registry does not store an independent generation counter.
Pointer/lock roles must resolve under that same pinned old root. Bounded reads,
duplicate-target/project rejection and write/replacement denial are verified by
15 focused offline migration tests. No new install authority, service mutation,
production package change or Wake/browser execution follows from this change.
Closed store/launcher inventory, independently reviewed first-generation anchor,
ledger admission, provisioning/reader cutover, exact remote CI and installed
acceptance remain open. See the current October MVP acceptance record.

## 2026-10-15 native no-replace publication fixture proof (source only)

Four Windows fixtures prove a single handle-relative no-replace rename after the durable attempt marker, flush both pinned parents, and require exact post-move bytes, ACLs and physical IDs. Collision, injected parent-flush failure, actual DACL drift and same-byte object replacement stay fail closed. Full Wake 213 library tests passed/one ignored plus enabled targets; strict checks and independent review passed. See `T-0485_R30_NATIVE_RENAME_FIXTURE_PROOF.md`. Pushed source `ab67d390dee529e604904daac604ca1e161e34a5` had zero exact GitHub workflow/check runs. The rename is test-only: the stage receipt's delete-denying pins still need a safe production handoff to a typed writer receipt. No production PUBLISHED writer or install authority exists; service/install holds stay closed.

## 2026-10-15 durable publication-attempt marker (source only)

A separate canonical create-once marker now binds signed approval, STAGED digest, root and stage/parent physical IDs before any future rename. Record and parent flush/readback precede publication; reopen at STAGED returns sticky attention and bound PUBLISHED-or-later replay requires the marker. Historical unbound V1 records remain readable without future authority. Full Wake 209 library tests passed/one ignored plus enabled targets, strict checks and independent review passed. See `T-0485_R29_PUBLICATION_ATTEMPT_MARKER_SOURCE.md`. Pushed source `c8e9fa029ff5935d8a7b03c7d5535a240f296408` had zero exact GitHub runs/checks. Native production rename, typed PUBLISHED receipt, dev.84 anchor, signed broker and exact CI remain open.

## 2026-10-15 durable packages-parent STAGED binding (source only)

New STAGED records durably bind the pre-existing protected `packages` parent physical ID. The typed stage receipt pins/revalidates its ACL and ID across the STAGED/root flush; locked read-only resume rejects same-name parent replacement. The optional trailing field preserves canonical old V1 replay, but unbound records cannot authorize a future publisher. Full Wake 204 library tests passed/one ignored plus binary/integration tests, strict gates and independent review passed locally. See `T-0485_R28_DURABLE_PACKAGES_PARENT_STAGE_BINDING.md`. Pushed source `b74ef2e00b1d21d9cd97a058af6fc94e431b5b7b` had zero exact GitHub check-runs. Pre-rename marker, no-replace rename/PUBLISHED receipt, dev.84 anchor, signed broker and exact CI remain open; service/install holds stay closed.

## 2026-10-15 retained moved-package pin (source only)

The locked exact-resume diagnosis now holds the moved package, five leaves and both namespace parents through its final ledger recheck. Retained handles deny write/delete sharing; fixture cases reject ambiguous location, object/byte/ACL drift and mutation attempts. Full Wake 200 library passed/one ignored plus binaries/integrations, combined root 1,231 main passed/28 ignored, strict gates and independent review passed locally. See `T-0485_R27_RETAINED_MOVED_PACKAGE_PIN_SOURCE.md`. Pushed source `aac1c7e2e4e5a6e820b22098ca739f2a0a67c17b` had zero exact GitHub check-runs. The packages parent is not durably bound to STAGED; production rename/PUBLISHED/pointer/owner/reader and first-generation migration anchor remain open. Service/install holds stay closed.

## 2026-10-15 protected package publication seam (source only)

Locked read-only exact-resume diagnosis now distinguishes the STAGED package at its staging slot from the exact signed package destination after a rename, retaining staging-parent and six package object bindings. A rename before PUBLISHED is sticky attention; altered or ambiguous physical state refuses. Focused and full Wake tests (200 library passed, one ignored, plus binaries/integrations), combined root Rust and strict gates passed locally; independent review found no blocking issue. See `T-0485_R26_PROTECTED_PACKAGE_PUBLICATION_SEAM_SOURCE.md`. Pushed source `aa8059c99adb5600b8d2e4af7abafcb61b5b17140` had zero exact GitHub runs/checks on immediate readback. The production writer, dev.84 connected owner/store anchor, signed broker, exact CI, protected dev.85 activation and fresh Chat53 remain open; service/install holds stay closed.

## 2026-10-15 canonical protected pointer plan (source only)

The reader now plans canonical `current.json` bytes and SHA-256 from a verified approval and supplied stage IDs, checking stage identity/collisions and reader schema/byte limits. It writes nothing and grants no ledger, pointer-switch or install authority: the approval key is not independently bound to protected broker policy here, and stage IDs do not prove publication. Focused reader 13 passed, full Wake 199 library passed/one ignored plus binaries/integrations, combined root Rust and strict gates passed locally; independent source review found no blocker. See `T-0485_R25_CANONICAL_PROTECTED_POINTER_PLAN_SOURCE.md`. Pushed source `007e6001c944df320793a6339dabceb799773d840` had zero exact GitHub runs/checks on immediate readback. First-generation dev.84 owner/store reconciliation, signed broker, production writer/reader cutover, exact CI, protected dev.85 activation and fresh Chat53 remain open; service/install holds stay closed.

## 2026-10-15 fresh-successor predecessor gate (source only)

Production ledger acceptance now compares a signed successor with a retained COMPLETE protected pointer/package under the global lock, including exact pointer physical ID/digest and predecessor package/pointer/generation/root. It then refuses before CI fetch or INTENT because the first dev.84 migration anchor and current live old-owner proof are absent. Focused ledger 26 passed, full Wake 197 library passed/one ignored plus integrations, full root/Python and strict gates passed locally. See `T-0485_R24_FRESH_SUCCESSOR_PREDECESSOR_GATE_SOURCE.md`. Pushed source `9e8d1c8d5f4ade8327fdcf521181faf31b901ab30` had zero exact GitHub runs/checks on immediate readback. The production install/service holds remain closed; no protected dev.85 activation or new Chat53 receipt occurred.

## 2026-10-15 read-only pointer-switch binding checkpoint

The ledger can now retain the new `current.json` physical ID and byte digest across `POINTER_SWITCHED`/`COMPLETE`; a legacy unbound completion remains readable but cannot authorize another INTENT. The locked diagnostic compares that binding with the pinned protected reader, still returning no install authority. Focused broker (132 passed, one ignored), full Wake (197 library passed, one ignored, plus binaries/integrations), full root Rust and strict Clippy/format/build gates passed locally. See `T-0485_R23_POINTER_SWITCH_LEDGER_BINDING_SOURCE.md`. Pushed source `c5b35648223c1bb5e838d0f28a61ef28b0a128810` had zero exact GitHub runs/checks on immediate readback. The production pointer writer, staging parent, first-generation dev.84 anchor, owner/readers transaction, signed broker and exact-current CI remain open; install guards stay closed.

## 2026-10-15 read-only completed predecessor checkpoint

The broker can now corroborate a nonempty COMPLETE ledger head against pinned protected pointer/package objects under its global lock, requiring exact policy continuity and installed package/manifest/four-artifact physical IDs. An integrated Windows fixture covers durable history, same-length artifact drift, unchanged ledger bytes and no new INTENT. The diagnostic explicitly leaves the staging parent, new pointer switch and first-generation migration anchor unverified; it grants no install authority. Full Wake and root Rust suites, strict Clippy, formatting and all-bin build passed locally. See `T-0485_R22_COMPLETED_PREDECESSOR_PROJECTION_SOURCE.md`. Pushed source `ee5e9e39e644966a9cef211d8b5af1038040c888` had zero exact GitHub runs/checks on immediate readback. The service/install guards remain closed; no dev.85 activation or new Chat53 receipt occurred.

## 2026-10-15 broker-owned exact-CI fetch source checkpoint

Production ledger acceptance now fetches the signed SHA's GitHub runs and attempt jobs itself from fixed WinHTTP paths after signature verification and before INTENT. It rejects non-200, redirect, incomplete, changed, oversized or nonmatching evidence; a process-wide permit prevents concurrent stalled fetch workers. Full Wake tests and strict Clippy/format passed, and a read-only native transport probe matched an older green run. See `T-0485_R21_BROKER_OWNED_EXACT_CI_FETCH_SOURCE.md`. Pushed source `d66d22a` has zero exact workflow/check runs on bounded readback. No signed/provisioned broker, trusted current dev.84 connected owner, approved transaction or protected activation exists; service/install guards stay closed.

## 2026-10-15 fixed SCM entry and bounded pipe source checkpoint

A separate fixed `CatDeskWakeInstallBroker` service entry point now exists in source. It requires SCM launch, reports RUNNING before its own SCM preflight, pins the compiled protected root/policy and global lock, proves its running service/process before creating one fixed endpoint, and reads one operator-admitted frame with bounded overlapped connect/read. STOP, deadline, callback error and unwind retire pending I/O safely; a one-shot refusal cannot be reused. It always ends with `WAKE_BROKER_TRANSACTION_GATES_UNAVAILABLE` before INTENT or any install mutation. The full independent Wake suite passes locally (181 library tests plus binaries/integrations), as do strict Clippy, formatting and repeated cancellation-race tests. See `T-0485_R20_FIXED_SCM_BROKER_BOUNDARY_SOURCE.md`. No signed service/namespace was provisioned, trusted dev.84 connected owner or independent exact CI provider is available, and no approved package/pointer/reader transition or fresh Chat53 Wake occurred. Both install guards remain closed.

## 2026-10-15 guarded exact-resume diagnosis (source only)

The dormant broker ledger now offers a read-only policy-bound inspection under its retained global lock. It creates no record or lease: missing INTENT refuses, INTENT requires empty staging, STAGE_STARTED is sticky attention, and STAGED compares protected five-file bytes/ACLs and seven physical IDs with the ledger. Three integrated tests and the full Wake suite pass locally; see `T-0485_R17_GUARDED_READ_ONLY_EXACT_RESUME.md`. The observation callback still requires a signed broker with trustworthy clock and pinned process/namespace evidence; no recovery mutation, package/pointer/owner/reader transition, exact-source CI or protected activation is enabled. Both install guards remain closed.

## 2026-10-15 write-ahead stage durability source checkpoint

The dormant ledger now flushes a `STAGE_STARTED` marker before candidate creation and refuses any second create after reopening that marker. Pinned parent directories are flushed after ledger record and candidate/leaf entry creation, with a hard failure if the OS flush cannot be performed. Fault/restart tests and the full Wake suite pass locally; see `T-0485_R16_STAGE_STARTED_PARENT_FLUSH_SOURCE.md`. A controlled power-cut test on the actual protected deployment filesystem is still required before claiming sudden-power-loss durability. The signed service/bootstrap, trusted observation/clock, independent exact-CI fetch and full package/pointer/owner/reader transaction remain absent; install guards stay closed.

## 2026-10-15 fixed-source INTENT→STAGED transaction source checkpoint

The dormant broker transaction now acquires the pre-provisioned global lock before calling a future trusted observer, requires a policy-key-verified opaque approval, pins exact five-file source bytes, creates and rechecks the fixed protected `staging/candidate`, then records its seven physical IDs in a typed STAGED phase. A residual partial stage is sticky attention. The temp-root integrated test covers fresh copy/record/reopen and partial-stage refusal, but uses a test-only owner/security mode. See `T-0485_R15_FIXED_SOURCE_TO_STAGED_TRANSACTION.md`. The callback and exact CI fetch are not implemented by a signed service, parent-directory power-loss durability is unproven, and no package/pointer/owner publication or protected activation is enabled. Both install mutators still refuse with `WAKE_INSTALL_BROKER_UNPROVISIONED`.

## 2026-10-15 exact-source CI scheduling audit

Authenticated read-only CLI inspection finds the repository active, Actions permission `enabled:true`/`allowed_actions:all`, workflow `311012404` active, and no repository execution policy. `ci.yml` has push and pull-request triggers but **no `workflow_dispatch`**. The prior `gh workflow run` attempt's retained CLI error text is HTTP 422 `Actions has been disabled for this repository`; its raw response was not retained, and the missing manual trigger is a distinct issue. The latest green push run is still for older `4e9a2e2`, while the pushed `7dafbf1` has zero exact runs/checks. The personal Actions billing endpoint returned 404 with a missing `user` scope, so billing/account restriction state is unknown. No new dispatch, scope refresh or settings change was attempted. See `T-0485_R14_CI_SCHEDULING_READ_ONLY_TRIAGE.md`; exact-source Windows CI remains mandatory.

## 2026-10-15 fixed candidate-stage readback source checkpoint

`install_broker_candidate_stage.rs` now inspects only the compiled protected `staging/candidate` slot. It verifies the detached approval against the pinned protected policy key and supplied observations, then compares the canonical manifest and exact four artifact bytes. It pins no-follow, no-write/delete-share handles, requires exact SYSTEM-owned protected ACLs and directory inventory, checks length/hash/object identity, and reopens the fixed names. Its readback explicitly marks stage creation and ledger binding **unproven**: stale identical bytes can match, and the result grants no installation authority. Seven focused Windows tests passed; see `T-0485_R13_FIXED_CANDIDATE_STAGE_READBACK_SOURCE.md`.

There is still no broker-owned source copy or protected stage creation record tied to an intent generation and physical slot identity, nor an installed signed service/namespace or independent CI fetch. The source-only readback cannot clear either production `WAKE_INSTALL_BROKER_UNPROVISIONED` hold or authorize dev.85 promotion.

## 2026-10-15 inert build freshness and one-shot service source checkpoint

`wake/install.ps1 -BuildOnly` now rebuilds both WakeHost and the GUI from the working source, even when an earlier GUI target exists. The normal install path still refuses with `WAKE_INSTALL_BROKER_UNPROVISIONED` before either build. A PowerShell parse and mock-cargo probe passed; no protected state was touched. See `T-0485_R11_BUILDONLY_GUI_FRESHNESS.md`.

`install_broker_service_core.rs` is a dormant, one-shot read-only wrapper. It consumes the fixed endpoint, checks the protected policy-root physical ID before request I/O, uses the immediate same-pipe operator-token admission, then verifies the signed envelope with the pinned policy key against **supplied** observations. The result explicitly says `NO_INSTALL_AUTHORITY` and lists missing trusted observations/clock, exact four candidate bytes, independent CI, ledger, owner handoff, and protected launch/service bootstrap. Two focused Windows tests, full independent Wake suite, strict Clippy and formatting passed. See `T-0485_R12_ONE_SHOT_READ_ONLY_SERVICE_CORE_SOURCE.md`.

There is still no installed signed service, protected namespace/trust-root bootstrap, bounded connect/read server loop, independently fetched CI or approved package/pointer transaction. The supplied observation is not broker-owned evidence. Both install mutator holds remain; source-only tests and BuildOnly artifacts cannot authorize dev.85 promotion or a fresh Chat53 wake.

## 2026-10-15 connected client and single-message request source checkpoint

`install_broker_client.rs` now has a dormant fixed-pipe client gate that sends no application bytes before checking the connected server against the exact running SCM service PID, a pinned live process, a LocalSystem primary token with enabled and restricting service SID, the fixed protected image and exact pipe DACL. Its consuming first approval write rechecks those observations on the same handle. Four focused Windows tests pass, including a same-user pipe squat that receives zero approval bytes. `install_broker_request.rs` reads only one bounded message from the already connected fixed server endpoint and immediately checks the last-read client token against pinned protected policy before decoding; three focused live-pipe tests pass. See `T-0485_R9_SINGLE_MESSAGE_BROKER_REQUEST_SOURCE.md` and `T-0485_R10_CONNECTED_BROKER_CLIENT_GATE_SOURCE.md`.

These are transport primitives, not an installed service or approved transaction. A real provisioned-service fixture must show that the elevated operator can query the service's restricted SYSTEM token without enabling an extra privilege; the client fails closed if it cannot. The process image path and currently pinned image hash do not prove the historical loaded image section. Server connection/read deadlines, exclusive last-read ownership, independent CI fetch, protected ledger/namespace transaction, launch-reader cutover, dev.84 reconciliation and exact-source Windows CI remain open. Both production install holds remain.

## 2026-10-15 protected broker policy source checkpoint

`install_broker_policy.rs` now has a dormant, fixed-root read-only loader for one canonical bounded `broker-policy.json`, pinned to the compiled ProgramData namespace and preflighted service. It requires exact root/file physical IDs and SYSTEM-owned DACLs with broker-only write and IU read, a single-link non-reparse file, an exact user operator SID, and an independently installed nonweak Ed25519 approval public key. The opaque value rechecks original and fixed-name reopened bytes/security before use; the endpoint derives its DACL SIDs from it and grants clients `READ_CONTROL` without pipe-create-instance rights. The request seam checks the connected operator token immediately after its last read without intervening policy I/O. The fixed SCM/image source policies now grant the exact IU query/read rights needed for interactive client preflight, without mutation rights. Eight policy, three preflight and five endpoint focused Windows tests pass; see `T-0485_R8_PROTECTED_BROKER_POLICY_SOURCE.md` and updated R2/R7 bundles.

This is source-only. There is no privileged policy/image bootstrap, protected trust-root provenance, installed service, complete ancestor/worker-token denial, approved transaction, live launch-selector migration or dev.84 reconciliation. Both production install holds remain, and exact-source Windows CI is still required.

## 2026-10-15 fixed pipe endpoint source checkpoint

`install_broker_endpoint.rs` now has a dormant fixed-name, first-instance, remote-rejecting, non-inheritable message-pipe constructor with exact owner/DACL and mode readback. The production constructor requires an opaque operator SID without a current provisioner and the fixed broker preflight; it cannot run here. Its operator ACE excludes `FILE_CREATE_PIPE_INSTANCE`, and five focused Windows fixtures pass. See `T-0485_R7_FIXED_BROKER_ENDPOINT_SOURCE.md`. A pipe-name squat before service start still requires future **client-side authentication of the connected service process/image/token**; the genuine service fails closed on its first-instance collision. There is no service, endpoint listener, approved request, protected policy or install mutation.

## 2026-10-15 connected operator and protected reader source checkpoint

`install_broker_operator.rs` now has a dormant connected-pipe Windows impersonation/token admission check against a SID that a future broker must obtain from independently protected policy. It requires exact user, high integrity, elevation, an interactive session, impersonation/delegation level, no AppContainer and no token restrictions, then reverts the service thread. Seven focused tests pass, including a real pipe client/revert fixture; see `T-0485_R5_CONNECTED_OPERATOR_TOKEN_SOURCE.md`. It does not create a service/endpoint or authorize an install. The future service must serialize the last request read and this check on one pipe instance.

`install_broker_reader.rs` now returns a zero-argument read-only snapshot of the fixed proposed ProgramData pointer/package, using pinned no-follow handles, exact leaf ACL/inventory/identity/canonical metadata, all four byte hashes and a final pointer reopen. Ten focused tests and independent audit passed; see `T-0485_R6_PROTECTED_INSTALL_READER_SOURCE.md`. It does not bind the completed ledger or carry pins into launch. All current launch/status/recovery readers still use the legacy per-user selection, and the protected service/namespace is unprovisioned. Neither new module changes `WAKE_INSTALL_BROKER_UNPROVISIONED`.

## 2026-10-15 CI comparison and protected-ledger source checkpoint

`install_broker_ci_witness.rs` compares the signed exact-CI claim with independently fetched, complete GitHub run and attempt-job JSON, requiring the latest matching push run and three successful Windows jobs; it performs no fetch or signing and grants no authority. Seven focused tests pass. `install_broker_ledger.rs` holds a pre-provisioned global lock, inventories history through a fresh pinned relative directory object, refuses gaps/replay/corruption, and records create-once ordered phases with original desired/owner state. It can cryptographically reverify the original envelope against the protected acceptance record for exact post-expiry resume. Fifteen focused tests pass; see `docs/orchestrator/review_bundles/T-0485_R3_CI_CLAIM_COMPARISON_SOURCE.md` and `T-0485_R4_PROTECTED_LEDGER_SOURCE.md`.

Both modules are dormant. The future broker must independently fetch CI, protect its own image/key/root/lock, anchor the first generation after dev.84 physical reconciliation, use a trustworthy clock, prove parent metadata durability and reconcile actual package/pointer/owner side effects. The production install hold remains. The exact pushed source SHA has no Windows CI run.

## 2026-10-15 fixed broker read-only preflight checkpoint

The source now includes a zero-argument diagnostic of the proposed fixed SCM service and protected ProgramData/Program Files objects. It uses pinned no-follow relative opens, exact leaf owner/DACL policy and image identity/hash readback. A real invocation on this host failed closed at `WAKE_BROKER_SERVICE_UNPROVISIONED`; no service was installed. Three focused policy tests and the final full Wake suite passed. The snapshot explicitly says it is **not installation authority**; ancestor effective access, signature/integrity, worker-token denial, service protocol, protected key/ledger and launch-reader migration remain. See `docs/orchestrator/review_bundles/T-0485_R2_WAKE_BROKER_READ_ONLY_PREFLIGHT.md`.

## 2026-10-15 source approval verifier checkpoint

`wake/src/install_broker_approval.rs` now verifies a bounded canonical, purpose-specific Ed25519 approval envelope. Its signed payload binds the exact four-artifact manifest, independent-source/CI claims, predecessor pointer/package, protected and old runtime physical objects, old host image/PID/creation time and install generation. Six focused tests and the final full independent Wake suite passed locally. This is a dormant verifier, not an issuer, service, installed key, transaction or launch selector. Both production `WAKE_INSTALL_BROKER_UNPROVISIONED` holds remain. See `docs/orchestrator/review_bundles/T-0485_R1_WAKE_APPROVAL_VERIFIER_SOURCE.md`. Exact source commit CI, fixed broker service/namespace, worker-token denial, replay ledger and live migration remain required.

**Priority:** P0 prerequisite for installing dev.85 and accepting T-0483's real review-to-Wake cycle.
**Status:** Implementation ticket; broker, protected namespace, approval issuer, and live migration are **not provisioned**.
**Evidence base:** [T-0483 R10](../review_bundles/T-0483_R10_WAKE_CALLER_APPROVAL_BOUNDARY_HOLD.md), [R11](../review_bundles/T-0483_R11_WAKE_OWNER_CHALLENGE_SOURCE.md), [R12](../review_bundles/T-0483_R12_INSTALL_BROKER_HOLD.md), and [R3](../review_bundles/T-0483_R3_WAKE_STORE_AUTHORITY_PROVENANCE.md).

## Problem and existing boundary

`wake/install.ps1` currently refuses every normal install with `WAKE_INSTALL_BROKER_UNPROVISIONED` before building or staging. `-BuildOnly` is inert. In `wake/src/bin/CatDeskWakeHost.rs`, `guarded_install_store` obtains the object guard and live host-owner challenge, then calls `require_install_mutation_broker` before `Store::open`; this closes `install-init`, `install-set-source`, `publish-reviewed-install`, and `activate-reviewed-install` in production. The narrowly scoped `test-support` temporary fixture is not a production authorization path.

The root and `host.lock` proof identifies the physical runtime store and serving host. It does not authenticate the requesting installer or approve the four artifact bytes. Today `runtime::default_root()` selects `%LOCALAPPDATA%\CatDeskWake`; `wake/src/runtime.rs` places `versions`, `current.json`, `previous.json`, `reviewed-install-handoff.json`, `install.lock`, and the review-source/config transaction in that user-owned store. A same-user process can bypass a candidate-built CLI and write those objects directly. A self-consistent manifest, hashes, PID, root fingerprint, or a signature check executed only by a caller-built candidate cannot close this gap. The existing signed **main CatDesk image** trust root has a different purpose and must not be reused as Wake package authority.

Installed dev.84 cannot answer the new owner challenge. Earlier connected-versus-direct probes found different physical root/lock objects behind the displayed path, with connected Chat53 generation 34 and a separate direct-shell view. Those are historical observations, not current live readback. No installer may infer reconciliation from a path string or replay the old ambiguous manual Wake event.

## Required design and source deliverables

### 1. Separate fixed trusted service and protected namespace

Implement a **separate, fixed, signed Windows service** for install mutation. An administrator-controlled bootstrap installs its fixed binary and policy under a protected Program Files location, configures a restricted service SID (for example `NT SERVICE\CatDeskWakeInstallBroker`) and a LocalSystem service account, and independently reads back the SCM image path, binary identity/signature, account, SID policy, start type, and service security descriptor. The service must not load its policy, executable, or verification key from a candidate package, workspace, user-writable directory, or caller-selected path. Service upgrade is a separately signed/reviewed operator operation; the Wake candidate cannot replace its own broker.

Bootstrap creates one fixed versioned namespace, proposed `C:\ProgramData\CatDesk\WakeProtectedInstallV1`, with broker-only write access for package staging/final directories, `current`/`previous` pointers, review-source authority, transaction journal, approval replay ledger, and broker policy. Grant read/execute only where the interactive WakeHost and Binagotchy need it. Validate the complete ancestor chain, owner, DACL/inheritance, integrity, volume/file IDs, reparse state and hardlink policy on startup and before mutation. Any missing, retargeted, writable, or ambiguous object fails closed; the service never repairs a caller-supplied path opportunistically. Untrusted workers, ordinary same-user processes, and their descendants must have no write/delete/rename/ACL/owner rights over this namespace or service image.

Refactor the runtime/install layout explicitly. Keep Wake queue, browser delivery, timers and other runtime data in the intended interactive-user context, while **the executable package, install pointer, review-source binding, and install journal are broker authoritative**. `start_installed`, `reviewed-install-status`, Binagotchy shortcuts, recovery, and handoff must resolve the executable through the protected pointer and revalidate it; a legacy `%LOCALAPPDATA%\CatDeskWake\current.json` must not select code after migration. If a runtime file remains user writable, prove the restricted Codex worker cannot change it and that it cannot override protected install authority. Do not run the browser/profile as SYSTEM: the broker coordinates an exact reviewed interactive-session launch with the proper user token and verifies the resulting owner.

Expose only a fixed, bounded local install protocol. The mutation endpoint's DACL and the server's impersonated-client token checks require the independently authorized operator/installer identity and reject low-integrity/AppContainer workers; PID, process path, environment variables, or a same-user name alone are insufficient. Neither the endpoint nor the service offers a generic filesystem writer, arbitrary command runner, caller-selected service path, or arbitrary target root. A worker can submit inert candidate bytes for review but cannot publish or activate them. Keep the present production `WAKE_INSTALL_BROKER_UNPROVISIONED` hold until this service, namespace, protocol and approval path are integrated; do not simply change the guard to return success. Retire or keep denied the four candidate-host mutation commands so installation authority remains in the fixed broker rather than the candidate executable or PowerShell script.

### 2. Independent, exact package approval and replay authority

Define a canonical, versioned, detached `WakeInstallApprovalV1` signed by a purpose-specific independent offline authority. Its private key remains outside CatDesk, Codex, MCP, workers and the broker; its public root is fixed in the signed broker or installed as protected policy by a separate operator action. Reject wrong-purpose signatures and any attempt to repurpose a main-image envelope. At minimum the signed fields bind:

- fixed approval purpose and schema; operation scope (publication and activation, or separately signed phase approvals); unique approval ID/nonce, issue/expiry policy and monotonically advancing install generation;
- exact package ID/version and **names, byte lengths and full SHA-256 digests of all four** `CatDeskWakeHost.exe`, `CatDeskBinagotchy.exe`, `adapter.py`, and `wake_bridge.py`, plus the canonical manifest digest and no extra executable payload;
- exact source commit, independent exact-commit Windows CI workflow/run/job identities and conclusions, and reviewer/approval identity, so an approval cannot be issued for merely claimed source or CI;
- expected predecessor package/pointer and target protected-root physical object identity, plus the reconciled old runtime-root/lock/owner identity needed for this migration.

The broker verifies signature, purpose, canonical encoding, exact fields, policy and current physical objects **before creating or changing install state**. It reads candidate bytes through pinned, no-follow handles, enforces small fixed inventory and per-file bounds, hashes independently, then copies into a broker-created protected stage and verifies the stage again before publication. Caller-computed hashes and caller-written manifests are untrusted hints. The service records the signed approval digest and transaction phase in a broker-only durable replay ledger before the first mutation. An approval may resume **the same exact transaction** after a crash, never authorize another package, store, predecessor, generation or operation. Duplicate completion is idempotent; copied old approval, stale generation, rollback replay, torn/corrupt ledger, or two concurrent contenders fail closed. Specify and test flush/replace order for journal, package materialization and pointer changes.

### 3. Broker-owned, pinned handoff and recovery

Serialize install under a broker-owned lease. Pin the protected root, package directory, active pointer/ledger, and the reconciled serving runtime root/`host.lock` by handles and volume/file IDs through the transaction; reject path swaps, reparse points, links, owner changes and changed process creation time. Verify the old host image and lock owner using kernel evidence, not only `status.json`. Broker alone performs publication, desired-state preservation, stop, old-owner exit, exact pointer switch, interactive restart, new-host owner challenge, and commit. Its transaction journal preserves the previous protected package/pointer and original `RUNNING`/`PAUSED`/`STOPPED` intent. Every crash seam has a deterministic read-only diagnosis and exact resume/rollback rule. An unproven new owner leaves durable attention and the last-known-good package; no blanket cleanup or silent second Wake submission.

`wake/install.ps1` may remain an inert build/inspection frontend or submit an already approved exact artifact to the fixed broker client. It must not stage into the protected namespace, invoke a candidate-built mutation CLI, modify service configuration, or treat a PowerShell confirmation as approval. The installer and broker report bounded transaction IDs, package identities, phases and errors for independent readback. A healthy broker status is not proof of a delivered browser event.

## Dev.84 migration and release separation

1. **Read-only reconciliation first.** Through the exact connected CatDesk process and exact installed WakeHost, capture current PID **and creation time**, executable/package digest, `host.lock` owner and root/lock volume/file IDs, root/config/current/status provenance, desired state, and Chat53 target generation/digest. Compare with a direct caller view at a stable observation point. A missing transport, changed owner, split physical store, ambiguous lock, or different target halts migration; preserve dev.84, its last-known-good package and forensic event records. Do not infer current state from R3's historical values.
2. **Source implementation and isolated review.** Deliver the fixed service, protected layout and runtime reader changes, approval parser/verifier and replay ledger, owner-pinned handoff/recovery, operator bootstrap/readback tooling, and bounded tests. Keep the current install hold through this phase. A source build or local test does not install the broker.
3. **Operator-controlled bootstrap.** After independent review, an authorized administrator installs the separately signed broker, creates and reads back its ACL-protected namespace, provisions the independent Wake public trust root, and verifies the actual worker token/descendants. Adopt the exact dev.84 predecessor only under a reviewed migration transaction that has passed step 1; do not overwrite, delete or silently normalize the divergent per-user store. This is a distinct privileged action with its own audit record.
4. **Protected release approval.** A separate reviewer/offline signer issues approval only for the exact final dev.85 package, predecessor, target object and source/CI evidence. An operator submits that approval to the broker; only then may it publish and activate. No source CI, local hashes, signed main-image approval, broker bootstrap, or operator login by itself is the Wake package approval.
5. **Live acceptance after readback.** Confirm protected pointer/package hashes, source version, serving owner/root/lock identity and unchanged canonical target. Run one **new**, uniquely identified Chat53 current-generation diagnostic canary; require exact USER receipt, `SENT`, timer `COMPLETE`, and no duplicate. A later natural T-0483 review-ready event must bind a real Sol/medium deliverable and exact review record. Keep the hourly deadman disabled and the old ambiguous manual event forensic.

## Adversarial acceptance and CI gates

- From the actual restricted worker token **and each descendant**, attempt create, overwrite, rename, delete, hardlink, reparse/junction retarget, ACL/owner change and service-configuration change against service image/policy, protected packages, pointer, review source, approval ledger, signing transport and host authority. All must be denied; verify opened-object IDs and contents remain unchanged. Repeat with intentionally inherited/duplicated writable handles and prove none crosses the boundary. Also test a same-user process that bypasses `wake/install.ps1` and directly targets the old per-user paths cannot alter the broker's selected package.
- Refuse a correct live PID/fingerprint with no approval; self-consistent caller hashes; wrong-purpose or wrong-key signature; copied/expired approval; wrong physical store; changed predecessor; stale/replayed generation; altered artifact/manifest byte; extra file; link/reparse/hardlink; concurrent install; and false or mismatched source/CI evidence. Assert **no install mutation before rejection** and stable prior owner/pointer.
- Inject failure or process death after each journal write/flush, protected-stage copy, publication rename, stop, pointer switch, restart and new-owner proof. Reopen the broker and prove exact-only idempotent recovery, no duplicate activation, bounded wait, preserved previous package/desired state and explicit attention if owner proof fails. Test dev.84 protocol absence and connected/direct physical-store divergence as hard migration refusals.
- Pass local Windows fixture/security tests, root and independent Wake Rust tests/strict Clippy/format, PowerShell parser tests, and Python Wake/advisor tests as applicable. Obtain **all three independent Windows CI jobs at the exact source commit**: `Rust`, `Python Wake and advisor tests`, `Independent WakeHost Rust` in `.github/workflows/ci.yml`. A green older SHA or a missing/disabled Actions run is not acceptance. Review the final service binary and signed approval against those results before operator bootstrap or protected activation.

**Completion evidence:** source review and exact-commit CI; broker service/SCM/namespace identity readback; denied worker/direct-write and inherited-handle probes; signed exact approval and replay-ledger proof; dev.84 physical-store reconciliation; broker transaction and owner readback; fresh canary and later natural review-ready receipt. These are separate gates. Until each applicable gate passes, dev.85 remains an inert candidate and P0/T-0483 live acceptance remains held.
