# T-0324 Paired Current-Chat Static-Schema Bridge Review Bundle

Date: 2026-09-10

## Objective

Restore a safe path to move the CatDesk project registry and effective wake target together when ChatGPT is attached to a legacy cached MCP schema. The currently authorized control conversation is `https://chatgpt.com/c/6aa2d008-3b78-83e9-b4ce-9dddabfdfa2b`, SHA-256 `4e0b61904268640353ad891a9b7b6e58c29c914a3adece36c72806e78dc90da7`. The live legacy registry still identifies predecessor `https://chatgpt.com/c/6a976557-1054-83ea-b4c3-b10bb51b4800`, SHA-256 `c16f6d1e834c3d1d530e3cc842be09021df2b029b3e94c44b272e80b85f5e911`.

## Defect / compatibility gap

Current source already contains `mcp::operator_update_designated_chat_target()`, the reviewed paired transaction used by the GUI. It validates the exact URL, requires coherent project/wake readback, compares the expected current digest, updates the central project target and effective wake target under the same lock, compensates a second-stage failure, and returns only after exact final readback. The currently serving pre-T-0299 77-tool generation does not expose that modern surface.

The older cached-schema compatibility mode `CHAT_TARGET_URL=<url>` on `autonomy_project_registry_bind` deliberately changes only the project registry. Using that older mode as a CatDesk migration workaround would permit project/wake divergence and is therefore not an acceptable event-wake restoration path.

## Implementation

`src/delegated/autonomy_supervisor.rs` now accepts an additional exact cached-schema compatibility decision:

`DESIGNATED_CHAT_TARGET_URL=<exact ChatGPT conversation URL>`

The request must contain exactly `projectId`, `decision`, and `expectedSha256` from fields already present in the legacy cached schema. It is valid only for `projectId=catdesk`. It validates the conversation URL and delegates directly to `crate::mcp::operator_update_designated_chat_target(&self.workspace, conversation_url, expectedSha256)`; it does not duplicate the paired update logic. It returns only `projectId`, `targetSha256`, and `targetUrl`.

Existing `CHAT_TARGET_URL=` registry-only behavior remains unchanged because it is still used by the reviewed multi-project compatibility path. External projects cannot invoke the new paired CatDesk-only mode. Mixed/extra fields, stale CAS, malformed URLs, and pre-existing authority divergence remain fail closed through the existing schema/key guards and paired transaction.

This change launches no browser and creates no browser-delivery evidence. It does not directly edit protected wake state, alter Secure MCP ownership, or change release/signing authority.

## Deterministic verification

- `cargo test cached_designated_chat_target_cas_keeps_catdesk_registry_and_wake_target_coherent -- --nocapture` — PASS, 1/1. The production compatibility handler updates both fixture registry and real fixture wake configuration, rejects stale CAS, rejects a non-CatDesk project, and leaves both authorities coherent after failures.
- `cargo test cached_connector_target_cas_requires_exact_fixed_decision_shape -- --nocapture` — PASS, 1/1. Existing cached registry-only target compatibility remains intact.
- `cargo test designated_chat_target_ -- --nocapture` — PASS, 3/3. Existing paired readback/update, stale/divergence refusal, and the new cached bridge all pass together.
- `cargo fmt --all -- --check` — PASS after applying canonical formatting.

## Live boundary

No live target mutation is claimed in this review bundle. The currently attached runtime remains the legacy 77-tool generation and therefore cannot yet consume this source change. The next deployment-parity work must place a reviewed modern generation behind the stable/self-service update path without restoring the rejected routine UAC/signing-clerk workflow. After that generation is live, the exact current chat must be bound using this paired mode with the old coherent digest as CAS input and exact post-update readback. Only a separately fresh ordinary CatDesk review event may then supply natural event-driven wake acceptance evidence; manual browser wake is not acceptance.

## Safety / invariants preserved

- externally owned official Secure MCP runtime remains externally owned;
- dirty worktree is preserved;
- no registry-only CatDesk migration workaround;
- no direct protected wake-state edit;
- no manual browser wake;
- no arbitrary URL/path/executable/shell authority;
- no Git publication;
- no private signing-key access or new trust root;
- routine versioning remains headed toward immutable reviewed user-owned versions with atomic activation/readiness/rollback and durable audit evidence rather than recurring Program Files/UAC rotation.
