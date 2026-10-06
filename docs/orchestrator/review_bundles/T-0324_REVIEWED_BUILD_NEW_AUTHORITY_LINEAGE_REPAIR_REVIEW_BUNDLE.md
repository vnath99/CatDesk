# T-0324 Reviewed-Build New-Authority Lineage Repair

## Classification

`REVIEWED_BUILD_NEW_AUTHORITY_LINEAGE_REPAIRED` - subject to fresh independent final review.

## Defect and bounded design

The accepted terminal-retry implementation preserved and retried an exact
`BUILD_FAILED_OR_AMBIGUOUS` binding, but rejected a different subsequently
acknowledged review/snapshot binding as `REVIEWED_BUILD_BINDING_IMMUTABLE`.
That prevented a repaired source authority from using the existing protected
reviewed-build path.

This repair reuses the existing single reviewed-build authority, protected
RootDirectory/no-follow control root, immutable terminal audit, retry plan,
generation directory, and atomic active-generation pointer.  No new signer,
review domain, caller-selected path, tool, generation, attempt ID, token, or
approval field was introduced.

For a different binding, PREPARE now permits the transition only when the
currently selected active attempt is first revalidated against its own
committed reviewed-source snapshot and fixed toolchain, then reconstructs the
exact immutable terminal family (attempt, reserved claim, owner proof, and
`BUILD_FAILED_OR_AMBIGUOUS` result).  `BUILD_ATTESTED`, pending, claimed,
malformed, missing-owner, mismatched, and drifted families remain immutable.

The transition order is:

1. write or exact-readback the old terminal family under `terminal-history/`;
2. create an internally identified inert generation under `generations/`;
3. publish one immutable retry plan under `retry-plans/`;
4. atomically replace the one `active-generation.json` pointer last.

If interruption occurs before the last step, the immutable plan and its target
are remeasured and only the exact same authority lineage may finish pointer
publication.  The active pointer remains the sole runnable authority; an
inert orphan cannot be confirmed or run.  Old control files are neither
deleted nor overwritten.

## Replay and authority invariants

Plan recovery now opens and revalidates the stored fresh attempt and compares
its authority lineage (review session/record/authority digest, committed
snapshot identities, fixed tools, and policy) to the newly remeasured request.
It intentionally excludes the internally generated attempt ID, confirmation
token, and attempt digest from that comparison.  Therefore:

- exact new-binding replay converges on the first fresh token;
- a competing reviewed binding fails closed with
  `REVIEWED_BUILD_BINDING_IMMUTABLE`;
- every prior token is rejected because CONFIRM and RESULT resolve only the
  active generation;
- the historical audit remains create-once and exact-readback verified.

## Attributable files

- `src/reviewed_build.rs`
  - revalidates a terminal active family before a different binding can enter
    the existing retry transition;
  - resumes a durable plan before allocating a new generation;
  - binds plan replay to the fully remeasured authority lineage;
  - adds focused terminal/new-lineage, competition, old-token, and
    post-plan/pre-pointer recovery regressions.
- `docs/orchestrator/review_bundles/T-0324_REVIEWED_BUILD_NEW_AUTHORITY_LINEAGE_REPAIR_REVIEW_BUNDLE.md`

The workspace has extensive pre-existing dirty and untracked material.  This
ticket does not reset, stage, commit, or attribute it.

## Local verification

Focused regressions completed locally:

```text
cargo test terminal_failure_new_authority_lineage_is_idempotent_and_rejects_competition -- --nocapture
cargo test terminal_failure_new_authority_plan_recovers_only_exact_durable_publication -- --nocapture
cargo test terminal_audit_refuses_missing_owner_drift_and_nonterminal_families -- --nocapture
```

All selected suites passed.  The required full formatting, strict Clippy,
full test suite, and `git diff --check` also completed locally:

```text
cargo fmt --all -- --check                         PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings  PASS
cargo test --workspace --all-targets --all-features                  PASS
git diff --check                                                     PASS
```

The final full suite reported 939 unit tests; configured platform-dependent tests
remained explicitly ignored. `git diff --check` emitted only existing CRLF
working-copy warnings and no whitespace error.

Independent verification exposed one test-fixture defect after the initial
implementation: the public-PREPARE regression retained its fixture's pinned
control-root handle while asking PREPARE to acquire that same protected root.
On Windows this could block the test worker. The fixture initially released
that guard before calling the public API, but the subsequent verifier failure
continued to truncate before naming a test. The public fixture was therefore
removed: it was the only newly added host-toolchain-dependent test and was
redundant with the deterministic protected-control tests above. Those tests
exercise the transition directly, including immutable historical audit
readback, exact replay convergence, competing-binding refusal, old-token
unavailability, and durable plan recovery. This changes no product lifecycle
authority or production toolchain revalidation.

The exact contract `CARGO_TEST` profile (`cargo test`) was then rerun and
completed with exit code zero as well.

## Fresh bounded verification pass

For contract `fnv1a64:510194f886b90812`, the prior exhausted verification path
was re-audited before any change. The attributable condition was the Windows
test-only pinned `BuildControlRoot` retained by
`prepare_replaces_only_a_revalidated_terminal_authority_lineage` while it
called public PREPARE. The fixture now releases that guard before PREPARE
reopens the protected control root. No product lifecycle code or authority
semantics changed in this verification pass.

Fresh local results:

```text
cargo test                                                        PASS (exit 0)
cargo test --workspace --all-targets --all-features              PASS (exit 0)
cargo fmt --all -- --check                                       PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings  PASS
git diff --check                                                  PASS
```

The two full test commands each exercised the 939-test main unit binary and their
configured target suites to completion. The remaining authoritative review is
CatDesk's independent verification and diff capture.

To rule out terminal-dependent behavior, the exact `cargo test` profile was
also rerun with both native output handles redirected (the same non-interactive
shape as the verifier's `Command::output` call). It completed with exit code
zero and its bounded tail contained no failure. No cargo or test worker
remained afterward. The same command and the full all-target/all-feature profile
were rerun after the deterministic test-only correction and completed with exit
code zero. The earlier verifier failure was never attributed to a named test,
so this bundle does not claim an independent clean result; CatDesk must capture
the authoritative profile and diff independently.

## Prohibited-action audit and remaining boundary

No live PREPARE/CONFIRM/RESULT operation, daemon reload, worker launch,
release promotion, wake/target/tunnel change, signing, Git publication, or
external-project mutation was performed.  This is repository source/test
work only.  A later separately reviewed host action may use the normal closed
reviewed-build operator route; it must not treat this bundle as permission to
choose raw paths, hashes, tools, control files, or execution IDs.

Fresh independent final review is required.
