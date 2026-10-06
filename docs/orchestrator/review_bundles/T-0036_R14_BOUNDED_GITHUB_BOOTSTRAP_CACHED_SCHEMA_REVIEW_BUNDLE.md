# T-0036-R14 Bounded GitHub Bootstrap Cached-Schema Review Bundle

## Scope

T-0036-R14 adds two closed compatibility decisions to the already exposed
`autonomy_project_registry_bind` surface. It adds no generic shell, executable,
owner, repository, visibility, path, credential, or GitHub token input. This
worker did not invoke either decision, inspect either sibling repository, read
credentials, or perform a GitHub mutation.

The only policy mappings are:

| Project | Exact workspace | Private remote |
| --- | --- | --- |
| `BYOVD_DRIVER_PIPELINE` | `<USER_PROFILE>\OneDrive\Desktop\Projects\BYOVD_DRIVER_PIPELINE` | `https://github.com/vnath99/BYOVD_DRIVER_PIPELINE.git` |
| `BUG_BOUNTY_RECON_PLATFORM` | `<USER_PROFILE>\OneDrive\Desktop\Projects\BUG_BOUNTY_RECON_PLATFORM` | `https://github.com/vnath99/BUG_BOUNTY_RECON_PLATFORM.git` |

`catdesk`, any old local CatDesk folder, any third project, and every caller
supplied owner/repository/origin/visibility value are rejected.

## Cached request forms

Only these exact, no-surrounding-whitespace forms are accepted:

```json
{"projectId":"BYOVD_DRIVER_PIPELINE","decision":"GITHUB_BOOTSTRAP_PREFLIGHT_WORKSPACE=C:\\Users\\Volap\\OneDrive\\Desktop\\Projects\\BYOVD_DRIVER_PIPELINE"}
{"projectId":"BYOVD_DRIVER_PIPELINE","decision":"GITHUB_BOOTSTRAP_CONFIRM","confirmationToken":"<opaque 32-hex token>"}
```

The Bug Bounty form substitutes only its fixed project ID and fixed workspace.
Every mode has a closed top-level-field allowlist. Mixed target/registration/
thread fields, cached CAS fields, raw thread IDs, Git identity input, arbitrary
workspace fields, whitespace ambiguity, and extra parameters reject before any
operation.

## Trust and state machine

Preflight is read-only. It requires an absolute, existing, regular,
non-symlink/non-reparse workspace which canonicalizes to the policy's exact Git
top-level root. Fixed Git metadata calls require missing `remote.origin.url`, a
valid existing `HEAD`, a safe checked-out branch, and a bounded porcelain dirty
summary. File names and repository content are not retained or returned.

GitHub CLI discovery uses only a small absolute allowlist of `gh.exe`
installation paths, rejects missing, multiple, reparse, workspace-contained,
or fingerprint-drifting candidates, and never uses PATH or a caller executable.
Fixed `gh api user --jq .login` must resolve exactly to `vnath99`; the response
returns only policy evidence, never login raw output, tokens, headers, or
credential-store material. A fixed repository-view probe must show the exact
policy remote absent; an unmanaged pre-existing remote is rejected.

The opaque short-lived preflight binds canonical workspace identity, Git root,
missing-origin state, HEAD, branch, dirty boolean/count, trusted-gh fingerprint,
verified account, fixed policy remote, absence proof, and expiry. Confirmation
re-reads all evidence. A mismatch, expiry, replacement, changed head/branch or
dirty state, origin drift, account/CLI drift, or project/token mismatch stops
without a new external mutation.

Before the first external mutation confirmation atomically journals a
non-secret transaction. Its only possible fixed command sequence is:

1. `gh repo create vnath99/<mapped-repository> --private`.
2. `git remote add origin <mapped-exact-origin>` only while origin is absent.
3. `git push -u origin <already-checked-out-safe-branch>`.

It never stages or commits files, changes remotes, forces or rewrites history,
publishes tags/all branches, alters visibility/permissions, deletes a remote,
or accepts arbitrary command text. Journaled stages make a post-create or
post-origin failure resumable only with the same token/project/evidence; an
expected journaled origin is the sole accepted existing-origin case. Nothing is
auto-deleted during recovery.

## Changed paths and deterministic coverage

- `src/delegated/github_bootstrap.rs`: fixed policy, bounded command runner,
  trusted executable validation/fingerprinting, evidence records, durable
  journal, fixed mutation sequence, and reason codes.
- `src/delegated/autonomy_supervisor.rs`: two strict cached-mode branches and
  schema forms; legacy direct binding, T-0097/T-0098 CAS, R12/R13 controls, and
  target initialization remain separate.
- `src/delegated/mod.rs`: exposes the internal bootstrap module.

Coverage includes exact policy/CatDesk rejection and unsafe branch rejection in
the bootstrap component, plus dispatcher regression cases for extra fields,
mixed legacy fields, whitespace, and CatDesk bootstrap rejection. The complete
existing registry suite continues to cover project/thread uniqueness, atomic
registration/adoption confirmation, stale evidence, scheduler one-writer
behavior, and all existing cached decision compatibility paths.

## Verification and host sequence

Local verification completed:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test` — 536 passed, 18 ignored
- `git diff --check`

The authoritative diff was reviewed for scope containment. The workspace was
already dirty; no reset, Git publication, daemon reload, browser/wake,
Scheduler, tunnel, external-workspace, or GitHub action was performed.

Separate host acceptance, after an isolated reviewed build/reload and verified
connected transport, must execute one fresh read-only preflight per approved
project. Only a successful preflight may be followed by its matching opaque
confirmation. The host records only bounded response evidence, then verifies
the exact policy origin, branch/HEAD and transaction stages. Any rejection or
partial result remains fail-closed for operator reconciliation; it must not be
converted into a destructive rollback or a second repository identity.
