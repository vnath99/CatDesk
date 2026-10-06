# T-0431 - fixed isolated-output lock provenance (Codex)

## Classification

`FAIL_CLOSED_ISOLATED_OUTPUT_IS_SERVING`

`CARGO_BUILD_RELEASE_ISOLATED` is not appropriate for the T-0431 dev.68 Wake
verification while this process remains live. That profile writes its output
to the exact executable Windows is currently executing. This conclusion is
about the output-path collision only; it grants no release, reload, Wake, or
source authority.

## Read-only process evidence

The first requested read-only Windows API, `Win32_Process` through
`Get-CimInstance`, was denied by the worker context (`0x80041003`). The
alternate read-only `Get-Process` API succeeded and reported exactly one
`catdesk.exe` process:

```text
PID:       35464
start UTC: 2026-09-26T15:16:34.8832900Z
image:     <USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop\.catdesk\verification-targets\autonomy-release\release\catdesk.exe
```

Read-only TCP ownership lookup exposed no TCP rows for this PID in this worker
context. No endpoint was used as evidence and no transport or daemon command
was sent. The process image path is the relevant current CatDesk transport
identity for this lock-provenance question.

## Exact measurement

The live process image and the fixed isolated candidate resolve to the same
absolute path. They were separately read and measured:

```text
serving image path:  <USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop\.catdesk\verification-targets\autonomy-release\release\catdesk.exe
candidate path:      <USER_PROFILE>\OneDrive\Desktop\Projects\CatDesk-codex-loop\.catdesk\verification-targets\autonomy-release\release\catdesk.exe
path equality:       true

serving SHA-256:     732e63f362a9c1fcd0b7d3bab026233e048a02e5c6a1e3fda3c8227bd8563355
serving byte length: 27219456
candidate SHA-256:   732e63f362a9c1fcd0b7d3bab026233e048a02e5c6a1e3fda3c8227bd8563355
candidate length:    27219456
hash equality:       true
length equality:     true
```

Consequently an isolated CatDesk release build would need to remove or replace
the running process image. It must not be attempted as a Wake dev.68
verification shortcut.

## Historical comparison and source boundary

T-0324 locked-serving evidence R2 previously proved the same *path* was a
running CatDesk image, then measured:

```text
historical SHA-256:     054617d87a3994fb1367a7a45b551300f1b1ed400762363d0b7f5bc01c3cbe7f
historical byte length: 26408960
```

Both values differ from the current image. Historical T-0324 evidence
therefore cannot establish a build-to-current-source binding for the present
binary, and it is not reused as Wake or controller authority.

The current reviewed-source manifest for
`adc-t0430-r1-independent-refresh-review-20260927` identifies the current
reviewed source snapshot
`f64d6365ba13be06f91a64010c56a2a0ec88a35442db4c9d4bff43f44ad22208`, but does
not, by itself, bind that snapshot to the current executable hash. That missing
build attestation is intentionally not inferred here.

## Required bounded verification choice

For T-0431 Wake work, use the approved Wake-subcrate locked/offline build and
test surfaces that write under `wake/target`, not the fixed root isolated
CatDesk release target. A future controller build requires separate authority
and a process-safe handoff; this review does not authorize either.

## Boundaries

Only read-only process, file, hash/length, snapshot, and historical-review
inspection occurred. No process was stopped, restarted, reloaded, killed,
renamed, replaced, rebuilt, or otherwise mutated. No Wake, target, tunnel,
Secure MCP, Git, credential, or external-project operation occurred.
