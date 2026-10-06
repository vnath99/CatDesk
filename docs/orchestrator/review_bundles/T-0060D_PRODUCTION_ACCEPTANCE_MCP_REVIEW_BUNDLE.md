# T-0060D Production-Acceptance MCP Review Bundle

## Delivered boundary

`catdesk_production_acceptance` provides the fixed MCP surface for the existing
PowerShell production-acceptance checker. Its action enum is exactly
`preflight`, `capture_pre`, `capture_post`, and `compare`. The first three
require one opaque 64-hex wake-target fingerprint; compare accepts no hash,
paths, workspace, configuration, executable, timeout, shell text, or other
caller-controlled fields.

The operation canonicalizes the configured workspace and invokes only its
fixed `scripts/catdesk-production-acceptance.ps1` by direct `powershell.exe`
argument vector. It does not duplicate PowerShell acceptance-gate decisions.
JSON output is bounded and structurally validated before a compact redacted
MCP response is created.

## Evidence safety

Capture writes are limited to fixed workspace-local
`.catdesk/production-acceptance/pre.json` and `post.json`. Directory and file
reparse/symlink ambiguity is rejected. Snapshot writes use a flushed temporary
file followed by rename; pre-capture accepts only a validated `PASS` snapshot,
while post-capture preserves a valid measured snapshot for fixed-path compare.
Malformed output, failed process output, oversized output, invalid schema, and
unsafe evidence state fail closed without returning sensitive diagnostics.

## Deterministic coverage and boundaries

Rust tests cover closed action/hash parsing, fixed direct argv construction,
fixed compare evidence paths, output size/schema rejection, atomic capture,
pre-PASS gating, post measured capture, unsafe evidence locations, MCP schema
registration, and rejected-input redaction. The existing PowerShell acceptance
fixture remains the source of truth for gate behavior.

No live acceptance invocation, evidence capture, daemon reload, Scheduled Task
operation, lifecycle action, browser/wake action, tunnel action, promotion,
credential/config inspection, or Git publication occurred. CatDesk host
verification and authoritative diff capture remain pending.
