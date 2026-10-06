# T-0323 — Post-T-0322 Codex acceptance sweep

Date: 2026-09-07
Status: COMPLETE — queue/recovery acceptance sweep finished; follow-up tickets T-0324 and T-0325 remain open

## Scope

Independent repository/acceptance review of the recent recovery/version-handoff work after T-0322, with emphasis on false-green host claims, source-vs-canonical deployment parity, GUI launch semantics, one-command recovery safety, and Qwen fallback trustworthiness.

## Transport and ownership observations

- CatDesk transport is currently `CONNECTED_VERIFIED` and local MCP is `READY`.
- CatDesk attached to the already-running official tunnel-client runtime and did not create a duplicate. External Secure MCP ownership remains preserved.
- The worktree is dirty and was preserved.
- No delegated implementation run is active. T-0322 remains only in `CANCEL_REQUESTED` after the known Qwen failure and is not duplicated.
- Several older autonomy sessions still report `WAITING_FOR_CHATGPT`, `PAUSED`, or `DRAFT`, but none correspond to a currently running provider worker. They are stale control-plane state, not active implementation authority.
- Codex provider status is `OPERATOR_CONFIGURATION_REQUIRED`; therefore no repeated Codex probing or thread replacement was attempted.

## Wake-target boundary

The canonical CatDesk project registry currently binds `catdesk` to:

`https://chatgpt.com/c/6a976557-1054-83ea-b4c3-b10bb51b4800`

The exact current control chat is:

`https://chatgpt.com/c/6a9e7dc6-da64-83ea-9f95-ec2e1eccc7c5`

These are not the same target. This is a genuine guarded paired-target/operator boundary. No protected wake-target/config state was edited and no manual browser wake was invoked. Repository work may continue safely while the wake binding remains parked.

## Findings from recent Codex recovery work

### T-0317/T-0319 stale-daemon recovery

The stale-daemon recovery path correctly performs bounded candidate discovery, exact canonical path/hash/mode classification, and no-listener confirmation before attempting termination. T-0319 host evidence is useful but is still not an independent final acceptance by itself.

A remaining authority race was identified: after selecting and validating the stale canonical process, production recovery ultimately issues PID-based termination. A validated process can exit and the PID can theoretically be reused before the final mutation, so the process terminated need not be the exact process instance that was reviewed. This is narrow, concrete, and security-relevant rather than speculative architecture work.

T-0325 was therefore opened to replace the PID-only final mutation boundary with exact process-instance authority using a stable handle and/or creation-time-bound identity, with deterministic PID-reuse/replacement tests. No live destructive host test is authorized for that implementation ticket.

### T-0320/T-0321 recovery semantics

T-0322 already corrected the significant provenance regression where operational health could mint/advance LKG rollback authority. The corrected boundary is retained:

- runtime/process/transport health can prove and restart the current valid canonical pair;
- operational health may only reuse an already-matching reviewed LKG;
- new rollback authority still requires reviewed promotion/provenance;
- generic `recover` must not build/promote mutable workspace source.

`CONNECTED_VERIFIED` must continue to be interpreted only as daemon/runtime health against the canonical release, not as source freshness, reviewed-release freshness, or GUI acceptance.

### GUI/source-vs-runtime parity

Current source contains the native Binagotchy GUI and the supported interactive lifecycle launches the canonical executable with `--catdesk-binagotchy-gui`. Therefore the previously observed older terminal UI cannot be attributed to missing GUI source.

The remaining acceptance gap is deployed-release/launch-path identity: a healthy canonical executable can be older than current reviewed source, and a bare executable launch can enter the TUI path. Future T-0222 GUI host acceptance must capture exact canonical release identity and launch mode rather than inferring from repository source.

### Qwen fallback

The fresh T-0322 Qwen 3.8 attempt failed before mutation on turn 3 with Ollama HTTP 500 `no user query found in messages` after CatDesk tool-result history. Prior deterministic Qwen acceptance is therefore insufficient to claim the currently deployed continuation path is trustworthy. T-0324 remains the dedicated regression/deployment-parity ticket. Do not route implementation work back to Qwen until T-0324 closes.

## Queue disposition

1. T-0323 is complete.
2. T-0324 remains open: reconcile the fresh Qwen continuation failure and determine source regression vs stale deployed runtime.
3. T-0325 is now the highest-priority bounded recovery correctness implementation ticket because it closes a concrete destructive-process authority race without requiring operator or tunnel/browser action.
4. T-0319 remains open until its independent review incorporates the T-0325 process-instance boundary.
5. After recovery/version-handoff hardening, resume the broader core order: stable supervisor host continuity, deployed native GUI acceptance, integrated acceptance sweep, then resilience soak.

## Safety conclusion

No external Secure MCP mutation, browser wake, target mutation, daemon fault injection, canonical promotion, Git publication, Scheduler/service mutation, or arbitrary shell authority was used in this sweep.

Independent conclusion: **accept T-0323 as complete and proceed with T-0325 while T-0324 remains a separate Qwen/runtime-parity workstream.**
