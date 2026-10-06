# T-0324 SR3L — wake-target bridge source review

## Scope and live-state boundary

This is a repository review of the designated-chat-target bridge only. The
current canonical target correction is durable protected state outside this
contract. It was neither read as acceptance evidence nor mutated. In
particular, a source review does not claim target migration, wake delivery,
serving parity, or any host/runtime state transition.

T-0389 identified two prior independently accepted reviewed-build source
authorities as the sole missing first-pair prerequisite. This review assesses
whether the corrected designated-target bridge source is suitable as the first
such source basis; it is not itself a reviewed build, attestation, pair
descriptor, or ordinary-worker authority.

## Technical review before source decision

### Current closed implementation

`src/operator_facade.rs` routes only the exact public grammar
`operator designated-chat-target set --conversation-url <url>
--expected-current-target-sha256 <sha256>` to
`OperatorAction::SetDesignatedChatTarget`. The parser refuses another action,
unknown flags, duplicate flags, missing values, and missing required fields.
It exposes no raw protected-state path, shell command, arbitrary operation, or
generic command channel.

The action canonicalizes the repository workspace and delegates only to
`mcp::operator_update_designated_chat_target`. That transaction first requires
a canonical ChatGPT conversation URL and exact hexadecimal SHA-256 CAS token,
then obtains the shared target lock, validates that the project registry and
effective wake target already agree, and rejects stale or divergent input before
the first write. It updates the wake target through the existing locked helper
only as the bounded staged part of `bind_project_chat_target_after`; a registry
failure triggers exact wake compensation under the same lock, and any ambiguous
compensation/readback becomes a fixed synchronization failure.

The workspace-bound registry lookup requires exactly one matching project.
The cached compatibility branch further permits
`DESIGNATED_CHAT_TARGET_URL=<exact URL>` only for project `catdesk` with an
expected digest. Sibling-project tests show the root-bound supervisor cannot
mutate a sibling target.

### Inherited wake-repair source delta and tests

The reviewed wake-repair delta currently present in source is limited to:

- `src/operator_facade.rs`: the typed designated-target action, closed parser,
  and fixed delegation; and
- `src/mcp.rs`: `operator_update_designated_chat_target`, coherent readback,
  locked paired registry/wake CAS and compensation behavior.

The directly relevant regression coverage is:

- `mcp::tests::designated_chat_target_readback_and_guarded_update_keep_project_and_wake_coherent`;
- `mcp::tests::designated_chat_target_rejects_stale_or_preexisting_divergence_before_wake_mutation`;
- `autonomy_supervisor::tests::cached_designated_chat_target_cas_keeps_catdesk_registry_and_wake_target_coherent`; and
- `autonomy_supervisor::tests::target_binding_cannot_mutate_a_registered_sibling_project`.

These fixtures cover coherent success/idempotence, stale CAS, pre-existing
registry/wake divergence, exact cached decision form, and sibling refusal.
The direct source inspection adds the facade grammar and fixed-delegation
boundary to that evidence.

### Threat and failure analysis

An arbitrary URL is rejected by canonical conversation-URL validation; an
invalid or stale digest is rejected before mutation. Extra grammar, duplicate
flags, missing fields, raw file edits, endpoint selection, arbitrary project
selection, and generic command execution have no route through this action.
Foreign/sibling project mutation, prior registry/wake disagreement, concurrent
CAS drift, registry second-phase failure, and failed compensation all fail
closed. No ordinary worker, reviewed-main-image, core-host, completion prose,
or live target value becomes source acceptance authority.

## Classification and source decision

**`FIRST_REVIEWED_WORKER_SOURCE_READY`**

No correctness or fail-closed defect was found. No source or test change is
made: a synthetic change would not improve the fixed bridge and would alter the
candidate source basis unnecessarily. This classification approves the current
repository source as a prospective first reviewed-build *source basis* only;
it grants no reviewed-build, attestation, pair, provision, activation, or live
target authority.

## Prohibited-action audit

No protected wake/target state, ProgramData/Program Files state, release store,
worker/supervisor/process, browser, tunnel/Secure MCP, signing/UAC, Git, or
external-project state was touched.

## Verification, attribution, and next action

Focused review evidence:

- `cargo test designated_chat_target -- --nocapture` — PASS: 3 paired
  transaction/registry tests.
- `cargo test operator_facade -- --nocapture` — PASS: 8 facade tests,
  including closed action parsing and the fixed target delegation seam.

Repository verification:

- `cargo fmt --all -- --check` — PASS (only the pre-existing environment
  canonicalization warning was printed).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  PASS (same warning only).
- `cargo test --workspace --all-targets --all-features` — PASS; the primary
  unit-test run enumerated 922 tests and all target-specific test binaries
  completed successfully.
- `git diff --check` — PASS; pre-existing working-copy CRLF warnings only and
  no whitespace errors.

The attributable T-0324 completion output is this bundle only; prior
wake-repair sources and unrelated dirty-worktree changes remain unmodified and
unattributed.

After independent final review accepts this source basis, the exact next action
is the host-side `catdesk_reviewed_build` PREPARE/CONFIRM path using this
acknowledged review record. It must create the first immutable candidate from
the reviewed snapshot; it must not add another trust mechanism or start a
generic audit.
