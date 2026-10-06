# T-0277 / T-0223-R4C-R1 lifecycle grammar and policy-classification repair

## Disposition

T-0276 was controller-verified but independently unaccepted. This bounded repair changes only the lifecycle facade's exact grammar and its policy-result propagation. No supervisor image authority, startup authority, host activation, ProgramData state, listener, pipe, worker, browser, wake, tunnel, service, Scheduler, or Git state was mutated.

**Status: READY FOR INDEPENDENT CHATGPT REVIEW.** This is not stable-supervisor or host-live acceptance.

## Original defects and exact repair

1. `parse_supervisor_operator_action` checked the count and first token but did not require the literal middle `supervisor` token. It now requires exactly three tokens: `operator`, `supervisor`, and exactly one of `status`, `preflight`, or `activate`.
2. Lifecycle composition collapsed `principal_and_runtime_policy()` to a boolean with `.is_ok()`. `preflight_with`, `production_preflight`, `activate_fixed_authority`, and the isolated activation test seam now carry `Result<(), SupervisorLifecycleReasonV1>` and return the original precise failure.

The resulting fixed vocabulary remains distinct:

| Policy failure | Returned category |
| --- | --- |
| Fixed named-pipe principal policy cannot be proven | `SUPERVISOR_PRINCIPAL_POLICY_UNPROVEN` |
| Stable runtime ownership cannot be proven | `SUPERVISOR_RUNTIME_OWNERSHIP_UNPROVEN` |

## Direct parser evidence

The direct parser test accepts only:

- `operator supervisor status`
- `operator supervisor preflight`
- `operator supervisor activate`

It refuses missing tokens, `operator anything status`, unknown actions, extra words, and attempted caller authority including `--path`, `--hash`, `--pipe`, `--service`, `--sid`, `--session`, `--port`, `--startup`, `--policy`, and `--tunnel`.

## Preserved pre-mutation and trust boundaries

`activate_fixed_authority` still evaluates policy, the distinct reviewed-supervisor image capability, fixed startup classification, and existing protected-state precondition before calling `SupervisorInstallerWriterV1::fixed_policy().install_exact_reviewed_image`. The isolated malformed protected-state regression proves no current receipt or versions directory is created before it returns `SUPERVISOR_STATE_INVALID`.

Production still returns `REVIEWED_SUPERVISOR_IMAGE_UNAVAILABLE`: it does not reuse ordinary worker reviewed-main-image trust, build output, PATH, current directory, sibling discovery, filename identity, caller bytes, or caller hashes. Fixed startup authority remains unavailable; no SCM, Scheduler, PowerShell, cmd, or generic process authority was introduced. `status` and `preflight` remain bounded and read-only. External Secure-MCP/tunnel ownership and ordinary worker lifecycle remain outside this module.

## Verification

| Gate | Result |
| --- | --- |
| Focused `supervisor_lifecycle` tests | PASS (9 tests) |
| Focused `operator_facade::tests::parser_accepts_only_closed_operator_actions` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (811 tests; existing Python-dependent tests remain explicitly ignored) |
| `cargo build --all-targets --all-features` | PASS |
| `cargo check --release --bin catdesk-control-plane-supervisor` | PASS |
| Project `rust_full` / `verify_project` harness | Not configured/discoverable in this workspace; not inferred |
| Targeted source scan | PASS: no `policy_is_valid` or production `.is_ok()` collapse; literal middle-token guard present |
| `git diff --check` | PASS (line-ending warnings only) |

The toolchain emitted its pre-existing `could not canonicalize path C:\\Users\\Volap` warning during Cargo commands; it did not fail a gate.

## Narrow attribution

T-0277 changes are limited to:

- `src/supervisor_lifecycle.rs` — exact parser guard, typed policy propagation, and focused regressions.
- This review bundle.

The worktree is broadly dirty and the lifecycle source is an inherited untracked T-0276 surface, so a Git patch cannot separate prior T-0276 lines mechanically. No unrelated tracked or untracked file was edited for T-0277, and no clean/reset/stage/commit operation was performed.

## Residual prerequisites and independent review request

The immediate fail-closed prerequisite remains a **distinct independently reviewed supervisor-image authority**. Fixed native startup mutation authority is also intentionally absent. Those are separate future tickets; no host-live activation is authorized by this repair.

Please perform independent ChatGPT review of the exact parser and typed policy classifications before any future supervisor-image, startup, or host-live work.
