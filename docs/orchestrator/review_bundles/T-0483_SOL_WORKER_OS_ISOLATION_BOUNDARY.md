# T-0483 — Sol worker OS isolation boundary

## Source increment

The Sol/medium Codex CLI launch now has two independent preconditions. The
existing path check rejects a worker directory that intersects any host
authority root. A second gate rejects the ordinary `tokio::Command` CLI launch
even when those paths are disjoint, because it still runs with CatDesk's
interactive Windows user token and can write the host's `.catdesk` authority.
The runtime applies the same combined check before Codex preflight and
persists a `WAITING_FOR_CHATGPT` attention outcome. The CLI repeats it
immediately before spawning, so a missed runtime preflight cannot launch a
Sol turn. Terra/high compatibility is unchanged.

The gate has no positive branch yet. This is deliberate: a path-only fixture
cannot be treated as an OS write boundary. The negative test constructs
disjoint sibling roots, proves the path check passes, then proves the direct
CLI launch still refuses before creating an active provider turn.

## Existing isolation evidence

T-0204 reached the real AppContainer child on this CatDesk Windows host, but
its fixed output creation failed with Win32 error 5. The reviewed-build
AppContainer launch remains test-only and returns unproven even on later
success until token, ACL, integrity, privilege, output-handle, and hostile
operation proof is complete. T-0203's dedicated service principal protocol is
also test-only; its fixed service was not provisioned. Neither primitive can
be imported as a positive Codex worker capability today.

## Functional migration required

1. A trusted host prepares an immutable, SHA-bound source snapshot at an exact
   approved commit and task contract. Host authority, `.catdesk`, user auth,
   browser profile, Wake state, and writable Git metadata stay outside the
   worker namespace. Reject reparse points, hardlinks, unexpected paths, and
   changed baseline identities.
2. A pre-provisioned noninteractive service or separate account owns a fresh
   worker root. The worker token must lack interactive-user ownership, write,
   delete, `WRITE_DAC`, `WRITE_OWNER`, parent `DELETE_CHILD`, and ownership
   recovery against every host authority root. Inspect the actual token,
   object descriptors, privileges, and Job membership from pinned handles.
   Codex authentication for that identity needs its own explicit provisioned
   route; inheriting the host's `CODEX_HOME` would defeat the boundary.
3. The service launches the exact Codex binary with fixed Sol/medium settings
   and a bounded task. It owns process cancellation and captures native-turn
   model/effort evidence. A host request cannot choose the executable,
   sandbox, account, writable root, or authority paths.
4. The service inventories changed regular files against the baseline and
   hands the host an exact, one-shot, handle-bound output manifest. The host
   verifies allowed paths and bytes, stages them into the canonical workspace
   through reviewed host-owned writes, then runs the current contract verifier
   and immutable source snapshot/review binding. Any stale handle, mixed
   baseline, reparse, extra output, owner drift, or ambiguous process state
   stops before host staging.
5. Windows adversarial tests must prove the real worker cannot edit or rename
   `.catdesk`, Codex auth, Wake/store authority, or an outside sentinel;
   cannot reacquire access through owner rights or privileges; and cannot
   replace a handed-off output after verification. Exercise cancellation,
   crash recovery, replay, and source rebase. Only then may the second gate
   gain an internal positive capability minted by the verified launcher.

The current `ContractVerifierV1` runs commands and Git diff in the canonical
contract workspace. Its implementation therefore cannot simply be pointed at
the service workspace: a trusted host staging step must precede verification,
and controller recovery must durably bind that staged output to the exact
native Codex turn. Until those components exist, the Sol cycle remains held.
