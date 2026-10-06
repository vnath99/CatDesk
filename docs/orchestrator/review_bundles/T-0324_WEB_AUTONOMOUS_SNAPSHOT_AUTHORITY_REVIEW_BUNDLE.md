# T-0324 WEB autonomous snapshot authority review

## Classification

`WEB_AUTONOMOUS_SNAPSHOT_AUTHORITY_READY`

This is a fresh evidence-only completion record for the existing reviewed
source snapshot path. It does not create a new trust domain or authorize a
reviewed build, release, wake, project-target, tunnel, browser, or host action.

## Accepted source identity and semantics

The accepted four-file WEB compatibility identity remains:

```text
sha256:725505e72b85614e6af8ab829f4ef6414c40614635d9cbd88f64d616d9629880
```

The durable accepted WEB review record identifies that exact current diff. The
current implementations in `src/delegated/autonomy_projects.rs`,
`src/delegated/autonomy_runtime.rs`, `src/mcp.rs`, and
`src/stable_wake_core.rs` retain the same bounded conversation predicate:

```text
legacy-id OR WEB:<legacy-id>
```

The legacy identifier remains nonempty, ASCII alphanumeric/hyphen/underscore,
and bounded to 200 bytes. `WEB:` is uppercase, its suffix must itself be a
nonempty legacy identifier, and the full prefixed segment remains at most 200
bytes. Project identifiers remain legacy-only. The four boundaries retain
HTTPS/allowed-host/userinfo/port/query/fragment/path-depth and encoded-
separator refusal. The designated canonical URL remains accepted by all four:

```text
https://chatgpt.com/c/WEB:1229263e-88d1-41a1-8ce2-b4aa97fbcb0f
```

The existing hostile suites continue to reject empty `WEB:`, lowercase
`web:`, extra colons, `WEB:` in project position, and prior malformed URL
forms. No product source or test file was modified in this task.

## Provider and model provenance

- Worker: current Codex API coding worker.
- Model family: GPT-5, as supplied by the executing Codex environment.
- Contract: `T-0324`, `fnv1a64:25cb5c950edd7b81`.
- No CatDesk MCP tool, fallback provider, credentials, signing authority, or
  external project was used.

## Local verification

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed; only the existing non-failing `C:\\Users\\Volap` canonicalization
  warning appeared.
- `cargo test --workspace --all-targets --all-features` — completed locally
  with 936 primary tests plus target-specific binaries and no reported failure.
- `git diff --check` — passed; inherited CRLF warnings were non-failing.
- `git status --short` — confirms the broad pre-existing dirty worktree;
  `autonomy_runtime.rs` and `mcp.rs` are modified and
  `autonomy_projects.rs`/`stable_wake_core.rs` are untracked in the baseline.
  Those accepted source changes were inspected but not altered.

## Attribution and prohibited-action audit

The sole task-attributable workspace mutation is this exact bundle. No product
source, test, script, protected `.catdesk` state, wake/project target, daemon,
reviewed-build PREPARE/CONFIRM/RESULT, release, browser, Secure MCP/tunnel,
Scheduler/service, Git, or external-project action occurred.

CatDesk must independently verify and emit the normal final review before this
completion record may be consumed by existing reviewed-source/reviewed-build
machinery.
