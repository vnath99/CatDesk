# T-0210 R5D-R3E — administrator operator integration

## Verdict

Added one product-owned Windows administrator CLI command, without invoking
it. T-0208's host result remains **NOT_PROVISIONED** and T-0209's fixed
administrator mutation host remains the only possible delegate. This ticket
does not claim provisioning success, READY state, producer isolation, or
reviewed-build authority.

## Exact CLI surface

The only accepted form is:

```text
catdesk --catdesk-reviewed-producer-provision-fixed-policy
```

`parse_dedicated_producer_admin_provision_args` accepts that single token only.
If it occurs with any extra positional token, option, duplicate, service name,
path, executable, SDDL/ACL text, account, credential, SCM setting, or tunnel
text, parsing fails before the administrator host is constructed. The command
does not accept values of any kind.

The main dispatch delegates to
`run_dedicated_producer_admin_provision_command`, which delegates only to
`execute_dedicated_producer_fixed_policy_as_administrator`. That T-0209 host
still requires its elevated token gate and clean fixed `ABSENT` preflight;
product constants continue to own the service, executable/mode, namespace,
restricted service-SID policy, descriptor intent, durable journal, and
rollback. An action result is explicitly
`REVIEWED_BUILD_DEDICATED_PRODUCER_PROVISIONED_PENDING_HOST_SECURITY_ACCEPTANCE`,
not producer-isolation acceptance.

The ordinary read-only operator surface still rejects `ExecuteFixedPolicy`.
The actual mutation implementation is excluded in test builds and its test
substitute returns
`REVIEWED_BUILD_DEDICATED_PRODUCER_MUTATION_HOST_UNAVAILABLE`; no test can
perform SCM/service/account/ACL mutation.

## MCP and autonomous exclusion

The flag is not a lifecycle-facade command and is absent from `src/mcp.rs` and
the autonomous command contract. It is consequently not reachable through
MCP `run_command`, lifecycle shell interception, or autonomous tool command
profiles. The direct CLI dispatch occurs only after the existing operator
facade declines the non-`operator` flag and before worker/daemon parsing.

`run_reviewed_build_worker` remains independently blocked by
`REVIEWED_BUILD_FINAL_LINK_HANDOFF_REQUIRED` before output, candidate,
attestation, promotion, or recovery authority.

## Deterministic coverage

| Test | Evidence |
| --- | --- |
| `dedicated_producer_admin_cli_is_exact_no_input_and_non_authoritative_in_tests` | exact single-token parse; service/path/SDDL/duplicate rejection; no MCP/autonomous/lifecycle recognition; test host refuses mutation |
| `administrator_fixed_mutation_host_is_no_input_and_unreachable_from_unit_tests` | no-input T-0209 host, ordinary execute refusal, test-only host unavailable result, real Win32 primitive shape |
| `windows_dedicated_backend_refuses_hostile_state_admin_bypass_and_rolls_back_in_reverse` | fake seam verifies non-admin refusal, hostile-state refusal, reverse rollback/recovery |
| `dedicated_producer_provisioning_model_cannot_authorize_reviewed_build_or_mcp_ownership` | final-link gate remains before output acquisition |

## Later administrator-run acceptance boundary (not performed)

An approved Windows administrator may explicitly run the exact one-token CLI
form above only after separately reviewing the fixed policy. The T-0209 host
must observe a clean `ABSENT` read-only preflight and elevated token, then
perform its fixed journaled service/SID/creation-time namespace operation. A
separate host-security acceptance must subsequently prove the actual service
token, namespace owner/DACL/OWNER RIGHTS/mandatory label, interactive-token
write/delete/rename/reparse/parent-`DELETE_CHILD` and ownership-recovery
denials, process/Job identity, stable FileId, and retained output handle. Only
a later integration ticket may consider that result for provenance authority.

## Verification

- `cargo test dedicated_producer_admin_cli -- --nocapture` — passed.
- `cargo test administrator_fixed_mutation_host -- --nocapture` — passed.
- `cargo fmt --check` and strict all-target/all-feature `cargo clippy` —
  passed.
- Full `cargo test` — 659 passed, 1 failed, 21 ignored. The sole failure is
  the pre-existing T-0196 replay test's provider-token Rustup failure
  (`REVIEWED_BUILD_STATE_UNAVAILABLE`); it is outside this ticket.
- `git diff --check` — passed (the workspace emitted unrelated CRLF warnings).

## Changed files

- `src/reviewed_build.rs`
- `src/main.rs`
- `docs/orchestrator/review_bundles/T-0210_T0154_R7D_R9_R5D_R3E_ADMIN_OPERATOR_INTEGRATION_REVIEW_BUNDLE.md`

No provisioning command, elevation, service/account/SCM/ACL mutation,
reviewed-build worker, daemon lifecycle mutation, Git publication, browser or
Scheduler operation, external-project operation, or Secure MCP tunnel action
was invoked.
