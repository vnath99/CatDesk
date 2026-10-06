# T-0301 — live daemon reviewed bootstrap reconciliation

## Classification

**`OPERATOR_BOOTSTRAP_REQUIRED`**.

The live daemon is connected but pre-T-0299. The host-supplied exact response
to `catdesk_daemon_reload` with `decision: REVIEWED_BUILD_PREPARE` and the
acknowledged T-0300 review record was `buildPath is required`. That is positive
evidence that this running daemon does not implement the newer reviewed-build
compatibility decision and instead follows its legacy reload parser. The three
T-0299 lifecycle tools are absent live.

## Version-capability matrix

| Capability | Current source | Proven in old live daemon | Bootstrap consequence |
| --- | --- | --- | --- |
| `REVIEWED_BUILD_PREPARE/CONFIRM/RESULT` compatibility decisions | Yes | No — observed fallthrough to `buildPath is required` | Cannot prepare an attested candidate through the old daemon |
| `catdesk_reviewed_build` worker/attestation path | Yes | Not proven by live discovery | Must not infer support from source |
| Legacy `catdesk_daemon_reload` | Yes | Yes — `buildPath` validation observed | It can reload only an already contained/measured binary, not establish provenance |
| T-0299 fixed lifecycle MCP tools | Yes | No — supplied discovery says absent | Status/preflight cannot run until replacement |

## Legacy reload authority audit

The legacy request shape is an exact `catdesk_daemon_reload` request with
`buildPath` and boolean `dryRun`; confirmation additionally requires the exact
preflight `expectedSha256` and confirmation token. The source contract:

- canonicalizes the workspace and candidate; candidate must be an existing
  regular workspace-contained Windows `.exe`;
- hashes the candidate and optionally checks caller-supplied expected SHA-256;
- writes a short-lived (120-second) preflight bound to daemon PID, fixed MCP
  port, canonical workspace, replacement path, and hash;
- revalidates every bound field and recomputes the candidate hash at confirm;
- runs the fixed native handoff helper, while the external tunnel action is
  explicitly `none-external-tunnel-untouched`.

This legacy authority does **not** consume reviewed source snapshot identity,
independent-review record/digest, reviewed worker claim/result/attestation,
toolchain identities, or promotion authorization. It is therefore not an
accepted way to choose a T-0299 binary merely because it exists under
`target`.

## Read-only durable artifact audit

- `.catdesk/reviewed-build` control state is absent.
- `.catdesk/promotion-control` is absent.
- `target/reviewed-builds` is absent.
- The reviewed-source-snapshot root has no matching reviewed-build attempt or
  candidate chain for T-0299.
- The only restart-handoff material is historical/expired and carries neither
  a current reviewed-build attestation nor a reusable confirmation authority.
- The existing `target/release/catdesk.exe` is not a current post-T-0299
  attested candidate. It cannot be promoted or reloaded by assertion.

T-0157 recovery and T-0160 promotion authorization are accepted historical
controls; T-0169 is design history superseded by T-0181's fresh worker chain.
Their current source presence does not prove the old live daemon understands
their MCP surface, and the required current durable records are absent. None
is a non-circular live bootstrap route here.

## One exact operator-only action

From the approved reviewed-release operator context, deploy the independently
reviewed T-0299 CatDesk image through the existing fixed reviewed-image release
procedure, replacing the old daemon without caller-selected path/hash/command
inputs. Then reconnect to the current MCP transport. Do **not** use direct
`target` execution, a generic copy/launch, PowerShell/cmd, SCM, Task Scheduler,
or a raw daemon-reload request to self-bless workspace bytes.

After that one replacement, the next host ticket must rediscover the three
T-0299 tools, invoke only status/preflight with `{}`, and record the exact
lifecycle outcome before any separately authorized activation decision.

## Source changes and risk

No deterministic source defect is proven: the failure is deployment age and
missing live/durable authority, not an error in the current compatibility code.
No product source changed. The remaining risk is intentionally fail-closed:
T-0223 stays unaccepted and activation remains prohibited until the reviewed
replacement and fresh MCP evidence exist.

## Prohibited-action audit

No live reload, reviewed build, promotion, recovery, supervisor activation,
browser/wake/target/profile, Secure-MCP/tunnel, external-project, signing or
dedicated-producer, ProgramData/Task Scheduler/SCM, unrestricted-shell, or Git
action occurred.

## Verification and attribution

Read-only source/artifact inspection covered the legacy reload parser,
current reviewed-build worker/attestation chain, and accepted T-0157/T-0160/
T-0169/T-0181 closure bundles.

- `cargo test daemon_reload --all-targets --all-features` — passed: 12 focused
  legacy reload-policy tests.
- `cargo test reviewed_build --all-targets --all-features` — passed: 67 focused
  authority tests (2 explicitly ignored fixed-host harness tests).
- `cargo test stable_supervisor_lifecycle --all-targets --all-features` —
  passed: 4 T-0299 bridge tests.
- `cargo fmt --all -- --check` and `git diff --check` — passed.

Attribution is limited to the milestone, current plan, and this bundle; the
dirty worktree is preserved.
