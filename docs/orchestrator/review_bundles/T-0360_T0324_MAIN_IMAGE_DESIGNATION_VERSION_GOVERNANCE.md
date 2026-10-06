# T-0360 / T-0324 — ChatGPT main-image designation and version-governance closure

Date: 2026-09-09
Status: IN PROGRESS — rotation requirement and executable-source eligibility independently designated; immutable snapshot/build measurement and signing payload still pending

## Standing designation authority

The operator has explicitly delegated CatDesk release/main-image designation to ChatGPT. ChatGPT, not Codex/Qwen and not the operator, now owns the engineering decision of whether a new signed main image is required and which exact immutable reviewed candidate should become that image. Workers may implement or independently review bounded changes, but they do not choose the release bytes.

This does not weaken the product trust boundary. The existing production Ed25519 root/public key remains unchanged (`3bc6a2101687816dc8235efde562db93dbd0ad822d922286640ec5c985239545`). The production private key remains outside the repository, CatDesk, MCP, ChatGPT, Codex, and Qwen and must not be requested, read, copied, exported, regenerated, or replaced. ChatGPT should use any already-reviewed local signer/elevation facade that can operate without exposing the key; only a technically inaccessible cryptographic/elevation action may remain external after exact candidate designation.

## Independent T-0324 image decision

A new signed reviewed main-image generation is REQUIRED.

The canonical serving runtime is the legacy 77-tool pre-T-0299 generation. The remaining core acceptance path depends on newer accepted executable/control-plane behavior, including the Qwen 3.8 post-tool continuation compatibility path, stranded delegated-run cancellation reconciliation, reviewed-build/stable-supervisor surfaces, cumulative accepted recovery behavior, and the newer GUI/control-plane generation. Re-signing or re-consuming the historical T-0215 epoch-1 image cannot provide those capabilities. The unsigned historical T-0217 epoch-2 payload also predates the required generation and is not an eligible candidate.

This rotation decision is based on deployment/source capability skew, not on a policy of signing after every change. Documentation-only or non-runtime changes do not trigger a new image. A new image is required when accepted executable/control-plane changes must cross into canonical serving authority, when the currently deployed generation lacks required accepted behavior, or when a security/compatibility correction invalidates the previously designated executable generation.

## Candidate-selection invariant

Passing tests do not make mutable workspace bytes deployment authority. `target/release`, the intentionally dirty worktree, verification-only artifacts, caller-selected hashes/paths, generic independent review, and direct promotion-script output remain ineligible.

The exact designated release must be derived from an immutable source candidate whose executable-affecting contents have been independently reconciled by ChatGPT to the accepted current engineering state. A locked release build must then be made from that frozen candidate and measured. Only after the exact candidate SHA-256 and length are fixed may ChatGPT finalize the monotonically newer signed-envelope epoch/purpose/review/build identifiers and canonical signing payload.

## Current evidence collected this cycle

- Official Secure MCP transport: `CONNECTED_VERIFIED`; local MCP: `READY`; existing official runtime remains externally owned/monitored and no duplicate was created.
- Serving catalog remains 77 tools / pre-T-0299 by the existing T-0324 evidence.
- Delegated workspace inventory contains only historical `T-0322` in `CANCEL_REQUESTED`; no new RUNNING autonomous session was found, so no competing mutator was started.
- Current workspace verification: `cargo fmt --check` PASSED, full `cargo test` PASSED, and `cargo build` PASSED through CatDesk `verify_project`.
- Strict `cargo clippy --all-targets --all-features -- -D warnings` PASSED.
- A locked release build was attempted twice through the legacy MCP command surface but exceeded its fixed 120-second command ceiling. This is not recorded as a compiler failure and is not release measurement evidence. No partial/mutable release output is designated.
- Search of production source found no signer implementation: `ed25519_dalek::SigningKey`/`Signer` is test-only in `src/reviewed_build.rs`. Therefore the current source preserves the design that production verifies signatures but does not possess the private signer.
- A second bounded repository-wide signer-facade audit on 2026-09-09 searched production Rust and `scripts/**` for signing/private-key/offline-signing/envelope operations. No reviewed non-exporting production signer facade exists in the repository. The only production main-image cryptographic operation is Ed25519 verification against the compiled public root; the PowerShell tree contains no main-image signing path. This closes the "keep searching for an existing signer facade" branch: candidate selection/build/measurement/payload generation remain CatDesk/ChatGPT work, while the final detached-signature operation is cryptographically external unless a separately installed non-repository signer surface is later proven.
- The executable-affecting workspace inventory remains intentionally broad: both tracked modifications and substantial untracked `src/**` modules are present, including the reviewed-build/source-snapshot, supervisor, wake, GUI, recovery and provider-control surfaces. This confirms that the exact release cutoff cannot be represented by `HEAD` or by a simple tracked diff. The candidate must be frozen from an explicitly audited source manifest rather than inferred from Git cleanliness.

## Executable-source eligibility closure — T-0365

ChatGPT completed the bounded executable-source eligibility audit in `T-0365_T0324_EXECUTABLE_SOURCE_ELIGIBILITY_MANIFEST.md`. The designated source domain for the next image is exactly the current `Cargo.toml`, `Cargo.lock`, recursive `src/**`, plus `build.rs` only if a safe regular file exists at immutable snapshot creation time. Current status inventory shows no `build.rs`. This designation does not include `target/**`, logs, diagnostics, review prose, Git metadata, or the rest of the dirty workspace as executable authority.

The audit mapped the untracked/current supervisor, reviewed-build/source-snapshot, wake, GUI, protected-filesystem, provider and acceptance modules to their existing accepted review lineages. A residual tracked-source audit found one bounded production diff needing direct ChatGPT disposition: `src/command.rs` adds only the closed lifecycle-facade intercept. ChatGPT reviewed its exact diff and `cargo test detect_lifecycle_facade_intercept -- --nocapture` passed 2/2. The other residual diffs are either the already-reviewed T-0030 exact `.` contract-root correction or test-only Windows ignore annotations. No unresolved production hunk was identified in that residual group.

Therefore source *eligibility* is now converged. Mutable workspace bytes are still not candidate authority. The next trust transition is the existing reviewed immutable source-snapshot + locked reviewed-build mechanism.

## Remaining exact designation work

1. Freeze the now-designated executable source domain through the reviewed immutable source-snapshot authority without cleaning/resetting unrelated dirty worktree state.
2. Build that frozen candidate with the locked release policy outside the legacy 120-second command-ceiling limitation, using a reviewed bounded local mechanism rather than blessing any pre-existing `target/release` bytes.
3. Measure exact payload SHA-256 and length and independently verify that payload corresponds to the frozen source candidate.
4. Select the next valid monotonic main-image epoch and the appropriate fixed bootstrap/rotation purpose from the existing accepted product-root policy; write the exact canonical unsigned envelope payload.
5. Use a reviewed non-exporting local signer facade if one exists. If no such technically accessible facade exists, record the exact already-designated signing payload as the only external cryptographic boundary; never expose the private key.
6. Independently verify the detached signature with the compiled product root, stage only the exact verified payload/envelope through the accepted fixed administrator transport, consume through the zero-choice bootstrap/rotation surface, and read back the signed receipt/image identity.
7. Reconnect while preserving external Secure MCP ownership, prove new serving-generation/tool parity, reconcile T-0322, run the bounded Qwen 3.8 continuation canary, then resume T-0223, guarded current-chat wake rebinding, GUI acceptance, T-0152, and T-0155.

## Safety / non-actions

No raw reload, manual binary copy, protected-lock deletion, direct promotion script, private-key access, signature fabrication, product-root replacement, browser wake, protected wake-target edit, Scheduler/service mutation, tunnel replacement, Git publication, or dirty-worktree cleanup was performed in this cycle.
