# T-0470 R1 — CatDesk run_command dry-run must enforce execution authorization

**Date:** 2026-10-10. **Conversation:** CatDesk_chat50, `https://chatgpt.com/c/6aca50a3-0da0-83ea-8358-dbc128c4f9ad`.  
**Status:** SOURCE CORRECTION IMPLEMENTED, LOCAL TESTED. **NOT served/reviewed for deployment**.

## Trigger

The operator expressly authorized the T-0468 read-only signed-image status diagnostic. Source-current `target\\debug\\catdesk.exe` was compiled successfully from reviewed source and the exact fixed `--catdesk-reviewed-main-image-status-fixed-policy` was used in an attempted CatDesk `run_command` call.

- `run_command({dry_run:true,command:".\\target\\debug\\catdesk.exe --catdesk-reviewed-main-image-status-fixed-policy"})` returned success-shaped `Would execute`, but this was **not an execution-authorization receipt**.
- Real invocation returned generic `INVALID_ARGUMENT` through the installed controller/plugin, with **no verified host readback**.
- The current source explains why the dry-run was misleading: `src/mcp.rs::handle_run_command` previously emitted success-shaped dry run before evaluating `command::validate_shell_safety` and `validate_shell_mode`. `validate_allowlisted_shell` permits Cargo, Git, Codex, etc., but not the CatDesk executable. The remote controller's exact rejection reason remains redacted by its old tunnel; the allowlist explanation is source-supported, not a returned parser code.

## Narrow change

In `src/mcp.rs::handle_run_command`, the dry-run branch now:

1. Calls the **existing** `command::validate_shell_safety` and refuses unsafe commands with existing `COMMAND_BLOCKED` category.
2. For commands that would actually enter the shell runner, calls the **existing** `validate_shell_mode` and refuses the exact same disallowed executable/arguments with existing `SHELL_MODE_BLOCKED` category.
3. Preserves direct-process lifecycle handling and the dedicated file listing/move intercepts; these special routes are not shell execution and are not forced into the shell allowlist.
4. Only emits `dryRun:true`/success after relevant non-mutating authorization checks pass. It does not spawn a child, run Cargo, inspect a signed host receipt, or change configured shell mode. **It adds no new executable to the allowlist.**

New async Rust regression test `mcp::tests::run_command_dry_run_must_not_claim_blocked_executable_is_allowed` creates an isolated temporary workspace/config. It verifies three blocked examples in dry-run (source-current CatDesk binary, a nested PowerShell interpreter, and shell chaining) and a permitted `git status` dry-run; no actual command is launched.

## Verification

- `cargo fmt --all` and `cargo fmt --all -- --check`: PASS.
- `cargo test --locked --offline --bin catdesk run_command_dry_run_must_not_claim_blocked_executable_is_allowed -- --nocapture`: PASS (1/1).
- `cargo test --locked --offline --bin catdesk`: PASS (**1,021 passed, zero failed, 26 ignored**), bounded terminal suite output.
- `cargo clippy --locked --offline --all-targets --all-features -- -D warnings`: PASS.
- `git diff --check`: PASS (only Windows LF/CRLF warning).
- Full GitHub Actions CI and independent source review for this new commit remain separate gates; this is not evidence of a newly serving binary or approved read-only protected host diagnostic.

## Operator gate and non-actions

The user-facing exact Windows PowerShell command remains:

```powershell
cd 'C:\Users\Volap\OneDrive\Desktop\Projects\CatDesk-codex-loop'
& '.\target\debug\catdesk.exe' '--catdesk-reviewed-main-image-status-fixed-policy'
```

This must run **in the operator's own terminal**, not through a CatDesk shell exception or synthetic test. It is one fixed read-only status invocation, not a signer, installer, rotation, promotion or Wake operation. The resulting fixed `SIGNED_MAIN_IMAGE_READBACK` line or bounded error must be returned before a signed-image epoch decision. See `T-0469_R1_SIGNED_MAIN_IMAGE_READBACK_OPERATOR_GATEWAY.md` for the exact response handling and trust separation.

No product signing key, Program Files/ProgramData receipt, installed daemon, Secure MCP tunnel, old WakeHost, Git history, unrelated historical untracked file, signed envelope, protected build or release was mutated by this source correction.
