# T-0436 — Protected V5 current-host toolchain discovery failure

## Status

DIAGNOSIS_OPEN

## Trigger

Fresh protected reviewed-build attempt `ca0ebdbb6ec64b408782cad3bb50e0e9`, created from accepted T-0435 authority and confirmed exactly once, terminated `BUILD_FAILED_OR_AMBIGUOUS` with `failureCode=REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE`, no attestation, and no Cargo failure diagnostic.

This is a new pre-Cargo-or-spawn boundary. Do not blind-retry the protected build.

## Current evidence

- The fresh attempt uses current V5 policy `CATDESK_REVIEWED_BUILD_POLICY_V5_OFFLINE_VALIDATED_CARGO_CACHE`.
- Current environment-policy digest is `fbe5c01ebe0e937e4d2684484206c3da20a9c3596978c5009652664eaa7adea7`.
- Current source can emit `REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE` while validating fixed MSVC/SDK roots/files or if Cargo process creation fails.
- `reviewed_windows_toolchain_environment` currently searches fixed Visual Studio 2022 edition roots under `C:\Program Files\Microsoft Visual Studio\2022\...`.
- Historical protected attempt `96c4e905f8e94e7da50d5e4b05cb6ec6` successfully reached `link.exe`; its preserved Cargo diagnostic identified the linker under `C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Tools\MSVC\14.50.35717\bin\HostX64\x64\link.exe`.

The root mismatch was the leading compatibility hypothesis, but current source already contains the closed `Program Files (x86)\\Microsoft Visual Studio\\18` compatibility layout. On 2026-09-28 the focused layout regression passed, and the ignored read-only current-host discovery probe passed against the real host. This disproves fixed-root discovery as the current-source failure site. The remaining leading boundary is Cargo process creation / serving-image parity: the failed protected attempt may have executed an older serving image or failed at `Command::spawn()`, both of which currently collapse to the same `REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE` vocabulary. Do not retry until that distinction is proven.

On 2026-09-29 the T-0436 independent review completed and was ACKed, and dev.69 naturally delivered the exact review event with `EXACT_USER_MESSAGE_APPENDED` at generation 22. A subsequent guarded reload attempt exposed a separate source-current contract defect before any daemon mutation: the public `catdesk_daemon_reload` MCP schema requires `action=PREFLIGHT|CONFIRM` (with `recordId` on PREFLIGHT), while the current handler still consumes the older `dryRun` shape after its compatibility branches. Both the schema-valid reviewed PREFLIGHT and the legacy dryRun shape are therefore rejected at the serving boundary. Treat this as a schema/handler parity defect requiring its own reviewed repair; do not bypass it with manual process replacement or direct protected-state edits.

## Required next work

1. Distinguish fixed-root discovery/validation failure from Cargo process-spawn failure using bounded non-secret evidence.
2. Preserve fail-closed semantics: no inherited ambient Visual Studio variables, no `vcvars*.bat`, no caller-selected toolchain paths, and no direct protected-state edits.
3. If host-layout compatibility is the defect, add only deterministic product-derived accepted roots/layouts with hostile ambiguity/reparse/missing-file regressions.
4. Run focused tests, strict Clippy, broader approved verification, and independent final review.
5. Only after accepted source authority may a fresh protected PREPARE/CONFIRM be issued.
6. Require `BUILD_ATTESTED` before reviewed promotion/LKG/recovery.
7. Keep SAME T-0425 PAUSED until reviewed serving/current parity is proven.

## 2026-09-29 reload parity diagnosis

The defect is now source-localized rather than merely inferred from MCP behavior. In `src/delegated/autonomy_supervisor.rs`, the public schema at lines 300-301 exposes only the reviewed `action=PREFLIGHT|CONFIRM` forms, but `AutonomousSupervisorV1::daemon_reload` begins at line 1077 and still falls through to `required(args, "dryRun")` at line 1157. Existing tests around lines 4457 and 4491 still exercise the legacy `dryRun` / `CATDESK_CANONICAL_RECOVERY` forms, so they do not prove the public schema reaches the handler.

The underlying bounded primitives already exist in `src/daemon_reload.rs`: `prepare_reload` at line 999 and `execute_reload` at line 1051. The reviewed repair should therefore remain narrow: normalize/validate only the public PREFLIGHT/CONFIRM keys, resolve and remeasure the acknowledged review authority on PREFLIGHT, call `prepare_reload` with the exact candidate/hash, and on CONFIRM require the persisted token plus exact candidate/hash before `execute_reload`. Legacy `dryRun` and decision aliases must fail closed at the public surface. Existing active-mutation blocking must remain ahead of both operations.

Required regressions: (a) the schema-valid PREFLIGHT shape reaches the handler, (b) CONFIRM is bound to the persisted token/path/hash, (c) unacknowledged/wrong review records fail, (d) `dryRun` and legacy decision shapes fail, and (e) active mutation still blocks reload. No reload should be executed as part of the repair review itself.

Security refinement (2026-09-29): do not implement PREFLIGHT by merely translating `action=PREFLIGHT` into the legacy `dryRun=true` branch. That branch calls `daemon_reload::prepare_reload` without consuming `recordId`, which would silently bypass the public independent-review gate. The controller already has the fail-closed `resolve_reviewed_promotion_review_authority(record_id)` primitive used by reviewed build/promotion. PREFLIGHT must resolve and remeasure that ACKed `COMPLETED_VERIFIED` authority before preparing reload. CONFIRM must consume the persisted preflight binding rather than accepting a caller-substituted review record. This closes both schema/handler parity and review-authority bypass in the same narrow repair.

Further trust-boundary refinement (2026-09-29): the generic `resolve_reviewed_promotion_review_authority(record_id)` check is necessary but is not by itself sufficient authorization to reload an arbitrary candidate. It remeasures the review session/completion outputs, but it does not bind the requested `buildPath` / `expectedSha256` to a purpose-specific reload approval. Existing core-host and ordinary-worker-pair authority paths already demonstrate the correct pattern: generic acknowledged-review authority plus one exact typed immutable completion artifact that binds the exact request. The reload repair should follow that pattern. PREFLIGHT must require a purpose-separated reload-review artifact that commits to the exact candidate SHA-256 (and any stable candidate identity needed by the closed policy), derive a reload-specific authority digest from that typed request plus generic review authority, and persist that authority in the preflight. CONFIRM must remeasure the same review, compare it to the persisted reload-specific authority, and require exact token/path/hash equality before `execute_reload`. A generic ACKed review must never authorize a different candidate merely because it is `COMPLETED_VERIFIED`. Add a hostile regression proving candidate substitution with a valid but differently bound review fails closed.

A bounded autonomous repair contract was attempted through the normal CatDesk control plane in this continuation, but the surrounding safety layer blocked creation because the task touches daemon replacement behavior. No source/protected/runtime mutation was attempted as a workaround.

## Operator status

OPERATOR ACTION: NONE.
