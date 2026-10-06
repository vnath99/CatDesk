# T-0214 — reviewed main-image bootstrap/install closure

## Root cause and result

The 2026-08-21 T-0212 operator retry established that the fixed T-0213 source image, `C:\Program Files\CatDesk\CatDesk.exe`, is absent.  T-0213 therefore correctly refuses to copy the fixed producer service image and returns the bounded image-missing outcome.

The bootstrap chicken-and-egg was evaluated against the current authority chain:

- R7C commits and validates immutable **source** snapshots; it does not attest an executable object.
- The current reviewed-build attestation schema records candidate bytes, but the worker is deliberately stopped at `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` before first output authority.  It has no accepted final-link retained-handle binding.
- Promotion/LKG records consume an already-valid producer attestation and therefore cannot independently authorize the first executable without circularly relying on the missing final-link proof.

Accordingly, there is no existing independently reviewed executable artifact that can safely bootstrap the first fixed main image.  T-0214 implements the allowed fail-closed outcome rather than blessing `target\release\catdesk.exe`, a caller hash, or an arbitrary pathname.

## Fixed policy and authority model

The new zero-parameter administrator command is:

```text
--catdesk-reviewed-main-image-bootstrap-install-fixed-policy
```

Its destination is compiled as `C:\Program Files\CatDesk\CatDesk.exe`.  The command accepts no image, path, destination, hash, review token, service/account, ACL/SDDL, command, or environment authority.  It is dispatched before the existing producer provisioning command and is absent from MCP and autonomous command surfaces.

`ReviewedMainImageBootstrapAuthorityV1` is deliberately an internal, one-shot capability containing an already-opened file, its SHA-256, length, stable Windows identity, a review-authority digest, and the compiled bootstrap-policy digest.  It contains no source pathname.  The bootstrap state machine re-measures that exact opened handle before copying handle-to-handle into a create-new fixed destination, then derives destination SHA-256, length, and stable identity from the opened destination handle.  Existing or reparse destinations are refused before creation and are never rollback targets.  A post-create failure rolls back only the newly created fixed object; rollback ambiguity returns a fixed redacted outcome.

The production resolver intentionally returns `REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_AUTHORITY_REQUIRED`.  This is the correct current result: no T-0214 code can fabricate the missing authority from a pathname, a post-hoc hash, or a pre-gate reviewed-build record.

Bounded outcomes are:

| Condition | Result |
| --- | --- |
| independently reviewed executable capability absent | `REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_AUTHORITY_REQUIRED` |
| stale/mismatched capability or existing/reparse destination | `REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_REFUSED` |
| copy/destination evidence failure after create | `REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_INSTALL_FAILED` |
| rollback cannot be proven | `REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_ROLLBACK_UNPROVEN` |
| non-administrator execution | `REVIEWED_BUILD_MAIN_IMAGE_BOOTSTRAP_ADMIN_REQUIRED` |

## Changed files

- `src/reviewed_build.rs`
- `src/main.rs`
- `docs/orchestrator/review_bundles/T-0214_T0154_R5D_R3I_REVIEWED_MAIN_IMAGE_BOOTSTRAP_INSTALL_CLOSURE_REVIEW_BUNDLE.md`

## Deterministic evidence

- `reviewed_main_image_bootstrap_is_fixed_opened_authority_only_and_rolls_back` proves fixed-policy handle-to-handle installation, destination content evidence, one-shot capability consumption, and outside-sentinel preservation.
- `reviewed_main_image_bootstrap_refuses_stale_authority_destinations_and_recovers_interrupts` covers stale/substituted source evidence, pre-existing/reparse-class destination refusal without deletion, interrupted post-create install rollback, destination evidence mismatch, and the administrator gate.
- `reviewed_main_image_bootstrap_cli_is_exact_non_mcp_and_non_authoritative` rejects extra flags, paths, hashes, destinations, services, and duplicate invocation; proves MCP/autonomous exclusion; and statically verifies the executor has no `target/release` or caller-hash authority.
- Existing dedicated-producer and replay/final-link-gate regression tests remain passing.

## Verification

- `cargo fmt --check` — passed.
- `cargo test reviewed_main_image_bootstrap -- --nocapture` — 3 passed.
- `cargo test dedicated_producer -- --nocapture` — 10 passed.
- `cargo test replay_preopen_producer_attestation_rejects_same_length_swap -- --nocapture` — passed; this provider context used its bounded trusted-toolchain environment-unavailable fixture branch.
- `cargo clippy --all-targets --all-features -- -D warnings` — passed.
- `cargo test` — 666 passed, 0 failed, 21 ignored.
- `git diff --check` — passed.

## No live mutation and next live acceptance

No bootstrap command was invoked.  No Program Files, SCM, account, ACL, service, namespace, elevation, worker, promotion, recovery, browser, Scheduler, external-project, Secure MCP, or Git mutation occurred.

Before T-0212 can be retried, an explicit operator-owned trust-root acceptance must supply a verifiable independent executable-review capability: a fixed, offline-reviewed CatDesk image accompanied by an immutable review record authenticated against a compiled product trust root.  That later authority provider must produce the internal opened-object capability, not a path or caller-supplied digest.  Only then may an administrator run the zero-parameter T-0214 bootstrap command to install the fixed main image.  The normal T-0212 sequence may then perform the existing fixed producer deployment and separately prove the actual service SID/token, IPC peer, Job membership, namespace hostile-token denial, and retained output-handle provenance.  `REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` remains in force throughout.
