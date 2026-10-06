# CatDesk GitHub and Local Retention Policy

_Status: draft for cleanup/publication review — no deletion or remote publication authorized by this document._

## Goal

A fresh clone plus documented prerequisites should contain enough source, tests, architecture,
and reproducibility information to rebuild and understand CatDesk. Local runtime authority,
credentials, browser state, generated build products, and transient diagnostics must not be
mistaken for source-control authority.

## Track in Git

These are source/reconstruction inputs and should normally be versioned after review:

- `Cargo.toml`, `Cargo.lock`, `.gitignore`, `README.md`, and the public `catdesk.ps1` facade.
- `src/**`, including control-plane, reviewed-build/recovery, Binagotchy CLI, Wake integration,
  provider, tunnel, and Windows lifecycle code.
- `wake/Cargo.toml`, `wake/Cargo.lock`, `wake/src/**`, `wake/adapter.py`, and the reviewed
  Wake installer/source scripts required to build an immutable Wake package.
- Supported `scripts/**` and their tests. Historical one-off scripts should first be proven
  unreferenced, then either moved under an explicit legacy/archive location or removed in a
  separate reviewed cleanup.
- `tests/**` and `examples/**` that exercise product behavior.
- `docs/orchestrator/**`, including architecture decisions, ticket/review bundles, recovery
  traceability, retention reports, and sanitized evidence needed to understand why a trust or
  lifecycle boundary exists.
- `CATDESK_MILESTONES.md`, `CATDESK_NEW_CHAT_NOTES.txt`, and other sanitized durable handoff
  documentation whose purpose is project reconstruction rather than live runtime state.

Before first publication, inspect any one-off signing/key-generation helper such as
`KEYGEN.ps1` separately. Script source may be publishable, but no private key, credential,
machine-specific secret, or generated signing material belongs in Git.

## Keep local and ignored

These are runtime state, caches, generated output, or machine-specific evidence:

- `.catdesk/**`: autonomy sessions, review inbox runtime state, protected build-control state,
  current plan/session/todo runtime copies, logs, Wake bridge state, restart handoffs, local
  project/provider state.
- `.codex/**` and other provider-local session/configuration state.
- `target/**`, `target-verify/**`, `target-debug/**`, `wake/target/**`, and other Cargo
  build roots.
- Browser profiles, cookies, tokens, authenticated Selenium/Chrome state, local Wake runtime,
  and anything under the installed per-user Wake root.
- Local tunnel runtime aliases/configuration/credentials and installed tunnel-client binaries.
- `latest_logs/**`, startup/recovery diagnostics, one-off process dumps, probe output, and
  temporary build/link experiments unless their useful conclusions are distilled into a
  sanitized documentation artifact.
- Download/cache material and Python bytecode.
- Editor/repair backup files (`*.bak-before-*`) once their corresponding source change is
  reviewed.

## GitHub Release assets, not normal Git history

Once stable releases exist, large immutable binaries may be retained as GitHub Release assets
with hashes/manifests rather than committed into the repository:

- reviewed/promoted `catdesk.exe` release images;
- immutable reviewed Wake packages;
- signed release manifests/attestations intended for distribution.

Release assets must be reproducibly tied to reviewed source and must never substitute for the
local protected promotion/LKG authority used by CatDesk recovery.

## Root-folder cleanup policy

The root should converge toward a small operator-facing surface:
- `catdesk.ps1`
- Cargo manifests/lock
- README / durable project docs
- source/test/script/documentation directories.

Historical root launch/reload helpers and numbered bootstrap scripts are not routine entry
points. Before moving/removing them:
1. search source/scripts/docs/tests for references;
2. verify the public facade and current recovery path do not invoke them;
3. classify them as current dependency, retained historical migration evidence, or disposable;
4. move retained historical helpers under a clearly named legacy/archive directory rather than
   leaving multiple apparent launch authorities in the root;
5. delete only from a digest/metadata-bound cleanup manifest.

## Storage cleanup

Reuse the T-0056/T-0057 safety model:
- metadata-only inventory first;
- no reparse traversal;
- known Cargo signatures required before classifying a build root as rebuildable;
- protected/runtime roots are denylisted;
- unknown material is retained;
- frozen pre-cleanup manifest with byte counts/digests where applicable;
- cleanup produces per-root receipts and a post-cleanup inventory.

Current obvious *candidates for measurement*, not deletion authority, include repeated
`target-verify` roots such as old retry-lineage builds, T-0398 probes, repeated T-0401
scrubbed builds, the superseded V4 daemon build, and older verification targets. The active
V5 controller build and any executable currently serving CatDesk must be excluded.

## Publication sequence

1. Finish reviewed BUILD_ATTESTED -> promotion -> durable reviewed-promotion LKG and prove
   one-command recovery.
2. Finish Wake dev.50 and restore reliable event-driven Wake.
3. Refresh storage inventory and independently review the cleanup manifest.
4. Apply non-destructive root organization and cleanup.
5. Review Git candidate set for secrets/machine-specific paths.
6. Commit coherent source/documentation history.
7. Push the feature branch through the existing authenticated Git CLI.
8. Use GitHub Release assets for reviewed binaries only after source/release parity is proven.
