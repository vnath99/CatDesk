# Supported production executable promotion and recovery

Current authorization: October 24, 2026 (America/New_York). The operator has
superseded the October 27 deferral and authorized bounded, qualified promotion,
recovery and production lifecycle testing now. Required evidence, Windows
ownership, independent review and producer gates remain in force. No recurring
automation, external tunnel replacement or browser-profile change follows from
this authorization.

## October 24 EDT C118 — complete promotion path and resumed-owner evidence

Fixed the two C117 findings: nested candidate paths now use the Rust producer's
forward-slash representation, and Phase1 resume retains the verified timestamp,
image path and hash needed by actual handback. The full consumer regression also
exposed an unavailable hashing cmdlet in the fixed native interpreter; promotion
now delegates to the existing shared helper using streaming framework SHA256.

18 canonical Rust tests, strict Clippy/format and both Windows PowerShell release
fixture suites passed. The expanded wire test uses an actual Rust authorization
and claim, a nested candidate, real script identity and no authorization seam.
It forces the optional hash cmdlet unavailable. Resumed metadata failed before
the fix and passes with complete identity afterward. Initial failures and fixture
loader corrections remain recorded; no isolated wire-path RED claim is made.

Production remains unchanged. Fresh independent review, the fixed normal controller
build and a genuine reviewed producer/snapshot chain are next; no October27 hold
remains. All 306 runtime and 55 historical files are exact, v77/v76 and native
sessions preserved. Source/fixture results do not establish installed rollback,
connector availability or actual Web coding. Evidence:
`docs/verification/C118_PROMOTION_INTEROPERABILITY_2026_10_24.json`.

## October 24 EDT C130 — producer gates fixed; parallel target-read regression resolved

Independent source review exposed three producer defects. Snapshot creation and
replay now check overlapping completion hashes; PREPARE/CONFIRM reuse the existing
crash-safe admission lock; output admission waits for root exit, whole-Job exit
and bounded stderr completion. Two snapshot regressions failed before the fix.
The full Rust suite also exposed Git probes held under the shared Wake-target
lock. Status probes now run outside that lock, with exact prepared-root checking
and paired binding readback retained. A delayed-project regression reproduced the
old sibling failure. The four-file follow-up source review passed its stated scope.

The complete local Rust commands, strict Clippy and format passed. Earlier failed
runs remain in the audit. A separate controller from published base `5fb1529`
passed two short read-only MCP starts (401/401/200, natural exit and cleanup),
but predates these fixes and is held. None of this is installed production proof.
All 306 development and 55 historical files remain exact; v77/v76, five project
bindings and native sessions are preserved. No recurring automation was activated.

Production remains stopped and unpromoted. Next: rebuild the corrected controller,
establish the genuine current completed-review/snapshot/producer chain, then use
the existing guarded release-pair route. This external review is not producer
authority; Sol gates remain enforced. No date hold or new operator action is
identified. Connector 429 and official runtime remain separate, unchanged
dependencies. Installed rollback and actual two-conversation Web coding remain
open. Evidence: `docs/verification/C130_PRODUCER_AND_PARALLEL_TARGET_FIXES_2026_10_24.json`.

## October 24 EDT C134 — corrected controller built and qualified; production admission open

The corrected normal-release controller is built from published source
`3f272767c96b4113902a547efea804b76cd46eae`. SHA-256:
`86f00f6e333a8988171fb30f2a1c09547c746586d339fc340a6785664acbb7cc`.
Both twelve-second read-only loopback starts passed: missing/wrong auth 401,
valid auth 200 with ten tools, natural exit and listener cleanup. All 361 pinned
development/history files remain exact; v77/v76 and project/native ownership stay
preserved. No browser, Wake, recurring automation or external runtime was started.

This is a qualified separate controller, **not a producer-attested or installed
production release**. The concrete admission gap is the legacy completed-review
path's deliberate Sol OS-isolation refusal. Native queue operator decisions and
external source-review JSON cannot replace its acknowledged review/snapshot chain.
Resolve that integration through reviewed source work; do not switch models,
fabricate legacy records, rewrite checksums or relabel a development predecessor
as trusted canonical rollback authority. No new operator action is identified.
Remote source CI remains unverified (zero exact-SHA runs/checks). Production
reconciliation, installed rollback and actual two-conversation Web coding remain
open; the October 27 date deferral stays superseded and the goal stays active.
Evidence: `docs/verification/C134_CORRECTED_CONTROLLER_QUALIFICATION_2026_10_24.json`.

## Three different release scopes

| Scope | Authority and owner | What it does not establish |
| --- | --- | --- |
| Isolated development `versions/`, `current.json`, `verification.json` | Existing `operator web-dev-runtime` commands and local operator-provided records | Canonical production promotion, producer attestation or protected LKG |
| Canonical workspace `target/release/catdesk.exe` and sidecar | Reviewed-build producer, native promotion/reconciliation worker, public `catdesk.ps1` lifecycle | Signed product-image rotation or external Secure MCP ownership |
| Signed product-root installation and external Secure MCP | Their existing fixed-policy installer/runtime owner | Permission to substitute their images, credentials or tunnel for the canonical workspace |

The current working development v77 and retained v76 remain useful qualified
development artifacts. Neither becomes a trusted production rollback release by
copying its hash into canonical metadata.

## Complete supported lifecycle

Run operator commands from the exact intended canonical workspace. The controller
must itself be a checksum-verified, reviewed source build supporting these commands;
do not select an arbitrary executable found on PATH. Placeholders below are required
observed values, not ready-to-run tokens or permission to invent evidence.

### 1. Freeze and identify reviewed Git source

Record `git rev-parse HEAD`, `git branch --show-current`, origin, tracked/index
diffs and an inventory of unrelated untracked files. Verify the exact remote SHA.
Keep normal development on `codex/web-rollover-oct17`; preserve the canonical
branch and existing histories. A canonical source fast-forward is separate from
binary promotion. Never run a default `cargo build --release` in the canonical
checkout: it can overwrite the very executable the release process must preserve.

### 2. Verify tests, CI, independent review and source provenance

Required evidence includes the source-specific test commands/results, toolchain,
review scope/findings, exact candidate build recipe, and checksums. Read exact-SHA
GitHub Actions/checks; zero results mean **CI unverified**. Record focused/local
coverage separately from full regression and remote CI. Do not silently waive a
required CI condition because local tests pass.

The production resolver in `src/delegated/autonomy_supervisor.rs` requires one
acknowledged CatDesk `COMPLETED_VERIFIED` review-inbox record, an inactive completed
session, exact approved contract, passed verification, nonempty final review,
attributable completion outputs and their remeasured hashes. Its committed byte
snapshot is validated by `src/reviewed_source_snapshot.rs`. The normal supported
workflow uses `autonomy_contract_create`, `autonomy_contract_validate`,
`autonomy_contract_approve`, session work/verification and
`autonomy_review_inbox_ack`. Approval/start requests require the actual session ID,
state version, contract decision hash and stable idempotency/approval identities.
Acknowledgement requires the exact completed `recordId`.

Existing direct-work claim/finalization APIs apply only to genuine authorized
direct work and preserve its baseline/actor evidence. They must not relabel a
failed native turn, an inconclusive report or pre-existing output as verified work.
The isolated Sol worker's host-authority guard remains enforced. Ordinary native
Codex CLI review is a separate lane; an external review report alone does not
automatically become a producer attestation or promotion authorization.

### 3. Build separately through the reviewed producer

```text
<reviewed-controller> operator reviewed-build prepare --review-record-id <acknowledged-current-review-id>
<reviewed-controller> operator reviewed-build confirm --confirmation-token <fresh-build-token>
<reviewed-controller> operator reviewed-build result
```

`src/reviewed_build.rs` validates the committed source snapshot, fixed Cargo/rustc
identity, offline locked dependency policy, protected attempt/claim/owner chain and
its isolated output. The fixed build is `cargo build --release --locked --offline`
against that snapshot, bounded to 900 seconds. The caller cannot choose a command,
toolchain, environment, output or source root through this API.

Expected states: `PREPARED`, the confirmed owned build's recorded state, then
`BUILD_ATTESTED`. Inspect the actual active generation's terminal result and
attestation. `BUILD_FAILED_OR_AMBIGUOUS`, an absent attestation, unknown ownership,
immutable-binding rejection or a live unfinished worker is not permission to retry
or copy an ordinary build into its output. Preserve failed generations and claims.

### 4. Verify the candidate's identity

Remeasure the candidate with `Get-FileHash -LiteralPath <candidate> -Algorithm
SHA256`; record length, exact contained relative path, source snapshot and producer
attempt. The native promotion preflight revalidates the complete producer
attestation, candidate hash, source snapshot and review binding. A matching
`verification.json`, executable version string or successful compilation is
insufficient. A separate normal-build candidate may be staged for examination;
mark it **not producer-attested** until the real producer gate passes.

### 5. Select the applicable supported preflight

**Matching canonical executable/sidecar:** the ordinary reviewed promotion tool is
`catdesk_reviewed_build_promotion` with `action=PREFLIGHT`, `recordId`, `buildPath`
and `expectedSha256`. Its subsequent `CONFIRM` also requires the exact fresh
`confirmationToken`, `buildPath` and `expectedSha256`; `RESULT` takes only action.
This uses `src/daemon_reload.rs` and the fixed
`scripts/promote-reviewed-catdesk-build.ps1` worker. Do not invoke its internal
`-Execute`, authorization or transaction parameters manually.

**Already divergent canonical pair with preserved operational slot-a:** use the
existing reconciliation CLI:

```text
<reviewed-controller> operator release-pair prepare --review-record-id <acknowledged-current-review-id> --build-path <attested-contained-candidate> --expected-sha256 <full-candidate-sha256>
```

This first invokes the same independent reviewed-build preflight, then measures
the divergent original binary, raw sidecar, slot-a, pointer, fixed scripts and
interpreter. Expected output is `PREFLIGHT_READY`, a fresh confirmation token and
expiry. Changed/missing review, attestation, source, candidate, fixed input, owner
or escrow evidence refuses. No manual checksum repair is part of this path.

The ordinary PowerShell script's default plan mode is read-only but requires a
matching old pair. On this host it returns `OPERATOR_ATTENTION_INVALID`; this
does not invalidate the separate reconciliation route. Do not repeatedly run
ordinary promotion against a known divergent pair.

### 6. Preserve and establish trust without inventing a predecessor

Before mutation, retain the original binary and raw sidecar, operational slot-a,
pointer, signed artifacts, projects, target histories and task journals. An old
`operational_verified` slot is preserved forensic/compensation material, not a
working trusted LKG. Candidate producer/review authority is established before
mutation; new serving LKG is committed only after the final exact owner proof.

If no trusted production predecessor exists, say so. First successful reviewed
reconstruction can establish the first trusted current release; it cannot prove
working rollback to the old divergent pair. Qualify a previous production release
through the existing validators before claiming the full previous-version rollback
acceptance. Development v76 health must not fill that gap by relabeling.

### 7. Confirm promotion once and inspect its durable result

```text
<reviewed-controller> operator release-pair confirm --confirmation-token <fresh-pair-token>
<reviewed-controller> operator release-pair result
```

The native worker owns the candidate with a suspended-child/private Windows Job.
Its fixed script `scripts/reconcile-reviewed-catdesk-release-pair.ps1` performs
Empty/Stage/Probe/Commit/Rollback phases; these are internal worker phases, not
operator alternatives. Stage escrows exact original bytes. Before durable release,
failure compensation requires complete Job exit plus empty canonical ownership.
After durable Job release, an unknown outcome remains explicit operator attention;
never launch another candidate or roll back under an unproven serving process.

Expected completed result: `RECONCILED_CANONICAL_READY`. Preserve
`authorization.json`, claim, transaction, job-release and `reconciled-release.json`.
`RESTORED_ORIGINAL_OPERATOR_ATTENTION` proves compensation to the original bytes,
not that the old mismatched backend is healthy. `CLAIMED_OUTCOME_UNPROVEN` or
`RECONCILE_UNPROVEN_OPERATOR_ATTENTION` requires inspecting the same transaction;
do not discard it or blindly confirm again.

### 8. Verify installed executable and recorded fingerprint

Measure the actual canonical path and sidecar independently. Require both values
to equal the admitted candidate's full SHA-256. Validate the resulting LKG through
`Get-CatDeskLastKnownGoodRelease` as used by the public lifecycle. Reconciliation
requires completed transaction, exact post-release serving-owner proof and eligible
slot-b. A receipt name, process start or hash-only sidecar rewrite is not acceptance.

### 9. Use the public lifecycle owner

```powershell
.\catdesk.ps1 diagnose
.\catdesk.ps1 recover
```

`diagnose` is read-only; check each reported layer rather than only its exit code.
`recover` invokes the existing canonical recovery engine, reconciles supported
pending reviewed state or a valid LKG, then uses the existing stack owner. It can
also progress GUI/Wake readiness: inspect its actual scope before execution and
keep recurring automation disabled. No external Secure MCP ownership transfers.
The supported restart sequence is `./catdesk.ps1 stop` followed by
`./catdesk.ps1 start`, with exact owner checks and post-start verification.
There is no public `restart` verb in the current parser. Inspect downstream
Wake/UI/runtime effects before invoking either operation; never use guessed
PIDs or internal stop scripts as substitutes.

### 10. Perform live production and multi-project checks

Verify listener 3200, exact PID/creation time/image path/hash, authenticated MCP
200 and unauthenticated rejection, intended catalog/workspace/store, bounded stop
and normal restart. Compare all five bindings and the additional controlled pairs,
digests/generations, predecessors, handoffs, journals and native writer ownership.
Then read official runtime status through its existing owner and make one connector
readback. Persistent HTTP 429 is a separate dependency; do not reinstall/cycle the
daemon or create a tunnel in response.

Once the actual connector serves the admitted catalog, prioritize two independent
project-bound ChatGPT Web coding conversations, useful file/test/diff evidence and
one blocked project with sibling progress. Qualify project Wake/context operations
without replaying completed canaries. Stop all test-owned browser/Wake processes;
preserve the authorized production daemon owner and record final inventory.

### 11. Publish evidence and retain the working release

Keep sensitive receipts, raw logs, machine identities, actual private conversation
URLs and authentication data in the private audit. Commit sanitized source,
milestones, handoff, acceptance, exact artifact/source hashes and real pass/fail
scope to the existing public CatDesk development branch. Audit every push and read
back its full remote SHA. Preserve previous immutable artifacts and Git history.

### 12. Rollback and failure recovery

For failed ordinary promotion, the existing transaction retains the exact prior
backup and performs at most one recovery handoff; report its actual result. For
failed pre-release reconciliation, restore the escrow only after whole-tree exit
proof. For completed releases with an independently eligible LKG, public `recover`
is the supported restoration entry point. Never modify a trusted release in place
merely to test failure: use an approved controlled failure with preservation,
identity checks and a proven trusted predecessor first. Test interrupted selection,
failed current release, bounded restoration, process ownership and state preservation
before retiring any compatibility writer. Fixture success is source qualification;
actual installed rollback and unattended operation remain separate acceptance gates.
