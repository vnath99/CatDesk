# T-0036-R15 GitHub Bootstrap Policy/Absence Hardening Review Bundle

## Root cause and repair

The R14 live preflight stopped safely with
`GITHUB_BOOTSTRAP_WORKSPACE_POLICY_MISMATCH`. The fixed workspace literal was
compared as uncanonicalized text after only the caller value was canonicalized.
That rejects an OS canonical/extended-length spelling of the same directory.

R15 validates the caller path and the fixed policy path independently as
absolute, existing, regular, non-symlink/non-reparse directories, canonicalizes
both through the OS, and compares canonical `Path` identity. No slash stripping,
prefix removal, repository initialization, or path guessing is used. Git root
proof then runs from that validated canonical policy workspace and requires its
canonical `rev-parse --show-toplevel` result to equal the same directory.

## Hardened preflight and confirmation

The fixed two-project policy, private remote mapping, cached forms, and closed
argument allowlists remain unchanged. Preflight requires exact account evidence
for `vnath99`, trusted-tool fingerprints, valid HEAD/safe branch/dirty summary,
missing origin, and a positive exact repository state result.

Repository absence is now a fixed `gh api graphql` request whose only accepted
successful bounded JSON envelopes are `{"data":{"repository":null}}` (ABSENT)
or `{"data":{"repository":{"id":"..."}}}` (EXISTS). Command, auth,
network/TLS/API/rate-limit, GraphQL, oversized, non-UTF8, malformed, or extra
schema responses all fail closed as `TARGET_REPOSITORY_PROBE_FAILED`; none means
ABSENT. Confirmation repeats account and target-state proof immediately before
journaling. A newly appearing repository stops before mutation.

Git publication no longer resolves `git` via current directory or PATH. The
bootstrap runner and evidence check use fixed absolute Git installation
candidates, require one canonical regular non-reparse candidate outside the
workspace, and fingerprint it. `gh.exe` retains the same absolute candidate,
canonicality, outside-workspace, ambiguity, and fingerprint protections. Both
tool fingerprints are revalidated at confirmation.

If the durable transaction says `REPOSITORY_CREATED`, recovery positively proves
the exact mapped repository EXISTS before continuing. Otherwise it positively
proves ABSENT immediately before `gh repo create`. A journal inconsistency,
origin drift, or changed evidence fails closed. The only mutation allowlist is
private create of the mapped `vnath99/<repo>`, exact `remote add origin`, and
`push -u origin <safe checked-out branch>`; staging, commits, force/all/tags,
rewrites, remote replacement, deletion, visibility/permission changes, and
rollback are absent.

## Changed paths and verification

- `src/delegated/github_bootstrap.rs`: canonical policy path comparison,
  trusted Git executable fingerprinting, exact GraphQL existence/absence proof,
  and journal-aware recovery revalidation.
- [R15 review bundle](T-0036_R15_GITHUB_BOOTSTRAP_POLICY_ABSENCE_HARDENING_REVIEW_BUNDLE.md).

Focused deterministic coverage checks corrected literal separators, CatDesk
rejection, safe branch validation, exact account validation, exact ABSENT/EXISTS
repository envelopes, and rejection of command or ambiguous GraphQL responses.
The existing dispatcher/registry suite continues to cover closed cached forms,
atomic registration, expiration, conflicts, and scheduler isolation.

Local checks completed: `cargo fmt --check`, strict clippy, focused bootstrap
tests, and the full Cargo suite. No external workspace, GitHub, browser, wake,
tunnel, Scheduler, daemon/reload, release, credential store, or Git publication
operation was performed.

## Host acceptance

1. Reload only an isolated reviewed candidate, then require `CONNECTED_VERIFIED`.
2. Invoke BYOVD preflight only with the exact cached policy form.
3. Confirm only if every returned bounded preflight fact is positive and fresh.
4. Stop on any rejection or partial journal stage; reconcile without deletion or
   another repository identity. A Bug Bounty action requires separate approval.
