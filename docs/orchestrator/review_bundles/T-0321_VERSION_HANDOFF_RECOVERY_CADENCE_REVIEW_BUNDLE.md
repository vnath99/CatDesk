# T-0321 — Version-handoff recovery cadence review bundle

## Outcome

Accepted at the source and host public-lifecycle boundary. Reviewed promotion
remains the only new-binary authority. ChatGPT Web has one post-promotion
operation: `./catdesk.ps1 recover`.

## Changes

| Surface | Change | Authority impact |
| --- | --- | --- |
| `scripts/catdesk-autostart-supervisor.ps1` | Recognizes the fixed `TRANSPORT_VERIFICATION_FAILED` result. | No added authority; the supervisor still calls only public `status`/`recover`. |
| Supervisor recovery loop | Stops after one explicit transport-verification failure and returns to health polling with `EXTERNAL_RUNTIME_PENDING`. | Avoids local restart bursts; does not own the tunnel/runtime. |
| README | Documents the reviewed-promotion -> public-recover sequence. | Documentation only; no new execution path. |

## Verification

All commands were run from the repository root on 2026-09-06:

```text
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-autostart-supervisor.ps1
autostart supervisor fixture tests passed

powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-catdesk-lifecycle.ps1
consumer lifecycle fixture tests passed

powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-start-catdesk-stack.ps1
canonical bootstrap behavioral tests passed

powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-promote-reviewed-catdesk-build.ps1
reviewed build promotion fixture tests passed

./catdesk.ps1 status
{"command":"status","state":"READY","detail":"local daemon and external runtime verified"}

./catdesk.ps1 recover
{"command":"recover","state":"CONNECTED_VERIFIED","detail":"canonical local daemon and external runtime verified"}
```

The host check used only the public facade. It did not change connector
configuration, credentials, tunnel runtime, or process state directly.

## Residual boundary

T-0319 remains open: this ticket does not prove that the watchdog alone restores
a deliberately lost daemon. It only makes the normal post-version-handoff and
temporary-runtime-observation path bounded and straightforward.

## Review status

This is implementation and host-lifecycle evidence, not an independent review.
