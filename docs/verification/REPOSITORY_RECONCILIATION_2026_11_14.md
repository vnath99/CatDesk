# Repository reconciliation â€” 2026-11-14T16:50:51.082837+00:00

| Reference | Exact SHA | Main-only / ref-only | Integration-only / ref-only | Role |
| --- | --- | --- | --- | --- |
| `codex/c1-ui-002-content-fallback` | `53e9f4db3236ee45ed8f4d9eed567ecef0fa14e2` | 0/1098 | 8/2 | Divergent UI text fallback; 2 unique commits, not installed |
| `codex/response-observer-release-nov09` | `8d96edea45ba759ccd809d5b4d52d1e3126db4f9` | 0/877 | 229/2 | Divergent reviewed-reader preparation; 2 unique commits, no auto-merge |
| `codex/web-rollover-oct17` | `5d51a1dd9d176f94ac8e1ec04f44d6c91d18d4e5` | 0/1104 | 0/0 | Existing canonical development/integration; PR #4 unmerged |
| `main` | `db6cef4d4ea50c02b5bf6959bdd96c1c58210ec8` | 0/0 | 1104/0 | Stable source baseline; not latest installed release |
| `orchestrator/chatgpt-codex-autonomous-loop` | `313f6a31f13c04c90d6098fad57f1e6a6c99cfd7` | 0/875 | 229/0 | Preserved canonical installation checkout; not development head |
| `upstream/main` | `5d11c3640b574efaa36b925d79d4846919fa7aca` | 53/189 | 1157/189 | Upstream reference; diverged, do not bulk merge |

All 70 local/remote references and ancestor/divergence counts are in the [machine-readable map](REPOSITORY_RECONCILIATION_2026_11_14.json). Counts compare actual object reachability, not code quality. Fetch changed references only; no checkout, reset, rebase, merge, stash application or pruning of another workspace occurred. 53 preexisting worktrees and three stashes were inventoried; two worktrees have tracked changes. Owners not independently established are UNKNOWN; a Git worktree is not proof of an idle agent. Canonical checkout's local 313f6a3 is 453 commits ahead of its stale remote branch and 229 behind development. Commits reachable through the development branch remain recoverable even when not tips of their original named branch.

`main` stays the stable source baseline; no installed acceptance is inferred from it. Reuse `codex/web-rollover-oct17` as development/integration because it contains the current reviewed source progression, rather than renaming active owners. Keep this documentation change on `codex/docs-consolidation-nov14`. UI and response-reader branches diverge and require specific review before integration. PR #4 targets the older orchestrator remote; do not silently retarget or merge it.

GitHub confirms PUBLIC visibility, default main and no main protection (404). No settings changed. The latest visible CI success is from October 14, not this checkpoint; hosted CI was not dispatched. Local checks qualify this documentation branch only. Tags v0.1.0â€“v0.1.6 exist; no evidence here ties them to the current installed executable, so no new stable release tag is invented.

## Migration map

The [783-file inventory](DOCUMENT_INVENTORY_2026_11_14.csv) records 10,087,139 bytes of tracked Markdown. Reuse five root documents for handoff, milestones, commands, activity and tickets. Preserve technical architecture as a supporting design authority. Preserve detailed tickets/review bundles at their existing paths. Reclassify old handoff/commands as historical via compatibility notices. Preserve the old milestone chronology by its exact published [archive reference](../archive/PRE_CONSOLIDATION_2026_11_14.md); no lossy copy or deletion of history. Keep local operational state private. No giant logs are copied into the new handoff.

Conflicts resolved: an October 29 README pointer was stale; the 679 KB handoff and 855 KB command reference mixed chronology with current instructions; a root `.catdesk` link does not exist in a clean clone. The root handoff now distinguishes installed evidence, development-only work, unknown sends and local state. Historical completion phrases are not imported as acceptance. Remaining historical ticket status is deliberately UNVERIFIED until attributable evidence is reconciled; no blanket DONE conversion.

Inventory byte sizes/SHA-256 values describe the pre-migration local working files, including their line endings. Published Git blobs may normalize those endings; the archive index separately hashes actual Git blob bytes at the pinned commit. A source last-commit date is not a test/acceptance timestamp.
