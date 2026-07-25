# Patch Engine

Status: T-0017 implementation
Date: 2026-07-25

## Scope

T-0017 adds a CatDesk-owned patch engine for structured replace patches. The engine validates model proposals, previews changes, applies approved patches, records actual Git diffs, compares revisions, and blocks false-success claims.

## Patch Proposal

`PatchProposalV1` includes:

- patch ID;
- optional parent patch ID;
- run ID;
- turn ID;
- base snapshot hash;
- target paths;
- expected pre-image hashes;
- structured replace operations;
- model rationale;
- claimed acceptance criteria.

## Validation

Preview and apply both enforce:

- supported schema version;
- non-empty operations and target paths;
- contained relative paths;
- execution-contract allowed paths;
- forbidden path blocking;
- operation path membership in `target_paths`;
- old and new text differ;
- expected pre-image hashes match the current file;
- replacement text matches exactly once.

Stale bases, conflicts, and out-of-scope paths fail before mutation.

## Preview And Apply

`preview` computes a structured diff without writing files.

`apply` reruns preview validation, writes changes through CatDesk, records before and after hashes, and captures the authoritative `git diff -- <paths>` result.

## Patch Comparison

`compare_patches` records:

- files added;
- files removed;
- files modified;
- superseded operations;
- net operation delta;
- scope warnings.

This supports repair loops where a revised patch supersedes a parent patch.

## False-Success Protection

`verify_model_completion_claim` rejects a model completion claim unless:

- the claim is non-empty;
- tests passed;
- the actual diff artifact is non-empty.

The model cannot self-declare verified success.

## Live Qwen Proposal Smoke

On 2026-07-25, Qwen `qwen3.5:9b` was given only a tiny disposable excerpt:

```text
path: src/bug.txt
content:
answer=41
```

It returned parseable JSON:

```json
{
  "patch_id": "patch-qwen-live",
  "target_path": "src/bug.txt",
  "old": "answer=41",
  "new": "answer=42",
  "rationale": "fix disposable answer bug"
}
```

CatDesk retained all patch application and verification authority. Qwen did not receive file, shell, Git, or patch execution authority.

## Implementation

Rust module:

- `src/delegated/patch_engine.rs`

Export:

- `src/delegated/mod.rs`

## Verification Coverage

Focused tests cover:

- preview and apply with actual Git diff;
- stale-base rejection;
- out-of-scope blocking;
- patch revision comparison;
- model false-success rejection;
- disposable bug-fix repair cycle.
