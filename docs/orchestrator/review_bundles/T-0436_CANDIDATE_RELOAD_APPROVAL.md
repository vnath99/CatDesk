# T-0436 — Exact candidate daemon-reload approval

## Scope

This review binds one already-built, non-serving CatDesk candidate to the purpose-separated daemon-reload approval authority introduced by the accepted T-0436 reload-authority repair.

No source implementation change, candidate rebuild, daemon reload, protected reviewed-build action, promotion/LKG/recovery mutation, Wake mutation, Secure MCP mutation, T-0425 resume, or Git publication is authorized or performed by this task.

## Candidate identity

- Relative path: `target-verify\t0436-reload-authority-r1\release\catdesk.exe`
- Byte length: `27455488`
- SHA-256: `fb5d5d62866789a42399e55b733b4b56a36a4c132faf690a1a3809d10c587663`
- Purpose: `catdesk-daemon-reload-approval-v1`

The candidate was built into an isolated non-serving `target-verify` target directory after the reload-authority repair review was naturally Wake-accepted and ACKed. The original build invocation timed out at the connector boundary; it was not blindly repeated. A bounded read-only measurement was then obtained through the project’s established temporary release-measurement test pattern and persisted to `target-verify/t0436-candidate-measurement.txt`. The temporary integration-test source was restored byte-for-byte immediately after the measurement attempt.

Measured output:

```text
PATH=target-verify/t0436-reload-authority-r1/release/catdesk.exe
LENGTH=27455488
SHA256=fb5d5d62866789a42399e55b733b4b56a36a4c132faf690a1a3809d10c587663
```

## Immutable approval artifact

The task writes exactly one purpose-specific canonical artifact:

`src/daemon-reload-approval-request-v1.json`

It contains:

- schemaVersion 1
- product `CatDesk`
- projectId `catdesk`
- purpose `catdesk-daemon-reload-approval-v1`
- the exact candidate relative path above
- the exact candidate SHA-256 above
- the exact candidate byte length above
- requestSha256 `b2a77d9029ff80a9e29965e2ab51a6e3f59e7162a63a8d7141c776b23b592185`

The request SHA is the SHA-256 of the canonical compact JSON encoding of the inner approval request. The artifact is intentionally candidate-specific: changing path, SHA-256, or length must invalidate daemon-reload authority.

## Required next gate

This candidate review is not itself a reload execution. After this session reaches `COMPLETED_VERIFIED`, its independent review record must:

1. be naturally delivered by dev.69 to canonical generation 22;
2. obtain durable `EXACT_USER_MESSAGE_APPENDED` receipt;
3. be independently inspected and ACKed;
4. only then be supplied as `recordId` to the repaired `catdesk_daemon_reload action=PREFLIGHT` with the exact candidate path and SHA-256;
5. use only the returned persisted confirmation token for exact `CONFIRM`.

Any path/hash/length/review-authority drift must fail closed.

## Post-reload sequence

After reviewed reload, prove serving/current parity before exactly one fresh protected V5 PREPARE/CONFIRM. Require `BUILD_ATTESTED` before reviewed promotion, durable `reviewed_promotion` LKG, supported one-command recovery, and finally SAME T-0425 / T-0429 live acceptance.
