# Manual diagnostic storage records, version 1

Status: TESTED_SOURCE. A separately admitted tests-enabled typecheck passed; a later focused run passed all 12 synthetic tests after correcting a fixture-export placement error. No real ignored diagnostic was executed or is authorized by this document. No existing generated root is backfilled. Evidence: `docs/verification/DIAGNOSTIC_STORAGE_LIFECYCLE_SOURCE_2026_11_04.json`.

The three existing Windows manual ignored diagnostics register their newly created root before materializing source or copying dependency caches. The generic root constructors, trusted worker, command policy, source-path shape and existing stdout/Result contracts are unchanged. The implementation is compiled only under `cfg(all(test, windows))`.

Two create-new, synchronized files are written only under the new diagnostic root:

- `diagnostic-storage-start.json`
- `diagnostic-storage-outcome.json`

Each serialized file is bounded to 16 KiB. Writes use the existing retained, no-follow `ProtectedDirectoryGuard` and `write_new_regular_in` machinery. The original workspace/root guards and strong physical identities remain retained. Failure to register START refuses before the diagnostic body. Failure to write OUTCOME does not mask or change the original body Result. A crash, panic, missing, corrupt or mismatched outcome remains UNKNOWN/HOLD. There is no completion record synthesized in Drop and no PID/age/reboot-based reclamation.

After START persistence succeeds, the runner emits one bounded discovery line to stderr: `DIAGNOSTIC_STORAGE_ROOT_RELATIVE=target-verify/<fixed-family>/<32hex-runId>`. This permits an owner to pass the exact new root to a bounded reader without a directory scan. It contains no absolute path or private message; stderr write failure is ignored and never changes the original Result. Existing stdout summary schemas are preserved. The focused whitelist test also retains one small synthetic pair in its private test log for cross-language validation; it grants no live-ingestion or cleanup acceptance.

Every record has `schemaVersion=1`, `producer=CATDESK_MANUAL_DIAGNOSTIC_STORAGE_V1`, `authority=false`, `cleanupAuthorized=false`, `retentionDisposition=HOLD_OWNER_REVIEW` and `outputAllocatedBytes=null`. These records grant no release, retry, task, cleanup or retention authority, even when completion is known.

START fields use camelCase: `recordKind=START`, `runner`, `runId`, `rootFamily`, `relativeRoot`, `rootIdentity`, `workspaceIdentity`, `parentAttemptId`, `parentAttemptDigest`, `snapshotId`, `snapshotAuthorityDigest`, `snapshotManifestDigest`, `cargoLockSha256`, `cargoLockBytes`, `cargo`, `rustc`, `policySha256`, `argvSha256`, `environmentPolicySha256`, `ownerPid`, `ownerCreationTime100ns`, `observedAtUnix`, `completion=UNKNOWN`, plus the common fields above. Snapshot/lock fields come from the already validated immutable snapshot entries. Tool records contain only `sha256`, `identity` and `versionSha256`; no tool paths or fresh version execution are included.

The existing serializable strong identity type retains its original nested shape: `volume_serial_number` (u64), `file_id` (16 byte values). Preserve integer precision when reading identities and Windows FILETIME values. The run UUID is 32 lowercase hexadecimal characters. Root families are `reviewed-build-host-linker-diagnostic` or `rl`, with relative root `target-verify/<family>/<runId>`. Runners are `CACHE_SEED`, `ACTIVE_UNCLASSIFIED` and `FIXED_LINKER`.

OUTCOME fields: `recordKind=OUTCOME`, runner/run/root-family/relative-root/physical-identities/parent-attempt bindings, `startSha256`, `observedAtUnix`, `functionResult`, `failureCategory`, `cargoStarted`, `compilerExitCode`, `stderrEofVerified`, `processTreeCompletion`, plus the common fields. `startSha256` hashes the actual immutable START bytes; there is no circular self-hash. Readers must verify that hash and all binding fields before joining records or separately measured allocation evidence. Neither record measures allocation.

`functionResult` is `RETURNED_OK` or `RETURNED_ERR`; the only recorded error category is the fixed `FUNCTION_ERROR_UNCLASSIFIED`. No arbitrary error, stderr, environment, session, confirmation token, private expected contract, absolute path or unrelated nonce is serialized. A function may return Ok while successfully reproducing a compiler/cache failure; RETURNED_OK is not a compiler or seeding PASS.

`processTreeCompletion` is `NOT_STARTED`, `UNPROVEN` or `OWNED_JOB_AND_STDERR_EOF_VERIFIED`. Cache seeding starts no compiler. The fixed linker retains UNPROVEN after root exit and stderr join because those observations do not prove whole-Job quiescence. Only the active runner's existing owned-Job wait and successful stderr completion, or its existing independently confirmed cleanup branch, can supply the stronger observation. No kills, waits or retries are added for recording.

Focused tests cover bounded encoding, create-new collisions and refusal before work, source/root/attempt/lock binding, secret exclusion, raw START hash binding, immutable outcome collisions, original Result preservation, failed/missing EOF, fixed-versus-active quiescence, changed identities and panic/missing-outcome behavior. They use disposable fixtures and never invoke the real ignored diagnostics. Exact test status is recorded separately when a resource window is admitted.
