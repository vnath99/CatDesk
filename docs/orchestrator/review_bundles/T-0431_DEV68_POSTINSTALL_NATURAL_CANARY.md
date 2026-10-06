# T-0431 — dev.68 post-install natural Wake canary checkpoint

## Classification

POSTINSTALL_READY_FOR_NATURAL_WAKE_ACCEPTANCE

## Installed package readback

The reviewed immutable Wake package is now installed and current as:

- version: `1.0.0-dev.68`
- directory: `1.0.0-dev.68-477a4b0f982c-247f506c99b9`
- WakeHost PID: `66212`
- host: `RUNNING`
- login: `READY`
- prior desired state preserved: `RUNNING`
- shortcut state: `READY`

Exact reviewed package artifacts:

- CatDeskWakeHost.exe SHA-256
  `477a4b0f982c0ba75510a05bc9bd356571040c5adf8183e099de4f17884f0cc9`
- CatDeskBinagotchy.exe SHA-256
  `247f506c99b9237f6fd96d2a4967b6c64ef2bd6f5136a4068937d2aa7f9024e0`
- adapter.py SHA-256
  `d7afbf5bc44144756e41f7d06454027965616531580b88bb2fc3db08be4dafb3`
- wake_bridge.py SHA-256
  `02646440f25941aead9140b62b453ef6e0b46b3869e28ffc119c6fa9531331a5`

## Canonical target

Project/Wake authority remains on Chat37 generation 20:

- URL: `https://chatgpt.com/c/6ab92046-d374-83e9-accf-6d26623923ac`
- target digest:
  `fe70f7a98a7e23796eaaa527cf01972be8881b2badc9c5f9c46edd8dacc5e0ec`

The post-install transport readback reported actionable queue depth zero. Historical
queue/stale evidence remains preserved and is not acceptance evidence.

## Accepted repair under test

dev.68 contains the accepted T-0430 response observer repair:

USER wake is submitted once; while ChatGPT Stop/Pause remains continuously active,
the same owned browser/profile/conversation waits two minutes, refreshes only the
exact canonical conversation, revalidates the exact latest USER digest after
hydration, and repeats on the same bounded cadence if Stop/Pause remains active.
Timeout Retry retains precedence, no USER re-submit is permitted, and existing
35-minute generation / 90-minute total bounds remain in force. Once deterministic
completion is proven, durable receipt persistence completes and the session-owned
browser is allowed to close.

## Natural acceptance purpose

Finalizing this bounded review is intended to generate one fresh legitimate
`review_ready` event after dev.68 activation. That event, not a manual test event,
is the live canary.

Required live proof:

1. dev.68 claims this exact event at generation 20.
2. USER wake is appended exactly once.
3. Stop/Pause becomes active during assistant generation.
4. If the UI remains active for two continuous minutes, dev.68 refreshes only the
   same canonical conversation and revalidates the exact USER turn.
5. Stop/Pause eventually clears and deterministic completion is proven.
6. Event persists terminal `SENT`.
7. Owned Selenium/Chrome closes after success.
8. No USER replay, target drift, fresh browser authority, or manual-test
   substitution occurs.

This checkpoint does not resume T-0425, change target authority, mutate Secure
MCP, or claim Wake maturity before the above live proof.
