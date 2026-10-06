# T-0269 / T-0152-R2A-R8C pinned same-parent rename authority ownership

## Result

**PASS** for the bounded R2A-R8C extraction.

The reusable same-parent native rename authority is now owned by `src/windows_protected_fs.rs` as `ProtectedDirectoryGuard::rename_direct_child_within_parent`. `src/reviewed_source_snapshot.rs` retains the snapshot-specific commit/racing-winner/staging policy and delegates only the exact-handle native rename to that shared primitive.

No enumeration, disposition/delete, recursive cleanup/reclaim, snapshot validation policy, stable-supervisor/R2B, named-pipe, wake/browser, lifecycle/release, Scheduler, Git publication, signing/provenance, dedicated-producer, or Secure-MCP authority moved in this ticket.

## Worker history

A bounded local `qwen3.8:27b` delegated run was created for T-0269 with only the two Rust modules and this exact review bundle writable. The worker performed four reads and then failed before any product mutation on turn 5 with:

`Ollama continuation failed ... httpStatus=500 ... {"error":"no user query found in messages"}`

The failed run did not provide implementation or acceptance evidence. ChatGPT therefore completed the bounded ticket directly through CatDesk and independently ran the acceptance gates below. The Qwen continuation failure is a separate worker-reliability signal and is not hidden by this ticket result.

## Before / after ownership

### Before T-0269

`src/reviewed_source_snapshot.rs::rename_pinned_staging` directly:

- validated the destination component;
- validated the staging and snapshot-root guards;
- obtained the exact already-open staging/source and destination-root handles;
- constructed native `FILE_RENAME_INFORMATION` bytes;
- called `NtSetInformationFile(FileRenameInformation)`;
- updated the guard path bookkeeping through the shared `mark_renamed_as_direct_child` compatibility seam.

The snapshot module therefore still physically owned reusable native rename authority after T-0268.

### After T-0269

`src/windows_protected_fs.rs` now owns:

- `FILE_RENAME_INFO_CLASS` for the reusable rename primitive;
- the shared-module `NtSetInformationFile` declaration needed by that primitive;
- `ProtectedDirectoryGuard::rename_direct_child_within_parent`;
- private post-success `mark_renamed_as_direct_child` bookkeeping.

The shared rename primitive:

1. validates exactly one protected destination component;
2. validates the complete source guard and pinned destination parent;
3. rejects a source that is not the recorded direct child of that exact pinned parent **before mutation**;
4. obtains the exact already-open source handle;
5. clones and identity-validates the pinned destination-parent chain, then uses its duplicated final handle as native `RootDirectory`;
6. constructs `FILE_RENAME_INFORMATION` with `ReplaceIfExists == false` and the validated UTF-16 child name;
7. calls `NtSetInformationFile(FileRenameInformation)` on the exact source handle;
8. only after native success updates local guard path bookkeeping;
9. revalidates the renamed source guard and destination parent.

No mutable caller-supplied pathname is opened to obtain source or destination mutation authority.

`src/reviewed_source_snapshot.rs::rename_pinned_staging` is now a thin snapshot-policy adapter that calls:

`staging.rename_direct_child_within_parent(destination_root, destination_name, "reviewed source snapshot commit")`

The surrounding snapshot caller still owns the racing-winner rule: a rename failure is accepted only if `validate_committed_snapshot` proves the same exact committed authority. Staging never becomes authority merely because the native rename was attempted.

## Important retained snapshot authority

`src/reviewed_source_snapshot.rs` intentionally still declares/uses `NtSetInformationFile` for **disposition/delete**. T-0269 does not attempt to remove every `NtSetInformationFile` occurrence from the snapshot module. It removes only `FileRenameInformation` ownership. Enumeration via `NtQueryDirectoryFile`, disposition/delete, recursive cleanup/reclaim and their higher-level policy remain later bounded R2A slices.

## Mechanical ownership / dependency evidence

The shared ownership regression now requires:

- shared source contains `rename_direct_child_within_parent`;
- shared source contains `FILE_RENAME_INFO_CLASS`;
- snapshot source calls `.rename_direct_child_within_parent(`;
- snapshot source contains no `FILE_RENAME_INFO_CLASS`;
- snapshot source contains no `mark_renamed_as_direct_child` compatibility call;
- snapshot source contains no direct `.directories` guard-internal access;
- existing single-owner `PinnedDirectory` / `ProtectedDirectoryGuard` / `PinnedParent` assertions remain green;
- existing production reverse-dependency regression continues to prove `windows_protected_fs.rs` contains no `crate::reviewed_source_snapshot` dependency.

Direct source search after implementation finds the production rename method only in `windows_protected_fs.rs` and the snapshot only as its consumer.

## Focused production-path regression

New Windows test:

`protected_directory_guard_rename_is_pinned_same_parent_and_fail_closed`

It proves on the real production method that:

- a renameable pinned direct child successfully renames from `staging` to `committed` under the same pinned parent;
- the guard path bookkeeping follows the successful rename and the renamed handle identity remains stable;
- an invalid `..\\outside` component fails before mutation;
- a different pinned parent is rejected and no `escaped` child appears there;
- an already-present `occupied` destination is not replaced;
- an outside sentinel remains byte-for-byte unchanged.

The existing reviewed-source-snapshot suite remains the regression authority for supported reparse/replacement/cleanup races at snapshot boundaries, including link redirection, reparse replacement, final-child replacement, nested-directory replacement, pre-disposition replacement, stale-staging replacement and handle-bound cleanup tests.

## Verification

| Gate | Result |
| --- | --- |
| `cargo fmt --check` | **PASS** |
| `cargo clippy --all-targets --all-features -- -D warnings` | **PASS** |
| `cargo test protected_directory_guard_rename_is_pinned_same_parent_and_fail_closed -- --nocapture` | **PASS** |
| `cargo test windows_protected_fs::tests` | **PASS** — 4 focused tests on each relevant binary target |
| `cargo test reviewed_source_snapshot` | **PASS** — 20 snapshot tests |
| project `verify_project` → `cargo fmt --check` | **PASS** |
| project `verify_project` → full `cargo test` | **PASS** |
| project `verify_project` → `cargo build` | **PASS** |
| `git diff --check` | **PASS**; only pre-existing LF→CRLF working-copy warnings |

## Attributable paths

- `src/windows_protected_fs.rs`
- `src/reviewed_source_snapshot.rs`
- `docs/orchestrator/review_bundles/T-0269_T0152_R2A_R8C_PINNED_SAME_PARENT_RENAME_AUTHORITY_REVIEW_BUNDLE.md`

The repository has a long-lived broadly dirty/untracked working tree and these newer Rust source files are not represented as a clean ticket baseline by ordinary repository-wide Git diff statistics. Repository-wide diff size is therefore not used as acceptance evidence. Attribution is based on the exact bounded source mutations above, mechanical source-layout assertions, focused production tests and complete project verification.

## Gate status

- reusable same-parent rename native authority shared-owned: **PASS**
- exact opened source handle used: **PASS**
- duplicated/validated pinned destination-parent handle used as `RootDirectory`: **PASS**
- direct-child/same-parent relation rejected before mutation when wrong: **PASS**
- invalid destination component rejected before mutation: **PASS**
- no destination replacement: **PASS**
- post-success bookkeeping and identity revalidation: **PASS**
- snapshot racing-winner/commit policy retained in snapshot: **PASS**
- snapshot `FileRenameInformation` ownership removed: **PASS**
- delete/disposition/enumeration/cleanup intentionally retained for later slices: **PASS**
- zero production shared→snapshot reverse dependency: **PASS**
- outside-sentinel zero mutation regression: **PASS**
- full repository verification: **PASS**

## Residual R2A authority / next slice

After T-0269, the largest remaining generic protected-filesystem authority still physically resident in `reviewed_source_snapshot.rs` is:

1. handle-relative directory enumeration via `NtQueryDirectoryFile`; and
2. exact-handle disposition/delete plus recursive cleanup/reclaim built on that enumeration.

The next ticket should remain mechanical and narrow. Prefer extracting **directory enumeration only** first, leaving disposition/delete and recursive cleanup policy for a subsequent slice. T-0231/R2B remains blocked until the full R2A authority extraction chain is independently accepted.
