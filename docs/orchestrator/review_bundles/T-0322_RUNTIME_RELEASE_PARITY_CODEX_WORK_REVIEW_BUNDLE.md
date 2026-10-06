# T-0322 — Runtime/Release Parity + Recent Codex Recovery Review

Date: 2026-09-07
Status: IMPLEMENTATION + HOST-INDEPENDENT REVIEW COMPLETE; LIVE SOURCE→CANONICAL PARITY STILL REQUIRES A SEPARATE REVIEWED HANDOFF/OBSERVATION
Reviewer/implementer: ChatGPT direct takeover after bounded Qwen failure

## Trigger

The operator opened CatDesk and saw the older terminal menu (`Control Computer`, `Control Browser`, `Both`, `Settings`) rather than the newer native Binagotchy GUI containing the `Designated Chat URL`, `Apply/Update`, and readiness controls. At the same time the public recovery path had recently reported `READY`/`CONNECTED_VERIFIED`.

The first review question was therefore not merely “is the daemon healthy?” but “does recovery/version handoff preserve the intended reviewed product generation, and can a healthy-but-stale canonical build be accidentally entrenched?”

## Qwen attempt and takeover

A fresh delegated run was created specifically on the operator-approved local model and settings:

- provider: Ollama
- model: `qwen3.8:27b`
- paid/cloud fallback: disabled
- tool calls required
- bounded path/command contract
- 18 turns / 80 tool calls / 3600 seconds
- bounded-read / early-concrete-progress instructions

The run made no attributable mutation and failed on turn 3 with the exact bounded provider error:

`Ollama continuation failed: model=qwen3.8:27b; turn=turn-3; historyBytes=34486; requestBytes=36580; httpStatus=500; retry=1; body={"error":"no user query found in messages"}`

Before failure it also read the entire large `.catdesk/todo.md` rather than following the requested bounded-read discipline. The run was cancelled without accepting a diff, and ChatGPT took over directly as explicitly authorized by the operator. T-0324 tracks this fresh Qwen continuation/deployment regression separately.

## What the current repository actually does

### Native GUI exists in source

`src/windows_gui.rs` defines the native Binagotchy GUI path and the fixed `--catdesk-binagotchy-gui` mode. The current GUI includes the designated ChatGPT URL draft/readback surface and the guarded Apply/Update behavior. `src/main.rs` dispatches the fixed GUI mode.

### Supported lifecycle launches the native GUI

`scripts/start-catdesk-stack.ps1::Start-CatDeskBinagotchyGui` launches the exact canonical binary with:

`--catdesk-binagotchy-gui`

when the current host is an interactive desktop. The successful recovery path calls that helper after daemon + official-runtime readiness succeeds.

Therefore the screenshot does **not**, by itself, prove that the current source still launches the old TUI. Two materially different explanations remain possible until exact host process/generation evidence is captured:

1. the canonical deployed executable is older than current repository source; or
2. the bare CatDesk executable/TUI path was launched directly rather than the supported lifecycle GUI mode.

A third, narrower lifecycle issue also exists: GUI launch is intentionally best-effort and its exception is swallowed, so `CONNECTED_VERIFIED` is transport/daemon evidence rather than proof of visible GUI launch. That is a live-acceptance/observability gap, not evidence that GUI source is absent.

## Concrete Codex regression found

The adversarial review found a more serious authority regression in the recent T-0320 recovery convergence work.

### Accepted older invariant

The accepted T-0159 provenance review explicitly established:

- native reload success is not independent review authority;
- process/path/hash/PID continuity is not review authority;
- a healthy canonical pair may be restarted without reviewed promotion evidence;
- that healthy pair must **not** thereby become LKG/rollback authority;
- LKG bootstrap/advancement must derive from durable reviewed-promotion authority or an already-valid LKG.

### T-0320-era conflicting behavior

On successful recovery, `scripts/start-catdesk-stack.ps1` performed:

`Save-CatDeskLastKnownGoodRelease -Source 'operational_verified'`

and `scripts/catdesk-release-recovery.ps1::Save-CatDeskLastKnownGoodRelease` allowed that source to create/advance the LKG slot/pointer.

That meant a merely healthy current canonical pair could become durable rollback authority solely because local MCP + external runtime were operational. In the exact class of incident that triggered this ticket, a stale-but-working canonical build could therefore be entrenched as “known good” even though no reviewed-promotion provenance had authorized it as rollback material.

This was a false safety convergence: transport health had been allowed to cross the release-provenance boundary.

## Direct correction

### `scripts/catdesk-release-recovery.ps1`

`operational_verified` is now a read/reuse-only source:

- canonical binary/fingerprint evidence is still validated first;
- an already-existing LKG is loaded;
- its hash must exactly match the current canonical pair;
- if missing or different, the fixed result is `LKG_AUTHORITY_MISSING`;
- no slot, pointer, generation, or source can be minted/advanced by operational health;
- a matching existing LKG is returned unchanged.

The reviewed-promotion path remains the write-authority path.

### `scripts/start-catdesk-stack.ps1`

The successful runtime checkpoint now treats operational LKG reuse as optional for a healthy canonical pair. A missing LKG does not make a healthy canonical daemon unrestartable, but it also cannot become rollback authority. Broken-pair restoration still requires independent durable authority.

This restores the T-0159 separation:

- **health authority:** can prove/restart the currently valid canonical pair;
- **rollback authority:** can only come from reviewed release provenance / existing valid escrow.

### `scripts/test-start-catdesk-stack.ps1`

The recovery fixture now proves:

1. operational health with no LKG returns `LKG_AUTHORITY_MISSING`;
2. it writes no recovery pointer in that case;
3. a reviewed-seeded LKG can later be re-used by operational health;
4. operational reuse preserves the same generation and `reviewed_promotion` source;
5. the reviewed LKG can restore a subsequently broken canonical pair;
6. an unrecognized source remains rejected.

### `docs/orchestrator/RECOVERY_ACCEPTANCE_TRACEABILITY.md`

The matrix no longer claims that an “operationally verified” canonical pair is eligible to become LKG authority. It now states that operational health may only reuse an already-authoritative matching LKG.

## Verification

Post-correction verification was run from the CatDesk workspace:

- `cargo fmt --check` — PASS
- `cargo test` — PASS
  - includes `tests/recovery_powershell.rs::lifecycle_and_reviewed_release_recovery_fixtures_pass`, which invokes `scripts/test-start-catdesk-stack.ps1`
- `cargo build` — PASS
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS
- `git diff --check` — PASS; only existing CRLF normalization warnings were emitted

The direct unrestricted PowerShell invocation of the fixture was correctly blocked by CatDesk shell policy, so no shell-policy bypass was used. The fixture was exercised through the normal Rust integration test path instead.

## Broader review of T-0317 through T-0321

### T-0317 — stale canonical daemon/no-listener convergence

No new authority violation was found in this bounded review. Its exact stale-daemon candidate logic remains fail-closed for foreign/multiple candidates and is covered by the recovery fixture. Keep its behavior, but do not allow its process-identity proof to substitute for reviewed-release provenance.

### T-0318 — autostart/watchdog recovery owner

The owner model remains useful for one-command/no-operator recovery. Its acceptance is runtime-ownership evidence, not proof that the recovered canonical build is the newest reviewed source generation. The watchdog must remain a health owner only.

### T-0319 — destructive recovery acceptance

Still OPEN. The existing two stop→watchdog recovery cycles and disposable stale-daemon fixture are valuable evidence but must not be promoted to independent approval automatically. This ticket should not be closed simply because T-0320/T-0321 are green.

### T-0320 — runtime-verification convergence

The local/runtime bounded-verification work itself remains useful and the external Secure MCP non-ownership behavior should be preserved. However, its operational→LKG promotion was a release-blocking provenance regression and is corrected by T-0322.

A second semantic limitation remains: `CONNECTED_VERIFIED` means canonical daemon + official external runtime are healthy. It does not prove GUI launch or source/release freshness. Documentation and later host acceptance must keep those claims distinct.

### T-0321 — version-handoff recovery cadence

Its core rule is correct: generic `recover` must not compile/promote mutable repository source; reviewed promotion is the authority for new canonical bytes. The unresolved product gap is therefore not “make recover auto-bless source.” It is to make reviewed source/build completion and canonical handoff observable and reliably convergent so a user does not mistake a healthy older canonical build for the current reviewed generation.

## Source/runtime identity model after review

The system must keep these four identities separate:

1. **Workspace source state** — mutable development tree; never deployment authority by itself.
2. **Reviewed build/candidate state** — independently reviewed/attested build output; eligible for the controlled promotion path only when its authority chain is satisfied.
3. **Canonical release state** — `target/release/catdesk.exe` plus its fingerprint/manifest; this is what public lifecycle/recover may run.
4. **Live process state** — the exact process/listener currently serving CatDesk; transport health proves this process works, not that it equals the newest repository source.

`CONNECTED_VERIFIED` establishes #4 against #3. It does not establish #1→#2→#3 freshness.

## Remaining runtime/release parity issue

The operator-visible old TUI is now classified as a **live source/canonical/launch-path parity question**, not automatically as a source bug.

A safe fix must **not** cause `recover` to build or promote arbitrary current source. The next acceptance surface should instead prove, in bounded form:

- exact canonical release SHA/review generation;
- whether the current independently reviewed candidate/source generation has been promoted;
- exact GUI mode launch/readiness when an interactive desktop is expected;
- a fixed state such as `CANONICAL_REVIEWED_RELEASE_STALE` or equivalent when the reviewed promotion chain shows a newer accepted candidate but canonical handoff has not occurred.

Only the existing reviewed promotion control plane may introduce the new bytes.

## Qwen regression discovered

The failed T-0322 delegated attempt recreates a previously “fixed” Qwen failure class (`no user query found in messages`). It must not be explained away as model quality. Either current repository history serialization regressed or, consistent with the operator-visible UI mismatch, the live daemon is older than the repository-side Qwen compatibility fixes. T-0324 must compare deployed runtime capability/identity to current source before Qwen is trusted again for implementation.

## Queue actions

- T-0322: this source correction and review bundle are complete; keep live canonical/source parity separate until host handoff evidence exists.
- T-0323: perform the broader recent-Codex acceptance sweep, concentrating on false-green repository-vs-runtime claims, GUI deployment parity, reviewed promotion handoff, and queue reconciliation.
- T-0324: reproduce/fix the fresh Qwen continuation failure or prove deployed-runtime/source mismatch.
- T-0319 remains open pending independent review.
- After the recovery/version-handoff sweep, resume the broader core sequence: stable supervisor host continuity → native GUI host acceptance → integrated T-0152 sweep → resilience soak.

## Security / operational invariants preserved

- No arbitrary current bytes were blessed.
- No canonical release or `target/release` file was mutated by this ticket.
- No live daemon reload/promotion/recovery fault injection was performed.
- No Secure MCP/tunnel ownership or configuration was changed.
- No browser/profile/target mutation or manual wake occurred.
- No Git publication occurred.
- Healthy current canonical operation remains restartable even when rollback escrow is missing.
- Broken canonical-pair rollback still requires durable reviewed authority.

## Independent-review conclusion

**ACCEPT the T-0322 source correction.** The recent recovery chain contained a real provenance regression: operational health could mint LKG rollback authority. The correction restores the previously accepted review boundary and passes the project verification path.

**DO NOT claim that the old-TUI screenshot has been conclusively attributed to a stale canonical binary.** Current repository source contains and lifecycle-launches the native GUI. Exact canonical/deployed generation and launch-path evidence must be gathered through the next bounded host/release-parity acceptance step rather than inferred.

**DO NOT treat `CONNECTED_VERIFIED` as source freshness or GUI acceptance.** It remains a transport/daemon health result.

## 2026-09-07 follow-up provenance addendum

A subsequent adversarial pass found one backward-compatibility gap left after the original T-0322 writer correction: `Read-CatDeskLkgSlot` still accepted historical manifests whose `source` was `operational_verified`. Therefore an LKG snapshot minted by an older build before T-0322 could retain rollback authority even though current code no longer creates or advances such authority.

The reader now treats `operational_verified` as a recognized legacy source but rejects it as **non-authoritative**. Unknown sources retain the existing `release recovery source is invalid` failure, while the known legacy source fails with `release recovery source is not authoritative`.

The recovery fixture now proves both required compatibility cases:

- a legacy `operational_verified` slot cannot be read or selected as trusted rollback authority; and
- if the durable pointer names a newer rejected operational slot while the alternate slot contains an older valid `reviewed_promotion` snapshot, the reviewed snapshot is still selected. Provenance therefore outranks pointer preference and generation recency.

Post-addendum verification on 2026-09-07:

- focused `cargo test --test recovery_powershell lifecycle_and_reviewed_release_recovery_fixtures_pass -- --nocapture` — PASS;
- `cargo fmt --check`, full `cargo test`, and `cargo build` through the project verification surface — PASS;
- `cargo clippy --all-targets --all-features -- -D warnings` — PASS;
- `git diff --check` — PASS with only existing working-tree LF→CRLF notices and no whitespace errors.

This closes the historical-reader portion of the T-0322 provenance defect without granting any new runtime, deployment, browser, tunnel, or Git authority.
