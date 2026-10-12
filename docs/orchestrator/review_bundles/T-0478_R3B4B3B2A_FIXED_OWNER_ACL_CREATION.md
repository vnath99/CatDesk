# T0478 R3B4-B3B2A — Fixed owner-token ACL at disposable fixture creation

Date: 2026-10-11. Canonical CatDesk Chat52: https://chatgpt.com/c/6acba3bf-e740-8331-8896-8a25fefdbf16.

**Status: SOURCE-ONLY implementation; local Windows tests PASS; independent CI pending. NOT a reviewed deletion control or active developer worker.**

## Prior CI baseline

- B3B1 non-destructive retirement inventory initial `a438d16`, missing leaf `2757537`, external hardlink `d9233f5`, and cleanup contract `61e01c3` independently completed GitHub Windows CI all 3 jobs SUCCESS.
- Paired Chat52 project and independent WakeHost target SHA `d82593bf8a7eb82936cd63d1ed52f77243c46afa3f9aa57ca2d9a642a31b73ce`, generation33; most recent manual `manual-wake-mcp-1791760868717` exact SENT/EXACT_USER_MESSAGE_APPENDED/COMPLETE. Hourly deadman disabled.
- GitHub mirroring is COMPLETE, but production Recovery still `SUPERVISOR_ROOT_UNAVAILABLE`, `SUPERVISOR_STARTUP_POLICY_UNPROVEN`, startup definition read failed, port3201 unprobed. No signer/T0366 or external Secure MCP action.

## Fixed-policy ownership creation

Existing `windows_protected_fs.rs` already owned a Windows native handle-rooted directory creator with a caller-supplied security descriptor, but only in `#[cfg(all(test, windows))]`. The B3B2A modification keeps the generic SDDL-taking helper **private** (not a new callable command or public runtime API) while making its OS-level core available to one new fixed-policy method:

- `ProtectedDirectoryGuard::create_owner_only_fixture_child(component, label)` accepts only a single directly pinned parent equal to the canonical OS temp directory, a fresh `catdesk-dev-fixture-<32 lowercase hexadecimal UUID>` component, and no caller-provided SID/ACL. Existing directory adoption is refused via native `FILE_CREATE` and no-follow child creation under `OBJECT_ATTRIBUTES.RootDirectory`.
- Derives the SID from the **current Windows process token**, using existing `windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid`. It fails closed for noninteractive or service/LocalSystem tokens and never uses `USERNAME`, `USERPROFILE`, arbitrary JSON, environment-supplied SID, caller SDDL, or a shell to choose an identity.
- Private deterministic `fixed_fixture_owner_sddl` rejects malformed/control/oversized or LocalSystem SID syntax. It forms `O:<derived SID>G:<derived SID>D:P(A;OICI;FA;;;<derived SID>)(A;OICI;FA;;;SY)`: protected DACL, with inheritable access only to the actual user and Windows LocalSystem; parent inherited ACEs do not silently widen authority.
- Uses the preexisting `security_descriptor_from_sddl` and `PinnedDirectory::create_relative_with_sddl` implementation; the low-level SDDL method is private outside `windows_protected_fs.rs`. The resulting pinned child identity remains retained in the `ProtectedDirectoryGuard`.
- The dormant `stage_witnessed_candidate_snapshot` now invokes this fixed directory creator rather than general `create_child` only on Windows; it still needs an internally minted current GitHub branch/CI witness and read-only fixed tracked Git-object bytes. On non-Windows it fails closed. This is not exposed to MCP or an independent worker.

## Local negative tests

- Fixed SDDL is deterministic and includes no Everyone, Builtin Users, or Authenticated Users broad ACEs. Invalid SID/control-SDDL injection, malformed/empty fields and LocalSystem identity are denied.
- The Windows test checks non-generated names refused, generated-root identity accepted with ordinary interactive user token, repeated creation of an already-owned root denied, and nested/foreign parent forbidden. If the process is noninteractive (e.g., some service CI runners) then the test requires refusal and proves no directory was created, rather than silently granting service identity.
- Existing fixture-staging, SHA/hash, hardlink/external-file and manifest retirement tests continue to pass. Tests clean up only their own newly generated empty temp paths.

## Verification and limitations

- Local `cargo fmt --check` PASS, focused Windows fixture tests **5/5**, fixed SDDL policy test **1/1**, full `cargo test --locked --offline --bin catdesk` exit0, `cargo check --locked --offline --bin catdesk` PASS and strict `cargo clippy --locked --offline --bin catdesk -- -D warnings` PASS.
- Workspace-wide all-target Clippy and all-target `cargo check` intermittently rejected by the installed CatDesk command gateway with `INVALID_ARGUMENT`, not an observed compiler failure. Independent GitHub Windows Rust, Python Wake/advisor and independent WakeHost Rust jobs must all finish SUCCESS for this exact source before B3B2A acceptance.
- **Not yet proven:** native security descriptor/owner read-back from the newly opened directory handle; ACL behavior under all Windows logon/service identities; owner-bound token lease and secure crash/partial-stage cleanup. The positive directory-creation test is conditional on a real interactive user token. A malicious process running as the *same* Windows user can still potentially act with owner authority; this ACL is not a same-user security boundary or Windows sandbox.
- **Absolutely no native deletion enabled**. Existing `reviewed_source_snapshot.rs::cleanup_staging_tree` is too broad for this fixture purpose and must not be called as a drop-in recursive cleanup. B3B2B must implement an identity-checked, owned-file allowlist and handle-only native disposition, including foreign/reparse/hardlink/owner-race tests, before any active launcher/retirement.

Next: B3B2B exact descriptor verification and reviewed handle-only fail-closed cleanup, then C bind the in-memory `StagedCandidateSnapshot` to the existing isolated MCP read/search router without broadening authority, D bound owned PID/port and safe restart, T0479 stable versioned worker/LKG, then production supervisor Recovery.
