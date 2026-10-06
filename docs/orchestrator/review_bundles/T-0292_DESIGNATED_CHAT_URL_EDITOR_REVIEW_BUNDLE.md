# T-0292 Designated Chat URL Editor — Review Bundle

## Scope and result

T-0292 adds the bounded native Binagotchy GUI editor for the designated
CatGPT control-chat URL. It is a view/controller over the existing project and
wake authorities; it is not a new target store and does not use a browser,
browser profile, wake dispatch, tunnel, daemon, shell, or caller-selected
filesystem path.

The GUI now has a separate editable draft, authoritative URL/digest readback,
fixed `Apply/Update`, fixed `Refresh readback`, and fixed-vocabulary status.
Text entry alone is a no-op. The production controller uses only the new
crate-internal composition of the existing canonical conversation URL validator
and guarded project/wake compare-and-swap authorities.

## Authority design

| Concern | T-0292 behavior |
| --- | --- |
| Initial/recreated state | Read-only authoritative readback from the one project registered for the canonical GUI workspace plus the fixed wake configuration. The registry read-only opener creates no children. |
| Draft edits | Held only in `DesignatedChatUrlControllerV1`; no authority method is invoked. |
| URL validation | Existing `canonical_project_chat_target` plus the matching fixed wake target validation; credential-free HTTPS conversation/project-conversation shape only. |
| Apply guard | Uses the authoritative displayed SHA-256 as the expected current digest. Bad/missing digest, stale target, malformed URL, malformed protected state, or preexisting registry/config divergence fails closed. |
| Coherence | The guarded transaction first proves central project and effective wake target are identical. It changes the wake target under its existing lock, applies the existing project-target CAS, then performs exact readback. A rejected second CAS compensates only the exact just-written wake target; an inability to prove compensation is `DESIGNATED_CHAT_URL_SYNCHRONIZATION_FAILED`. |
| Same value | Refreshes authoritative state and returns `DESIGNATED_CHAT_URL_UNCHANGED` without a mutation call. |
| Error display | Fixed redacted categories only: invalid, stale, protected-state mismatch, synchronization failure, or authority unavailable. No path, profile, raw URL, digest, or lower-layer error text is displayed on failure. |

The GUI does not call `catdesk_wake_bridge_run_once`, does not inspect browser
profile/storage, and cannot select any filesystem path. The native controls use
fixed Win32 child controls and only pass their draft text to the reviewed
authority.

## Deterministic evidence

| Test surface | Evidence |
| --- | --- |
| Controller initialization/recreation | Authoritative URL/digest is read on each controller construction. |
| Draft no-op | A draft edit makes zero authority update calls. |
| Apply/readback | A valid draft is CAS-updated and then refreshed from authority. |
| Idempotence | Same-value Apply is `UNCHANGED` with zero update calls. |
| Refusal | Malformed draft is rejected before authority; stale and protected-state fixture failures preserve the prior target and use redacted status. |
| Production authority fixture | Coherent project/wake state updates and reads back successfully; stale expected digest and preexisting registry/config divergence leave the wake configuration unchanged. |

## Verification

| Command | Result |
| --- | --- |
| `cargo test windows_gui --all-features` | Passed: 14 focused GUI tests. |
| `cargo test designated_chat_target --all-features` | Passed: 2 guarded authority tests. |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed. |
| `cargo test --all-targets --all-features --no-fail-fast` | Passed: 868 tests; expected platform-dependent ignored tests remained ignored. |
| `rust_full` project profile | Satisfied by the all-target/all-feature Rust test profile above. |
| `cargo build --all-targets --all-features` | Passed. |
| `git diff --check` | Passed (exit 0); accumulated dirty-tree CRLF warnings were non-fatal and unrelated. |

The successful commands emitted the pre-existing non-fatal workspace warning
`could not canonicalize path C:\\Users\\Volap`; it did not change their exit
status.

## Attributable files

- `src/windows_gui.rs`
- `src/mcp.rs`
- `src/delegated/autonomy_projects.rs`
- `CATDESK_MILESTONES.md`
- `.catdesk/current_plan.md`
- `.catdesk/todo.md`
- this exact review bundle

The worktree is intentionally dirty from prior approved work. This bundle does
not claim ownership of unrelated paths and no branch, commit, reset, clean, or
publication operation was performed.

## Prohibited live actions

No real configured chat target was mutated. No browser or wake was launched,
no profile/storage was read, no Secure MCP/tunnel or host lifecycle was
changed, no external project was touched, and no signing/provenance or Git
publication work was performed.

## Residual boundary

T-0292 is source/test complete pending independent review. It does not satisfy
T-0224 live natural delivery, T-0223 host-live stable-supervisor activation, or
T-0222 visible Windows GUI host acceptance. Those remain dependency-ordered and
operator/host-gated as recorded in the current plan.

**Status: READY_FOR_INDEPENDENT_REVIEW.**
