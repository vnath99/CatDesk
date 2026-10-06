# T-0060F-W4-R1 Operator Facade Review Bundle

## Delivered facade

The CatDesk executable now recognizes two closed, early-exit local actions:

```text
catdesk operator wake-target set --conversation-url https://chatgpt.com/c/<conversation-id> [--expected-current-target-sha256 <64-hex>]
catdesk operator repair-wake-bridge-environment
```

The parser rejects unknown, duplicate, missing, and extra arguments. Neither
action accepts a workspace, executable, script, profile, command, or browser
argument. Both execute before native daemon, headless-MCP, or TUI startup.

## Security and implementation boundaries

- The wake-target action calls the W4 `operator_set_wake_target` internal entry
  point. It therefore shares the exact W4 canonicalization, fixed-config-path
  safety checks, compare-and-swap, serialized update, and atomic persistence;
  no URL/config mutation logic is duplicated.
- Output is bounded JSON only. Target success returns action/status, target
  SHA-256, and the normalized host/path identity. Failure returns only
  `{"status":"attention"}`.
- The repair action has a fixed `powershell.exe -NoLogo -NoProfile
  -NonInteractive -ExecutionPolicy Bypass -File <canonical workspace
  script>` shape. It never accepts an executable or PowerShell argument.
- Before launch, the workspace, `scripts` directory, and exact
  `scripts/repair_wake_bridge_environment.ps1` target must be regular,
  non-symlink/non-reparse paths, canonical at the expected location, and below
  a bounded file size. Execution has a fixed ten-minute timeout and bounded
  capture. Success additionally requires clean exit, empty stderr, and the
  exact `WAKE_BRIDGE_ENV_REPAIRED` marker.
- No action reads browser/profile contents, touches wake state/review inbox,
  starts a listener, changes the Secure MCP tunnel or Scheduler, or modifies
  release/Git state.

## Changed paths

- `src/operator_facade.rs` — parser, closed target delegation, fixed repair
  invocation validation, result contract, and deterministic seams/tests.
- `src/main.rs` — terminal early-action dispatch before normal runtime startup.
- `src/mcp.rs` — minimal internal W4 delegation entry point; MCP behavior is
  unchanged.

## Verification evidence

- Focused `cargo test operator_facade -- --nocapture`: 4 passed.
- `cargo fmt -- --check` and `cargo clippy --all-targets -- -D warnings` passed.
- Full `cargo test`: 472 passed, 18 ignored; `git diff --check` passed.
- No live repair or target update was invoked. The existing wake venv was not
  modified, and no browser or credential surface was accessed.

## Post-review sequence

1. Build an isolated reviewed candidate; do not alter the existing W3/W4 live
   daemon during review.
2. Run that candidate's `operator repair-wake-bridge-environment` action.
3. Run its `operator wake-target set` action for the current chat, retaining
   the returned target hash for a guarded future change.
4. Generate one fresh automatic W2 canary through the normal autonomous flow.
5. Stop ChatGPT activity immediately so the exact conversation composer is
   idle, then allow the automatic dispatcher to make its bounded attempt.
