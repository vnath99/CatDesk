# T-0036-R16 Trusted Git/GH Resolver Normalization Review Bundle

## Design

R16 corrects only executable identity selection in the R15 GitHub-bootstrap
path. Git and `gh` are separate tool kinds with separate bounded failures.
Each has an ordered, fixed absolute Windows slot list. The resolver selects the
first valid slot only; lower-priority valid installations are intentional
fallbacks, not ambiguity. PATH, current directory, caller input, registry, and
workspace executables have no authority.

For the selected slot, CatDesk requires a regular non-symlink/non-reparse file,
canonicalizes it, rejects a canonical path inside the validated workspace, and
SHA-256 fingerprints its bytes. It persists slot number, canonical identity,
and digest for both Git and `gh` in preflight evidence. Confirmation re-resolves
the ordered lists and rejects slot, canonical-identity, or digest drift before
journaling or external mutation. This includes higher-priority candidate drift.

Git failures now return `GITHUB_BOOTSTRAP_GIT_*` codes; `gh` failures return
`GITHUB_BOOTSTRAP_GH_*` codes. Normal Git-for-Windows `cmd\git.exe` plus
`bin\git.exe` coexistence therefore selects `cmd` deterministically rather
than producing a false GH ambiguity. Missing preferred slots fall through to a
valid lower-priority policy slot.

## Preserved controls

R15 OS-canonical workspace comparison, exact Git-root proof, exact `vnath99`
authentication, and bounded GraphQL ABSENT/EXISTS schema validation remain
unchanged. The two-project policy, private-only fixed remote creation, exact
origin add, current safe branch push, durable transaction stages, and partial
recovery rules remain unchanged. No add/commit, force/all/tags, rewrite,
remote replacement, repository delete, visibility/permission operation, or
arbitrary command path was added.

## Changed scope and verification

- `src/delegated/github_bootstrap.rs`: typed tool resolver, fixed-priority slot
  selection, tool-specific failure codes, and preflight/confirm slot identity
  binding.
- This review bundle only.

Deterministic tests cover preferred-slot selection with a lower-priority Git
fallback present, fallback after preferred absence, workspace hijack refusal,
unsafe executable refusal, exact GraphQL outcomes, and exact account/policy
constraints. Existing cached-mode, journal, and registry tests remain in the
full suite.

Local verification: `cargo fmt --check`, strict clippy, `cargo test`, and
`git diff --check`. No sibling repository, GitHub, browser, wake, tunnel,
Scheduler, daemon/reload, release, credential-store, or Git publication action
was performed.

## Post-review host sequence

1. Reload only an isolated reviewed candidate and require `CONNECTED_VERIFIED`.
2. Run a single fresh BYOVD preflight with the fixed cached request form.
3. Confirm only if the returned bounded evidence is fully positive and current.
4. On any resolver, auth, GraphQL, or evidence rejection, stop without mutation
   and leave reconciliation to a separately approved host action.
