# T-0249 / T-0224-R1A-R7 — Genuine Stable Host Runtime Independence

## Independent review request

Independent final review is requested.  Controller status is advisory.  Review
the standalone executable, its child-process fixtures, and the bounded durable
input contract rather than any in-process daemon status path.

## Why T-0248 was insufficient

T-0248 kept its only non-test caller behind
`server::post_mcp -> AppState::transport_status_payload`.  Its adverse matrix
only repeated labels and did not change host fixture state.  This slice adds an
OS-process executable and invokes it against actual isolated filesystem states.

## Standalone host and invocation

New binary: `src/bin/catdesk-stable-wake-host.rs`

```
catdesk-stable-wake-host --workspace <workspace>
catdesk-stable-wake-host --workspace <workspace> --expected-target-sha256 <64-lower-or-upper-hex>
```

It is compiled as its own Cargo binary and has no `crate::`, `AppState`,
server, MCP, daemon, release, provider, browser, or Secure-MCP import/call
path.  It accepts only one workspace path and, optionally, an opaque digest
that can constrain the existing target but cannot select a replacement.

On success the entire stdout payload is bounded JSON:

```json
{"status":"READY","actionableCount":1,"staleCount":0,"targetSha256":"<sha256>"}
```

On refusal it emits only a bounded reason vocabulary and exits with code 2.
It never emits a conversation URL, record, profile, credential, token, or
unbounded path.

## Shared canonical authority

`src/stable_wake_core.rs` is a dependency-free shared read-only contract.
Both the standalone host and the existing in-process status adapter consume it.
It is the single stable parser for:

1. `<workspace>/.catdesk/autonomy/review-inbox.json`, the sole event source;
2. `<workspace>/.catdesk/wake-bridge/config.json`, the existing exact target
   authority; and
3. the fixed project identity `catdesk`.

No duplicate spool or producer exists.  The inbox uses producer-compatible
camelCase fields and the bounded 512-record array.  Safe nested references
such as `artifacts/completion.json` remain accepted; empty, rooted, prefixed,
backslash, traversal, NUL, or oversized references are refused.  Fixed path
components and final inputs are checked with `symlink_metadata`, canonicalized,
contained under their pinned parent, and must be regular non-link/non-reparse
objects where applicable.

Unread `COMPLETED_VERIFIED/independent_final_review` and
`WAITING_FOR_CHATGPT/chatgpt_decision_required` records are actionable.  All
other valid records are stale.  Exact record duplicates collapse; semantic
same-ID conflicts refuse the whole input.

The target parser preserves protected-config rules: bounded strict JSON with
duplicate-key rejection, regular contained config file, required bounded
profile field, canonical supported HTTPS ChatGPT conversation URL, no userinfo,
port, query, or fragment, and SHA-256 only in the host result.  The host reads
then revalidates the same target; an optional previously observed digest proves
target drift fail-closed without creating, inferring, migrating, or replacing a
conversation.

## Process-level evidence

`tests/stable_wake_host.rs` launches the compiled executable through
`Command`, never through `catdesk.exe` or an AppState fixture.

| Real isolated fixture condition | Child result |
| --- | --- |
| `target/release/catdesk.exe` absent; no daemon/listener/tunnel fixture | `READY` |
| replacement non-CatDesk `target/release/catdesk.exe` file | unchanged `READY` JSON |
| malformed `.catdesk/reviewed-release/manifest.json` | unchanged `READY` JSON |
| stale/malformed `.catdesk/promotion/lkg.json` | unchanged `READY` JSON |
| missing, malformed, duplicate-key, oversized target config | `REFUSED` |
| target changed after observed digest | `REFUSED`, `wake target binding drifted` |
| malformed/oversized inbox, wrong project/schema, invalid identity | `REFUSED` |
| traversal/rooted/oversized reference; conflicting duplicate; 513 records | `REFUSED` |
| final inbox replaced with directory/non-regular object | `REFUSED` |

The fixture explicitly compares inbox and config bytes before and after child
evaluation.  The child neither acknowledges nor rewrites either durable input.
No browser, interactive desktop, service, registry, ProgramData, daemon,
release, tunnel, or host mutation is used.

## Dependency regression

The child-process test source-scans production portions of the standalone bin
and shared core.  It rejects `crate::`, `super::`, `mod server`, `mod mcp`, and
`AppState`; it also proves the core names `review-inbox.json` but not the
rejected `.catdesk/stable-wake/review-events` authority.  The standalone source
uses only standard library process arguments plus the local shared core.

## Attributable files

- `src/stable_wake_core.rs` — new shared, read-only canonical inbox and
  protected-target parser/evaluator.
- `src/bin/catdesk-stable-wake-host.rs` — new independently invokable host.
- `src/stable_wake_bootstrap.rs` — in-process adapter now delegates to the
  shared parser rather than retaining a second implementation.
- `src/main.rs` — shared core module wiring for the existing status adapter.
- `src/mcp.rs` — T-0248-only read identity façade removed; the standalone core
  is now the stable read-only target authority.  Existing explicit target
  update functionality remains separate and was not invoked.
- `tests/stable_wake_host.rs` — isolated actual child-process matrix and
  source-dependency regression.
- this review bundle.

The worktree was already broadly dirty.  Attribution is limited to the listed
new files and the described adapter/module/target-reader hunks; no broad
worktree diff is presented as task evidence.

## Verification evidence

| Command | Result |
| --- | --- |
| `cargo test --test stable_wake_host` | passed: 4 child-process tests |
| `cargo test stable_wake_core` | passed: 7 shared canonical parser tests |
| `cargo test stable_wake_bootstrap` | passed: 3 adapter tests |
| `cargo fmt --check` | passed |
| `cargo clippy --all-targets --all-features -- -D warnings` | passed |
| `cargo test --quiet` | passed: 725 main + 15 supervisor + 8 host-bin + 2 recovery + 4 host integration + 2 measurement; 21 ignored |
| `cargo build` | passed |
| `cargo build --release --bin catdesk-stable-wake-host` | passed |
| repository `rust_full` equivalent (`fmt`, `test`, `build`) | passed as the project-defined commands |
| `git diff --check` | passed |

## Prohibited actions and R1B boundary

No live `.catdesk` state was read or changed; all durable files were isolated
test fixtures.  No browser delivery, claim/receipt, target update, stable host
installation, ProgramData/Scheduler/service action, daemon/release
reload/promotion, external Secure MCP/tunnel mutation, Git publication,
signing, or provenance work occurred.

R1B remains browser/desktop delivery, durable claim/receipt semantics, and
stable host installation/ownership.  Those authority-bearing actions are not
implemented by this executable.

## Mechanical result

**PASS:** the independently invokable, read-only host evaluates only the
canonical inbox and exact protected target config under actual adverse process
fixtures.  Independent host review remains required for acceptance.
