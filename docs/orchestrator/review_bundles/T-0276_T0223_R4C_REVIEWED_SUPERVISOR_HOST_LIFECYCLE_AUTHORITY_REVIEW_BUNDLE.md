# T-0276 / T-0223-R4C — Reviewed Supervisor Host Lifecycle Authority

## Disposition

T-0274's `SUPERVISOR_ACTIVATION_SURFACE_UNAVAILABLE` is structurally closed:
CatDesk now exposes one closed local operator lifecycle composition. It is not
host-activation-ready, because no independently reviewed authority exists for
the distinct stable-supervisor image. Production returns
`REVIEWED_SUPERVISOR_IMAGE_UNAVAILABLE` before it can reach the installer.

## Exact grammar

Only these forms parse beneath the existing operator facade:

```
operator supervisor status
operator supervisor preflight
operator supervisor activate
```

Every extra word is rejected. No caller may select a ProgramData root, image,
bytes, hash, pipe, port, endpoint, SID, session, service, task, tunnel, or
policy. No MCP route was added.

| Action | Authority | Current behavior |
| --- | --- | --- |
| `status` | Fixed read-only readiness and fixed categories | No root creation, launch, port probe, pipe client, state/receipt write, worker action, or tunnel access. |
| `preflight` | Actual T-0271 principal descriptor, actual stable-runtime capability, protected readiness, fixed dry-run startup policy, image gate | Fails closed with `REVIEWED_SUPERVISOR_IMAGE_UNAVAILABLE`. |
| `activate` | One crate-internal, zero-choice fixed-writer composition | Stops before writer/startup because image authority is absent. |

## Distinct supervisor-image trust

`reviewed_build::verified_current_reviewed_main_image_digest()` remains a
worker-only bridge for ordinary `catdesk.exe`. The new lifecycle never imports
or calls it. No accepted envelope/payload binding for
`catdesk-control-plane-supervisor.exe` was found. The lifecycle therefore does
not inspect `target/release`, current directory, PATH, sibling names, or
caller-supplied hashes/bytes.

The immediate next prerequisite is one independently reviewed protected
capability returning only the exact reviewed supervisor bytes and their
accepted digest. It must not be derived from the worker image envelope.

## Lifecycle and startup boundary

The composition checks the real principal/runtime policy, the fixed protected
dry-run startup plan, distinct image authority, fixed startup classification,
and readable protected state before calling the fixed installer. After install
it rechecks protected readiness. Fixed failure vocabulary includes
`ELEVATION_REQUIRED`, `SUPERVISOR_3201_PORT_AMBIGUOUS`,
`SUPERVISOR_STARTUP_AUTHORITY_UNAVAILABLE`, root/state/receipt failures, and
the distinct-image blocker.

No accepted native startup mutator currently exists. Production has no generic
shell, PowerShell, process-launch, SCM, Scheduler, or caller-selected identity
fallback. A future accepted native helper remains separately gated.

## Preserved authorities

The only possible production writer is
`SupervisorInstallerWriterV1::fixed_policy()` and can receive bytes/digest
only from distinct supervisor-image authority. The existing T-0273 pinned
no-follow installer and T-0275 unique temp/staging, idempotency, current/LKG,
and post-install readiness properties are unchanged.

The lifecycle reuses the actual T-0271 pipe descriptor: supervisor-derived
TokenUser plus TokenSessionId admission occurs before control-record decoding,
with existing downstream image/manifest binding. It reuses the stable runtime
capability restricting ownership to fixed local 3201, fixed private pipe, and
protected state/install lifecycle. It neither manages Secure MCP/tunnel nor
changes the ordinary 3200 worker or browser/wake behavior.

## Deterministic test matrix

- exact grammar succeeds; extra path/hash/pipe/service/tunnel tokens refuse;
- missing distinct image blocks production preflight and activation;
- policy, startup-unavailable, elevation, port ambiguity, and receipt/state
  reasons classify deterministically;
- rejected image/policy/startup/malformed-digest/missing-state fixtures do not
  create or change the protected root;
- isolated exact-image installation is idempotent, retains LKG after update,
  and rechecks readiness;
- source regressions reject worker-trust and generic process authority;
- existing T-0275 crash-residue tests remain in the full suite.

## Attributable files

- `src/supervisor_lifecycle.rs` — closed lifecycle, outcomes, test seams and
  source guards.
- `src/operator_facade.rs` — exact supervisor dispatch; no workspace authority
  is used for supervisor actions.
- `src/main.rs` — private lifecycle module declaration.
- `src/control_plane_supervisor.rs` — crate-visible test-only installer and
  readiness seams; production installer remains crate-private.
- this bundle.

The inherited worktree was broadly dirty/untracked. No unrelated file was
cleaned, reset, or attributed.

## Verification

After the final source edit, all completed with exit code zero:

- `cargo fmt --all -- --check`
- focused `cargo test supervisor_lifecycle --all-targets --all-features -- --nocapture`
  (7 focused tests passed)
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features` (809 main tests passed; existing
  opt-in advisor tests remained ignored)
- `cargo build --all-targets --all-features`
- `cargo check --release --bin catdesk-control-plane-supervisor`
- authority searches and `git diff --check`

No repository `rust_full` or `verify_project` command surface was found. No
CatDesk MCP tool was available or used. Cargo's existing
`could not canonicalize path <USER_PROFILE>
LF-to-CRLF warnings did not affect successful command results.

## Prohibited actions not performed

No live ProgramData, port 3201, pipe, SCM, Scheduler, worker/release,
browser/wake, Secure-MCP/tunnel, or real `.catdesk` mutation occurred. No Git
publication, signing, provenance, or dedicated-producer work occurred.

## Independent review boundary

`REVIEWED_SUPERVISOR_IMAGE_UNAVAILABLE` is the exact immediate prerequisite.
After distinct supervisor-image authority is independently accepted, the fixed
native startup authority must still be reviewed before a fresh T-0274 host-live
attempt. Source/controller green alone is not host acceptance.
