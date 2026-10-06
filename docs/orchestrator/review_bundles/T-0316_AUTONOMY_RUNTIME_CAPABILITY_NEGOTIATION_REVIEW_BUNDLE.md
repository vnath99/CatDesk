# T-0316 — Autonomy Runtime Capability Negotiation Review Bundle

## Scope and result

T-0316 repairs repository/source compatibility discovery only. The earlier
delegated Qwen attempt is historical provider evidence: it performed bounded
read/search work, failed with Ollama HTTP 500 `no user query found in
messages`, and produced no authoritative diff.

The candidate exposes a deterministic, bounded, non-secret runtime capability
manifest from current source. It neither deploys the connected daemon nor
asserts that the old daemon understands this capability.

## Design and source attribution

| Surface | Source | Boundary |
| --- | --- | --- |
| Authoritative profile catalog | `src/delegated/autonomous_contract.rs` | `AutonomousCommandProfileV1::catalog()` is the sole ordered profile list; manifest serialization maps directly from these enum values. |
| Manifest | `src/delegated/autonomous_contract.rs` | Versioned JSON with `schemaVersion`, `product`, `contractSchemaVersion`, and exact serialized `commandProfiles`; bounded and non-secret. |
| First-class query | `src/delegated/autonomy_supervisor.rs` | `autonomy_runtime_capabilities`, empty object only, read-only annotation, returns before supervisor open. |
| Cached-schema query | `src/delegated/autonomy_supervisor.rs` | Existing `autonomy_contract_validate` envelope accepts exactly `{"action":"RESULT"}`; any extra field or other action fails closed. |
| MCP registration test | `src/mcp.rs` | Confirms the first-class tool is listed by the normal autonomy control plane. |

The exact current ordered profile names are `CARGO_FMT`, `CARGO_CLIPPY`,
`CARGO_TEST`, `CARGO_BUILD_RELEASE`, `CARGO_BUILD_RELEASE_ISOLATED`,
`GIT_STATUS`, `GIT_DIFF`, and `APPROVED_PROJECT_TESTS`.

## Compatibility and authority semantics

The first-class and compatibility queries return byte-equivalent JSON from the
same product-derived manifest function. The existing generic MCP schema already
recognizes `action`; `RESULT` becomes query mode only when it is the sole input
field. A contract, session id, or any other co-present field is refused. Calls
without `action` follow the prior contract-validation path unchanged.

Both query paths are zero-authority: they create no contract, session, provider
turn, lease, project binding, file, Git state, or host state. The first-class
tool accepts no inputs. The manifest exposes neither credentials nor host paths,
and is constrained by fixed source data rather than caller arguments.

`CARGO_BUILD_RELEASE_ISOLATED` remains verification-only. Advertising it grants
no deployment, promotion, reviewed-image, LKG, signing, provenance, runtime,
or host-gate authority.

## Verification evidence

Focused regressions passed:

- `runtime_capability_manifest_is_catalog_derived_bounded_and_stable`
- `runtime_capabilities_are_read_only_and_compatibility_query_is_fail_closed`
- `multi_tools_list_exposes_run_command_mv_without_move_path_tool`

The approved legacy-profile verification set then passed:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-targets --all-features` — 895 tests
- `git diff --check` — exit 0; only pre-existing CRLF working-copy warnings

Neither `CARGO_BUILD_RELEASE` nor `CARGO_BUILD_RELEASE_ISOLATED` was invoked
against the old connected controller.

## Live-runtime limitation and unchanged gate truth

The connected daemon previously rejected a new-profile contract as schema
invalid. This source change does not reload or alter it, so callers must still
treat capability discovery as unavailable until a reviewed runtime that
contains this exact source is deployed through an accepted procedure.

T-0224 remains accepted. T-0223 remains `OPERATOR_BOOTSTRAP_REQUIRED`.
T-0222/T-0139, T-0152, and T-0155 remain unaccepted. The order remains
`T-0223 -> T-0222/T-0139 -> T-0152 -> T-0155`.

## Prohibited-action audit and residual risk

No daemon reload/deployment, supervisor activation, host/browser/wake/target/
tunnel action, signing/provenance/dedicated-producer work, external-project
change, release build, Git publication, or protected-state mutation occurred.

Residual risk is deployment skew: a cached client can use the compatibility
shape only after a runtime that actually contains this implementation is
reviewed and deployed. The next bounded action is independent final review of
this repository candidate; it must not infer live-controller support or start
T-0313.
