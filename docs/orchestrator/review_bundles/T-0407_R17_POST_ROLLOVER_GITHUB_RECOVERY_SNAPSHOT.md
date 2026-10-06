# T-0407 R17 post-rollover current-source GitHub recovery snapshot

## Decision

**ACCEPTED / PASSED for publication authority binding.**

R17 is the final post-rollover refresh of the independently reviewed R16 current-source recovery snapshot before Git history mutation. The manifest-included continuity documents were frozen before this final digest was calculated; final identity is recorded only in supplemental R17 artifacts so the manifest does not self-invalidate.

## Frozen R17 identity

- Manifest: `docs/orchestrator/T-0407_R17_GITHUB_RECOVERY_SNAPSHOT_MANIFEST.json`
- Manifest SHA-256: `ec3136f750709476f180a5c90b1e3201bfb319b277846ecc79555ee9d3515972`
- Descriptor: `docs/orchestrator/T-0407_R17_GITHUB_PUBLICATION_DESCRIPTOR.json`
- Descriptor SHA-256: `45ef2042ba68262b9b8d238f79832e30929d5b745e6099924da00a9b0289e761`
- INCLUDE_RECONSTRUCTION: 972
- EXCLUDE_ARCHIVAL_BINARY: 12
- EXCLUDE_RUNTIME_GENERATED: 1
- Branch: `orchestrator/chatgpt-codex-autonomous-loop`
- Frozen pre-publication HEAD: `b958eb9fff4522168ebb1ae4a726209896a27451`

## Controlled delta from accepted R16

R17 preserves the accepted R16 exclusion policy. The current generation-29 `CATDESK_MILESTONES.md` and `CATDESK_NEW_CHAT_NOTES.txt` bytes are remeasured, and the accepted R16 manifest, descriptor, and review bundle are retained as provenance. No CatDesk implementation file was intentionally changed as part of this snapshot refresh.

Current key measurements:
- `CATDESK_MILESTONES.md`: `ec6c4754181d474cf605ab21092d35db7851fdf3b704b554c44feecb99bd6c28`, 142883 bytes.
- `CATDESK_NEW_CHAT_NOTES.txt`: `f44dc0831eed50b13bfc67310af32f34587ed30108666e12580b94766101e450`, 161598 bytes.
- `scripts/wake_bridge.py`: `aa4c055990fabc2cea811cc9514bfbfb3e77085e6dbf7a3bab2fb88914e2697c`, 108181 bytes (working-tree bytes; stale staged/index copy remains non-authoritative).
- `Cargo.toml`: `9e00daf361105263c90eb849649b8afe6bade5ec9a4a2a6e98de3b4473c5e03d`, 1015 bytes.

## Validation

The final written manifest digest is bound by the descriptor. Descriptor counts match the manifest classifications. The manifest contains no duplicate paths, no R17 self artifacts, and no `CURRENT_GITHUB_PUBLICATION_AUTHORITY.json` entry in its hash graph. The R16 provenance artifacts are included from their accepted workspace bytes.

R16 already passed fmt, strict Clippy, full workspace/all-target/all-feature tests, diff check, and full include safety scanning. R17 is a publication-metadata/continuity refresh, not a code change. The commit path must still pass CatDesk's normal verified-commit gate or an equivalently reviewed verification boundary before history mutation.

## Publication boundary

The descriptor lists exactly three supplemental reviewed artifacts in source-required order: the R17 descriptor, this R17 review bundle, and the stable authority pointer. Publication may stage only the 972 INCLUDE_RECONSTRUCTION paths plus the R17 manifest and those three supplemental artifacts. Never use `git add -A`, never force-push, never mutate main/master, and require remote feature-branch HEAD == local HEAD.

## Operator action

None.
