## 2026-11-08T05:01:44.269511+00:00 — C1 ordinary delivery capability defect corrected in source

The earlier C0 finding that installed dev84 lacks bootstrap remains true, but the inference that ordinary C1 delivery requires a protected Wake upgrade was too broad. The ordinary sender reused the new-chat bootstrap checker. It now shares unchanged pointer/host/adapter/bridge/manifest integrity checks and admits only the existing protocol-1 ordinary acknowledged-send/receipt contract. Bootstrap callers still require bootstrap capability. The installed package and every project/Wake/native identity are unchanged; no installer or broker guard changed.

25 Wake scoped tests,10 coordinator tests,11 inventory tests, strict root/Wake all-target Clippy and formatting pass. Eight offline control-flow cases used checksum-verified installed adapter/primitives with a synthetic browser/sink; they prove no actual UI or Web acceptance. The existing17-check bearer-forwarding suite also passes on current PowerShell. Inventory now names bootstrap capability separately from an ordinary protocol declaration. C1 remains unaccepted; next build a separate development controller, qualify existing bounded backend-only/official-owner serving, then the original queued invitation without replay. No later milestone or replacement subsystem.

Evidence: docs/verification/ORDINARY_WAKE_CAPABILITY_SEPARATION_2026_11_08.json. The existing protected broker transaction gap remains a separate release/rollover concern, not automatically a prerequisite for an ordinary wake.

# C0–C1 execution checkpoint — November 7, 2026

Observed November 7 at 23:37–23:39 EST (November 8, 04:37–04:39 UTC).
Scope: the operator-activated architecture revision's C0–C1 only. Later gates
have not started. This supersedes the former CatDesk goal pause for this scope;
it does not resume other projects or change their authority.

## Current truth

| Layer | Observation | Acceptance |
|---|---|---|
| Source | Clean admission at `e911c6dd9f37db28ca8446172bc32304bb6e8722`, branch `codex/web-rollover-oct17`; origin initially `4da5c89f423e2f9ea899e6cca478c39be7a73e6a` | PASS inventory; new inventory source/test files subsequently added |
| Built development controller | Retained successful build from `f71a17be41d73875a82a553eda77a40e81cb72fc`; current bytes SHA-256 `cfe86c42c09dd2500458cf94eb27d0fb7b0c96ad56a3947bbf163923c60ce90b` | BUILT; not current-HEAD compilation or production authority |
| Selected development | dev.85 controller, host, adapter and bridge match pointer, manifest and authority checksums; retained verification names the same source and artifacts | SELECTED; past finite health is not fresh live health |
| Canonical backend | Executable, sidecar and generation-4 release pointer match SHA-256 `0c19770e1a73101a260778a19b8d4c8d94f7c44bfa24eb04b9b1207787b5e901` | INSTALLED checksum parity; no port-3200 listener |
| Installed independent Wake | dev.84 artifacts match existing pointer and manifest; stopped | INSTALLED; ordinary sender's required bootstrap protocol absent |
| Local daemon / supervisor | Canonical-owner doctor: `LOCAL_MCP_LISTENER_MISSING` / `SUPERVISOR_REGISTERED_MONITOR_STOPPED` | FAIL for serving; metadata does not qualify a restart owner |
| Connector | One read-only v5 status call: `MCP SSE probe returned 429 from openai.org`, UNAVAILABLE / invalid_mcp_response | BLOCKED_EXTERNAL; no retry or inference about daemon health from this response |
| Existing controlled projects | Two exact native roots clean at their reserved bases; separate native identities; both registry/Wake URL+digest pairs match at generation 1 | PASS metadata only; native writer/authentication liveness not inferred |
| Existing work | Two original requests OPEN, queued and associated; no delivery records | Preserved; no republish, reassociation, send or execution grant |
| C1 code/test/Wake cycle | No new Web USER, native edit, native test result or Wake response | BLOCKED; not accepted |

C0 truth reconciliation is complete for these affected components, including
their failed and unverified layers. It is **not** a runtime-health PASS. Source,
artifact, selection, installation and live results remain distinct.

## Small implementation and actual tests

`scripts/catdesk_acceptance_inventory.py` is an additive, read-only projection
over existing Git, release pointers, manifests, development authority and
verification/build receipts. It creates no registry or authority store. Existing
`catdesk.ps1 doctor` remains the runtime diagnostic. The projection emits no
workspace paths, conversation text/URLs, credentials or native thread IDs.

```powershell
python -I -B scripts/catdesk_acceptance_inventory.py --source '<existing-source>' --canonical '<existing-canonical>' --development '<existing-development-directory>' --wake '<existing-Wake-root>' --build-evidence '<existing-build-result>'
```

Optional `--test-evidence` accepts a bounded JSON object containing
`sourceCommit`, `exitCode`, positive `testsRun`, and `inputHashes` (relative
source path to SHA-256). PASS requires the current commit and every declared
input byte to match. Coverage is explicitly only those declared inputs; this
does not establish independent review, complete dependency coverage, promotion
authority or installed behavior. Inputs are operator-provided evidence, not a
trusted release attestation. Missing evidence stays NOT_RUN. Samples are not an
atomic filesystem snapshot; physical authority/owner verification remains in
the original guarded procedures.

Executed locally through the existing bounded contained-process helper:

- **11 inventory tests PASS:** missing installation, wrong executable checksum,
  changed authority, stale test commit/input bytes, malformed metadata,
  traversal refusal, privacy projection and no inferred live/release authority.
- **46 existing adapter tests PASS** in 13.067 seconds; fixtures/Node DOM checks,
  not a real authenticated browser observation.
- **10 focused bridge recovery tests PASS** in 0.183 seconds: Home versus auth,
  network redaction, settling target drift, duplicate/unknown submission,
  generation completion, ambiguous assistant retry, deadline and crash fences.
- **144 existing doctor assertions PASS** in a 5.859-second process.
- Actual read-only package inventory PASS for checksum consistency, with the
  operational failures above independently recorded by the passive doctor.
- **Full bridge suite INCOMPLETE_TIMEOUT** at 45 seconds. No passing full-suite
  claim and no full rerun. Focused cases above are their own narrower evidence.
- Initial owner workflow refused an outdated PowerShell hash **before tests**.
  The current host was then independently observed as Microsoft-signed Core
  7.6.9, SHA-256 `2a6769a3330dc1084d0154bfde7d164ab750ca7f700f313f9d417c1be78b2233`.
  Only local test admission changed; release/browser guards were untouched.

The five ordinary-delivery implementation paths remain byte-identical to the
previous qualified build source. Its retained 70-case result is historical;
neither a fresh Rust compilation nor whole-application acceptance is claimed.

## Exact C1 prerequisites, recorded once

1. Restore the existing backend/official serving owner through its qualified
   lifecycle. Canonical observation finds a registered but stopped monitor.
   Do not create another tunnel or assume a restart resolves upstream 429.
2. Qualify a browser package that the existing canonical ordinary sender can
   actually use. Source `installed_bootstrap_directory` refuses dev.84's missing
   protocol with `BOOTSTRAP_INSTALLED_PACKAGE_UNSUPPORTED`. This is a static
   compatibility finding; **no send command was run to manufacture that error**.
   Selected dev.85 does not authorize replacing installed Wake. The public
   installer still explicitly refuses non-BuildOnly execution with
   `WAKE_INSTALL_BROKER_UNPROVISIONED`; do not disable it. The native guard is
   also unconditionally closed outside its isolated test feature. The existing
   broker service deliberately ends at `WAKE_BROKER_TRANSACTION_GATES_UNAVAILABLE`.
   This is a real transaction-wiring/authority prerequisite; merely installing
   or starting a broker service cannot resolve it. Its fixed image/namespace
   are absent in the read-only presence observation. Reuse the existing broker
   components and genuine review evidence; do not design another installer.
3. Once serving and package authority are genuinely ready, require a successful
   bounded connector observation and exact current target/native-owner admission.
   Continue the original queued invitation; never replace it to evade an outcome.
4. Only an actual same-task source receipt, authorized native edit, recorded
   verification exit and exact Wake response can pass C1. Queued requests,
   context ACKs and local fixture tests cannot.

Next eligible engineering step is the missing existing Wake broker/migration
transaction admission and lifecycle-owner qualification needed for C1, without
executing a known-refused install or changing authority. No repeat discretionary approval,
different terminal, new chat or new binding is requested. Any required signing,
protected-owner or external action must first be identified from that procedure.

Both bounded owners returned terminal without timeout/termination failure; the
inner bridge timeout remains preserved. All **1,048 preservation pins matched**.
At final inventory all recorded test PIDs and matching owned invocations were
absent. No browser, WakeHost, native coding worker, tunnel or recurring automation
was started. Unrelated host processes were not stopped or certified idle.
Private payloads, journals, snapshots and detailed owner receipts stay outside
Git; source input hashes and sanitized evidence are in the verification record.

## 23:46 EST correction — observe the actual lifecycle owner

The initial source-facade doctor observation reported a supervisor registration
conflict because `Get-AutostartDefinition` correctly pins the executing facade's
own supervisor path. It was comparing a source worktree launcher against the
canonical release's registration. The canonical launcher instead reports
`SUPERVISOR_REGISTERED_MONITOR_STOPPED`; no conflicting registration was proved.
The earlier observation remains historical evidence, not a reason to overwrite
the registered task or Run entry.

The source doctor now returns UNVERIFIED / `SUPERVISOR_OBSERVER_SCOPE_MISMATCH`
and `RUN_CANONICAL_FACADE_DOCTOR` before querying registration from a different
facade scope. Same-scope registration conflicts still fail. The lifecycle owner,
registration validator, startup behavior and protected installation are unchanged.
All **152 doctor assertions pass** (the earlier 144 result remains historical).
Actual bounded read-only source/canonical readbacks independently verified both
states, with zero runtime starts, HTTP calls or repairs. The fix is source-qualified;
it was not copied into the canonical installation.
