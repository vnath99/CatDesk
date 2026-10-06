# T-0320 — Recovery Runtime-Verification Convergence Review Bundle

## Decision requested

Accept the source and host one-command-recovery boundary. This bundle is not a
claim that the separate watchdog-only T-0319 automatic-recovery drill is
complete.

## Root cause

Two false-negative readiness paths combined:

1. `Test-OfficialRuntimeVerified` collapsed all status, timeout, dynamic-health
   reference, and HTTP failures to `$false`. Bootstrap could not report the
   failed gate and previously recycled a healthy local daemon to retry an
   externally owned runtime observation.
2. `Test-LocalMcpReadiness` bounded a normal `tools/list` response to 65,536
   bytes. The actual canonical daemon returned valid initialize and required-tool
   evidence, but its ordinary generated tool catalog was 172,245 bytes, so the
   lifecycle reported `LOCAL_DAEMON_PENDING` despite a valid listener.

## Changes reviewed

- `scripts/start-catdesk-stack.ps1`
  - Adds `Get-OfficialRuntimeVerification`, retaining the boolean compatibility
    wrapper while returning only fixed redacted gates internally.
  - Classifies client absence, timeout, output overflow, command failure,
    malformed/negative status, approved dynamic health reference, `healthz`,
    and `readyz` failures.
  - Removes the unsafe branch that stopped/relaunched an already healthy
    canonical daemon merely because the external runtime had not yet verified.
  - Bounds a post-kill wait for the status child, persists a schema-1 redacted
    recovery evidence record, and raises normal MCP/tunnel capture limits from
    64 KiB to 1 MiB.
  - Retains exact canonical listener/process authority and does not stop,
    recreate, reconfigure, or take ownership of the external tunnel runtime.
- `catdesk.ps1`
  - Accepts only the fixed recovery gates and returns the gate in bounded public
    JSON; unknown gates remain generic/redacted.
- `src/openai_tunnel.rs`
  - Marks short-lived tunnel-client commands `kill_on_drop(true)` so a Tokio
    command timeout cannot orphan a status/doctor subprocess beside the
    externally owned runtime.
- PowerShell fixtures
  - Cover transient/timeout/oversized/malformed/negative status, healthz and
    readyz failures, approved dynamic health files, large healthy MCP tool
    catalogs, public gate redaction, and repeated recovery behavior.

## Safety audit

- Binary/sidecar/LKG validation is unchanged and remains fail closed.
- No reviewed-promotion authority is fabricated.
- LKG is saved only after canonical pair, local MCP, and external runtime are
  all positively verified.
- Status subprocess termination targets only the process created for that exact
  command; it is not a global tunnel-client or cloudflared kill.
- Redacted evidence contains only `schemaVersion`, UTC observation time, fixed
  state, and fixed gate—no URL, alias, path, command output, or credential.
- All new native command timing is bounded by the existing readiness deadline,
  five-second command budget, plus a two-second post-kill wait.

## Verification

- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1` — PASS.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1` — PASS.
- `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-promote-reviewed-catdesk-build.ps1` — PASS.
- `cargo test --test recovery_powershell -- --nocapture` — 2 passed.
- `cargo fmt --check` — PASS.
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS.
- `cargo test --all-targets --all-features` — PASS (897 tests; opt-in/live
  tests remain ignored by their existing contracts).

## Host acceptance evidence

1. Baseline literal `./catdesk.ps1 recover` returned `CONNECTED_VERIFIED`.
2. A second literal invocation returned `CONNECTED_VERIFIED`; it retained the
   same daemon PID and existing external tunnel-client process.
3. The destructive test first required one unique loopback listener whose
   process had the exact canonical executable path, SHA-256 sidecar hash, and
   `--catdesk-daemon` mode, plus `AUTOSTART_ENABLED`.
4. That exact daemon alone was terminated. A literal `./catdesk.ps1 recover`
   returned `CONNECTED_VERIFIED` in 5.6 seconds and produced a new local daemon
   listener while the existing external tunnel-client process remained present.
5. A final repeated literal recover returned `CONNECTED_VERIFIED` and the
   durable redacted evidence record was `CONNECTED_VERIFIED` / `READY`.

## Review outcome

Implementation self-audit found no major correctness or boundary regression:
fixed gate vocabulary is enforced at both engine and facade layers; healthy
external-runtime retries do not restart the canonical daemon; LKG trust remains
unchanged; and tunnel ownership is not expanded. This bundle is ready for an
independent repository review. No Git publication occurred.
