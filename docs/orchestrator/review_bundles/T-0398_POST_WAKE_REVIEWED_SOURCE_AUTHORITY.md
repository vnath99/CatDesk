# T-0398 — Post-Wake Reviewed-Source Authority

## Scope

This is a bounded documentation-only independent review after natural Wake acceptance. It does not build, promote, install, reload, mint LKG authority, mutate Wake state/config/profile, alter the external Secure MCP runtime, or publish Git.

The purpose is to let the source-current CatDesk completion path emit a fresh immutable reviewed-source snapshot that can be consumed by the protected reviewed-build workflow.

## Current authority and transport

- Canonical CatDesk control conversation: `https://chatgpt.com/c/6ab06a56-56c0-83e9-9697-9bc7c380c154`.
- Canonical project and independent Wake target digest: `1ffdbc8aa68d7b0d3b3ec695a87fb87d8c43f04c64f0b52f3d9c8880a3aa5f43`.
- Wake target generation: 13.
- CatDesk Local Tunnel v3 transport is `CONNECTED_VERIFIED`; local MCP is `READY`.
- The currently serving CatDesk daemon is the source-current isolated bootstrap image SHA-256 `8f531b09a5540dac2a0c1f02285ddb782b2062b08fe0ebf7b1fd94157a42fb3a` and reports 89 local MCP tools.
- That serving image is **bootstrap execution only**. It has not been promoted to canonical `target/release` and is not itself reviewed-promotion/LKG authority.
- The dirty worktree is intentionally preserved.

## Natural Wake acceptance reviewed

Fresh post-generation-13 review:

`review-adc-t0394-generation13-natural-wake-canary-20260920-6-independent_final_review`

T-0394 is `COMPLETED_VERIFIED` with verification `PASSED`. The exact review was generated after the generation-13 target cutoff and was naturally discovered by the independent WakeHost.

The real Wake USER message was visibly appended to the exact canonical conversation without manual diagnostic publication, `catdesk_wake_bridge_run_once`, stale replay, direct Store publication, or hourly fallback.

The initial dev.47 post-submit receipt observation failed closed as `SUBMIT_CLEARED_NO_APPEND` and retained the delivery at `SUBMITTING`. It was never downgraded or replayed.

A later non-sending, hash-only exact-target browser observation proved the expected T-0394 message digest existed exactly once and was the final user message on the canonical document. dev.48 added a bounded observe-only reconciliation path. That path cannot type, click, press Enter, submit, downgrade, or retarget; it can only advance an existing exact `SUBMITTING` journal to `SENT` after two separated exact persisted-digest observations.

Authoritative T-0394 delivery is now:

- phase: `SENT`
- receipt schema: 1
- target generation: 13
- target digest: `1ffdbc8aa68d7b0d3b3ec695a87fb87d8c43f04c64f0b52f3d9c8880a3aa5f43`
- message digest: `bdbf245eae07b53d927855889e710b835a535ec592acc0b69cf74c2a8222634e`
- sent UTC: `1790022466`
- evidence: `EXACT_USER_MESSAGE_APPENDED`

The exact T-0394 final review is acknowledged. Natural Wake acceptance is therefore complete.

## dev.47 / dev.48 source review

### dev.47 browser profile boundary

The production-only authentication defect was traced to `Store::open()` canonicalizing the Wake root on Windows to a verbatim path such as `\\?\C:\...\CatDeskWake`, after which dev.46 passed that form to SeleniumBase/Chrome as `--user-data-dir`.

Same-runtime evidence showed:

- historical workspace profile: `LOGIN_REQUIRED`
- independent `%LOCALAPPDATA%\CatDeskWake\browser-profile`: `EDITOR_READY`
- exact immutable installed adapter launched independently: `SAME_CONVERSATION:READY`

Live process evidence showed the full host used the verbatim `--user-data-dir=\\?\C:\...\browser-profile` form.

The dev.47 repair preserves the canonical/verbatim path for filesystem authority, hashing, runtime selection, and logs, while converting only a local verbatim drive root to ordinary Win32 form before the browser-process boundary. Ordinary absolute paths are preserved and non-local verbatim roots fail closed.

### dev.48 post-submit reconciliation

The dev.48 repair preserves the no-replay boundary:

- normal submission still transitions `CLAIMED -> SUBMITTING` before browser submit authorization;
- any uncertainty after that boundary remains `SUBMITTING`;
- reconciliation is observation-only;
- it requires the exact stored target/generation and exact expected message digest;
- the expected digest must occur exactly once and be the final user message in two separated observations;
- duplicate digest, target drift, malformed receipt, or failed observation remains fail-closed;
- Store reconciliation permits only `SUBMITTING -> SENT`;
- matching receipt bindings are required for event ID, target generation, target digest, message digest, positive sent time, and `EXACT_USER_MESSAGE_APPENDED`;
- the original submission owner is preserved;
- no `SUBMITTING -> CLAIMED` or resubmission path is introduced.

The immediate post-submit receipt observation window is extended to a bounded 60 seconds to reduce false ambiguity while ChatGPT persists/hydrates a newly submitted turn.

## Verification evidence

Wake dev.47 verification before deployment:

- Wake runtime tests: 18/18 PASS.
- process-tree tests: 3/3 PASS.
- protocol/store tests: 22/22 PASS.
- locked release build: PASS.

Wake dev.48 verification after the receipt-reconciliation repair:

- `cargo build --release --locked --manifest-path wake/Cargo.toml`: PASS.
- `cargo build --tests --locked --manifest-path wake/Cargo.toml`: PASS; all Rust test targets compile.
- Python adapter regression suite: 7/7 PASS, including proof that reconciliation emits a receipt without typing/submitting.
- Selected wake-bridge regression suite: 9/9 PASS, including stable post-submit receipt and observe-only reconciliation success/rejection cases.
- Direct execution of the newly added Rust reconciliation unit tests was intermittently refused by the MCP command wrapper with `INVALID_ARGUMENT` after successful test-target compilation. Therefore this review does **not** claim that those newly added Rust unit tests executed. This remains a quality follow-up and does not alter the live T-0394 acceptance evidence.

Current immutable Wake package:

- version: `1.0.0-dev.48`
- directory: `1.0.0-dev.48-89724bd1edbe-9d8dc6e1fc4a`
- WakeHost SHA-256: `89724bd1edbe36c349b4cb886773e523b786ebaad24783738aa18cdb6cd03525`

## Reviewed-source snapshot coverage

Current `src/reviewed_source_snapshot.rs::collect_release_inputs` requires:

- root `Cargo.toml`
- root `Cargo.lock`
- `wake/Cargo.toml`
- recursive root `src`
- recursive `wake/src`
- optional/validated root and Wake build scripts
- required recursively discovered Rust include inputs and other fixed release inputs

The root package declares `catdesk-wake = { path = "wake" }`, so the protected root release build is governed by the root `Cargo.lock`; a nested Wake lockfile is not a required root release input.

Existing regression coverage explicitly checks that the local Wake manifest is required and that the current workspace release inputs contain `wake/Cargo.toml`, `wake/src/lib.rs`, and `wake/src/bin/CatDeskWakeHost.rs`.

This closes the exact omission that caused the earlier protected reviewed build to fail when the snapshot lacked the local Wake crate.

## Release/recovery conclusion

The source-current isolated daemon and T-0396-R1 are bootstrap evidence only. T-0395 expired and is not promotion authority.

After this T-0398 session completes normally and its independent final review is accepted, the next authorized sequence is:

1. inspect the newly emitted immutable reviewed-source snapshot and verify Wake crate files are physically present;
2. call the protected reviewed-build workflow against that exact accepted authority;
3. require `BUILD_ATTESTED`;
4. inspect and independently accept the exact candidate evidence;
5. execute reviewed promotion to canonical release;
6. verify serving/canonical binary parity;
7. establish/read back `reviewed_promotion` LKG authority;
8. run the supported one-command recovery acceptance.

No step may substitute the temporary isolated serving image, an old `operational_verified` LKG, or mutable `target/release` bytes for reviewed promotion authority.

## Verdict

The current source is suitable to be frozen into a fresh reviewed-source snapshot for the protected reviewed-build stage, subject to CatDesk's normal completion verification and independent final-review machinery. Wake acceptance is closed; release promotion/LKG recovery remains open and is the next critical path.
