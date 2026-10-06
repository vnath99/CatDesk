# T-0324 WEB-prefixed chat-target compatibility review

## Classification

`WEB_PREFIXED_CHAT_TARGET_COMPATIBILITY_ACCEPTED`

This exact predeclared completion artifact is a source/test review result only.
It requires CatDesk independent final review and authorizes no live wake,
project-registry, browser, release, tunnel, or serving action.

## Four-boundary grammar review

The current exact-chat boundaries in `src/delegated/autonomy_projects.rs`,
`src/delegated/autonomy_runtime.rs`, `src/mcp.rs`, and
`src/stable_wake_core.rs` use the same accepted conversation grammar:

```text
legacy-id OR WEB:<legacy-id>
```

`legacy-id` is still nonempty, no more than 200 bytes, and restricted to ASCII
alphanumeric, hyphen, and underscore. `WEB:` is uppercase, requires a
nonempty legacy-valid suffix, and the full conversation segment remains at
most 200 bytes. Project identifiers retain only the legacy grammar.

Each boundary still rejects unsupported scheme/host/userinfo/port/query/
fragment forms, path-depth changes, and encoded separators. Canonical paths
remain exactly `/c/<conversation>` or
`/g/<legacy-project>/c/<conversation>`.

## Required positive and hostile evidence

All four boundaries canonically accept exactly:

```text
https://chatgpt.com/c/WEB:1229263e-88d1-41a1-8ce2-b4aa97fbcb0f
```

Focused tests retain legacy acceptance and reject empty `WEB:`, lowercase
`web:`, an extra colon, `WEB:` in a project-id position, and the prior invalid
scheme, credential, port, query, fragment, host, path-depth, trailing-slash,
and encoded-separator cases where applicable. No focused test or source defect
was found; no source/test edit was made.

## Verification and attribution

- `cargo fmt --all -- --check` passed.
- Strict workspace/all-target/all-feature Clippy passed, with only the
  existing non-failing `C:\\Users\\Volap` canonicalization warning.
- Focused `wake_target_set_rejects_invalid_urls_without_mutating_config`
  passed.
- `cargo test --workspace --all-targets --all-features` completed locally with
  936 primary tests plus target-specific binaries and no reported failure.
- `git diff --check` passed; inherited CRLF warnings were non-failing.

The worktree remains broadly dirty. The current diffs in
`autonomy_runtime.rs` and `mcp.rs` are inherited; `autonomy_projects.rs` and
`stable_wake_core.rs` are untracked in this baseline. Nothing was reset,
staged, committed, or altered outside this exact completion artifact.

No protected wake/project state access or mutation, browser action, daemon or
release action, reviewed-build action, tunnel/Secure MCP action, Git
publication, or external-project mutation occurred.

## Next boundary

CatDesk independent final review must consider this exact predeclared output
before any separately authorized designated-target operation. This review does
not itself grant that operation.
