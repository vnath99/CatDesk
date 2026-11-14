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

## October 24 EDT C115/C116 — production authorized now; two release blockers fixed

The operator superseded the October 27 deferral. Canonical source fast-forwarded
to published `af2ad2e` without changing its executable/sidecar or ten untracked
files; the checksum-verified normal candidate was staged separately. A bounded
independent public-source review identified a stale PowerShell authorization schema
and unproven whole-Job cleanup after spawn setup failure. Both are fixed in source.
The candidate is held; the corrected source needs fresh review and producer build.

59 targeted Rust tests, strict Clippy/format and both release PowerShell fixture
suites passed. New regressions use actual Rust serialization and a real resumed
child/descendant. Production was not promoted, installed rollback was not tested,
and actual Web coding remains open. All 306 runtime and 55 historical records
remain exact; no Wake/browser/tunnel or other native-session action occurred.
The old ordinary plan correctly refuses this divergent pair. The supported route
is reviewed release-pair reconciliation once its real gates pass, with no date hold.

Complete commands, parameters, prerequisites, failure states and rollback limits:
`docs/orchestrator/CATDESK_PRODUCTION_RELEASE_LIFECYCLE.md`.
Evidence: `docs/verification/C116_PRODUCTION_REVIEW_BLOCKER_FIXES_2026_10_24.json`.

# Supported production recovery: existing authority and current authorization

Observed October 24, 2026. Classification: **READ_ONLY_HOST_HISTORY**.
Recovery, protected deployment and actual parallel Web coding remain unaccepted.
This plan uses the existing procedures; it adds no recovery mechanism or authority.

## October 24 C111 development snapshot — separate from production admission

The existing isolated development runtime now selects the normal C109 v77 package
and retains the normal v76 working predecessor. Both have exact source/build/review
references and freshly observed actual selected-image MCP health (401/200; 79 tools),
recorded through the existing write-once development verification command. Ten paired
targets and all five project bindings remain unchanged. No protected install, official
runtime, browser/Wake activation or legacy retirement occurred.

This proves development can run independently while protected restoration waits.
It supplies no missing canonical attestation, review ID, confirmation token or LKG
authority. Do not copy either development executable into the canonical installation
or use these local verification records as protected admission. The minimum operator
boundary and exact post-recovery checks below remain unchanged. Evidence:
`docs/verification/C111_SELECTED_DEVELOPMENT_CURRENT_PREVIOUS_HEALTH_2026_10_24.json`.

## Findings

### The executable/fingerprint divergence has a recorded build/reload sequence

Selected historical command logs record, in order:

1. `cargo build --release`, ending successfully at **2026-10-07 00:47:41 UTC**.
2. An exact readback at **00:48:19 UTC** of a 26,838,016-byte canonical executable,
   SHA256 `9a8f4ebf6ea57cbb61efaa12e8afa54d18b8db7374b9884067325bc84565568c`.
3. The existing native reload receipt at **00:55:18 UTC** names that same hash,
   port 3200, and `REPLACEMENT_READY_PENDING_TRANSPORT_RECONNECT`.

These events occurred October 6 in America/New_York. Nearby Git output names
`530fd67df8d60979802a06f5a8e3d52492d77057`; it does **not** prove a clean build
or that every compiled input came from that commit. The current executable still
matches the logged hash. The sidecar continues to name
`a264c8dc85fa55311e74114158f4f4cd598f6e9e9d0a6ca535e14d18aa649939`.
Its observed file timestamp is August 19 UTC, while the executable's is October 7.
File timestamps supplement the recorded commands; they are not release authority.

The evidence supports a normal release build and native handoff without completion
of the reviewed binary/sidecar/LKG transaction. It does not support a tampering
claim. A native reload receipt proves its recorded handoff, not independent review,
promotion completion, LKG eligibility or successful external reconnection.

### Historical approval and usable rollback authority are different

| Evidence | What it establishes | Current recovery eligibility |
| --- | --- | --- |
| August T-0154 | An earlier A264 executable / 532E fingerprint incident; existing trusted-pair recovery and alternating escrow were implemented. | Historical review narrative alone is not a current producer/promotion record. |
| T-0159 authority correction | Native reload, health and review documents cannot mint LKG. Migration requires an existing valid LKG or exact durable reviewed-promotion evidence. | Preserve this established rule. |
| September 7 slot-a | Retained A264 executable, matching sidecar/pointer, generation 1, source `operational_verified`. | Bytes match, but current policy rejects this source as rollback authority. Do not relabel it. |
| September T-0322 correction | Operational health may reuse matching authoritative LKG but cannot create it; later reader correction rejects legacy operational slots. | Explains why health once observed does not authorize restoration now. |
| September 21 T-0395 | A different canonical executable / A264 sidecar mismatch already existed; temporary bootstrap reload did not establish promotion/LKG. | Shows the authority problem predates recent browser cleanup. |
| October build inventory | 40 generation results plus one older result, all failed/ambiguous; no attestation files at the inspected authoritative locations. | No `BUILD_ATTESTED` candidate found. |
| Current promotion/recovery locations | No promotion-control, promotion-recovery, release-pair-control or slot-b. | No interrupted reviewed transaction or completed reviewed LKG found. |
| Prior October 23 signed-image readback | Installed signed epoch 1 verified; a separate pending epoch 2 was reported. | Distinct product-image authority; not a canonical workspace promotion/LKG receipt. No rotation is authorized by this plan. |

**Conclusion:** a historically reviewed old release is preserved, but no presently
usable independently attested/promoted canonical rollback chain was found in the
authoritative locations inspected. This is a scoped finding, not proof that every
external archive is absent. Any subsequently located record must pass the existing
validator; a matching filename, hash, signature or old approval by itself is
insufficient to change the slot's authority.

### The original shutdown cause remains unknown

The reload receipt records the replacement running in October. T-0483 R6 records
the daemon still present on October 14. The October 15 installed audit subsequently
records no daemon/listener. No shutdown-cause record was found in the inspected
lifecycle-stop directory; it is empty. This is not an exhaustive Windows crash or
security-event investigation, and absence of a record does not prove graceful exit.

The known mismatch therefore did not prevent that already-running daemon from
continuing. It now prevents a justified restart because the pair is divergent and
reviewed recovery authority is missing. Do not attribute the shutdown to WakeHost,
Chrome, cleanup, a crash or an operator without additional evidence.

The last official Secure MCP status was stopped, and the connector returned
HTTP 429. Those are separate observations. Local port-3200 restoration neither
proves external runtime recovery nor guarantees that HTTP 429 will disappear.

## Current execution boundary — October 24 authorization

The operator has superseded the October 27 deferral. Qualified bounded promotion/recovery is authorized now; all evidence and ownership gates remain. No recurring activation or external tunnel change is implied.

1. **Review the existing authority disposition.** Supply any independently preserved
   producer attestation / reviewed-promotion chain through the existing verifier.
   If none exists, use the established independent review and reviewed-build
   procedure for one frozen candidate. Do not retry the old failed generations.
   The candidate, acknowledged review ID and producer attestation are currently
   missing inputs; this plan deliberately supplies no invented replacements.
2. **Authorize the specific supported transaction and startup scope.** If a valid
   reviewed LKG/interrupted promotion becomes available, use public recovery.
   Otherwise the already implemented October 18 `operator release-pair` path is
   the applicable existing procedure for this exact divergent pair and preserved
   operational slot-a. It requires the same independent review and producer
   attestation as ordinary reviewed promotion. Its native owner proof precedes
   creation of `reviewed_reconciliation` slot-b authority. No direct script call,
   legacy reload or signed-image substitution is a shortcut.
3. **Keep external ownership separate.** If the official runtime still requires
   restoration, its existing owner must perform that action independently. Do not
   create a tunnel or change credentials. Wake/GUI startup can follow public
   recovery, so the approved scope must account for that before execution.

The public commands, from the verified canonical workspace, remain:

```powershell
.\catdesk.ps1 diagnose
.\catdesk.ps1 recover
```

`diagnose` is read-only. `recover` is held until the authority/startup conditions
above are met. A healthy pair can restart without newly minting LKG, but this
host's currently divergent pair cannot.

For the already implemented reconciliation route, the exact CLI forms are:

```text
<approved-controller> operator reviewed-build prepare --review-record-id <verified-review-id>
<approved-controller> operator reviewed-build confirm --confirmation-token <fresh-build-token>
<approved-controller> operator reviewed-build result
<approved-controller> operator release-pair prepare --review-record-id <verified-review-id> --build-path <attested-relative-candidate> --expected-sha256 <attested-sha256>
<approved-controller> operator release-pair confirm --confirmation-token <fresh-pair-token>
<approved-controller> operator release-pair result
```

These are **conditional templates**, not ready-to-run approval or a new protocol.
Run the reviewed controller from the canonical workspace; its operator commands
resolve that working directory. A qualified development executable is not itself
an approved production candidate. Prepare creates authority-bound state, so do not
use it as a read-only diagnostic. Never reuse expired tokens or replay a claimed
transaction with an uncertain result. Preserve the original files and slot-a.

## Exact post-recovery acceptance checks

1. **Release authority:** remeasure the canonical executable and sidecar; both must
   equal the approved candidate's full SHA256. Validate the resulting LKG with
   the existing reader. For reconciliation, require the completed transaction,
   exact post-Job-release serving-owner proof, `reconciled-release.json`, and
   eligible slot-b. A successful build or native reload receipt is insufficient.
2. **Process and local protocol:** verify one expected listener on port 3200;
   bind its PID, creation time, executable path and image hash to the approved
   canonical daemon. Public `diagnose` must pass local config, release identity,
   local daemon and local MCP protocol. An exit code of zero only means valid
   diagnostic output. Record the fixed recovery result and each failed layer.
3. **Serving data and tools:** verify the actual serving workspace/store and its
   intended project registrations. The stopped development control and canonical
   store are not interchangeable. Confirm the scoped task transport in the actual
   serving catalog: direct assigned-task tool or the qualified closed cached
   envelope. Reject stale images or a healthy endpoint lacking the required tool.
4. **State preservation:** compare all five original project workspace/URL/digest/
   generation bindings, predecessors, handoffs, pending unknown outcomes and
   task ownership against the saved baseline. Include the extra controlled pairs
   and retired history; permit only the explicitly approved recovery receipts.
   Do not migrate a store or recreate registrations to make this check pass.
5. **External path:** independently read the existing official-runtime status and
   make one bounded connector readback after the applicable backoff. If it still
   returns 429, record that dependency without cycling the local daemon or tunnel.
6. **Actual Web B/C first:** once the serving path passes, use two project-bound
   ChatGPT Web conversations and isolated assigned task worktrees for real tool
   work. Require actual MCP call journals and useful file/test/diff evidence;
   show one blocked project leaves its sibling working. Existing Codex writers
   retain their original checkouts and tasks. Reuse the prior scope/ownership
   controls; synthetic receipts or delivered prompts do not pass this gate.
7. **Bounded cleanup:** stop only test-owned foreground processes, verify their
   exact owners exited, and preserve the authorized official runtime owner.
   Do not activate recurring automation or replay any completed canary.

## Evidence and publication scope

See `docs/verification/C100_HISTORICAL_RECOVERY_AUTHORITY_2026_10_24.json`.
The private audit retains exact historical file/log digests and location mappings.
This investigation changed no product source or runtime. Source tests were not
repeated. All 306 tracked runtime records and 55 inspected historical files stayed
exact. Protected signed artifacts and native project sessions were not modified.
