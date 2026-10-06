# T-0324 WEB-prefixed chat-target compatibility independent review

## Classification

`WEB_PREFIXED_CHAT_TARGET_COMPATIBILITY_ACCEPTED`

This is a source/test review result that requires CatDesk independent final
review. It does not authorize a live target, wake, browser, release, tunnel,
or project-registry action.

## Exact shared grammar

The authoritative current implementations in
`src/delegated/autonomy_projects.rs`, `src/delegated/autonomy_runtime.rs`,
`src/mcp.rs`, and `src/stable_wake_core.rs` have the same conversation-segment
predicate:

```text
legacy-id OR WEB:<legacy-id>
```

`legacy-id` remains nonempty, at most 200 bytes, and ASCII
alphanumeric/hyphen/underscore only. `WEB:` is uppercase and must be followed
by that same nonempty legacy identifier; the total conversation segment remains
at most 200 bytes. The project-position segment still uses only `legacy-id`.

All four retain the existing fail-closed URL rules: ASCII bounded input,
HTTPS only, `chatgpt.com` or `chat.openai.com` only, no username/password or
port, no query/fragment, and exactly `/c/<conversation>` or
`/g/<legacy-project>/c/<conversation>`. The segment parsing rejects additional
path depth and encoded separators because `%` is outside the legacy identifier
alphabet.

## Compatibility evidence

Each trust boundary accepts and canonicalizes identically:

```text
https://chatgpt.com/c/WEB:1229263e-88d1-41a1-8ce2-b4aa97fbcb0f
```

The focused regressions retain legacy conversation acceptance and reject:

- empty `WEB:`;
- lowercase `web:`;
- an extra colon after the `WEB:` payload;
- `WEB:` in the project-id position;
- prior invalid scheme, credentials, port, query, fragment, host, path-depth,
  trailing-slash, and encoded-separator forms where applicable.

The project, runtime receipt, MCP wake configuration, and stable wake core
implement the same predicate and canonical output. No concrete source or test
defect was found, so no source/test mutation was made.

## Local verification and attribution

- Focused `wake_target_set_rejects_invalid_urls_without_mutating_config` —
  passed.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
  passed, apart from the existing non-failing `C:\\Users\\Volap`
  canonicalization warning.
- `cargo test --workspace --all-targets --all-features` — completed locally
  with 936 primary tests plus target-specific binaries and no reported test
  failure.
- `git diff --check` — passed; inherited CRLF warnings were non-failing.

The worktree is broadly dirty. The current diff for `autonomy_runtime.rs` and
`mcp.rs` contains substantial inherited work, while the other inspected source
files are untracked in this baseline; none was reset, staged, committed, or
edited here. The sole attributable mutation is this review bundle.

## Prohibited-action audit and next boundary

No protected wake/project-state access or mutation, browser action, daemon or
release action, reviewed-build action, tunnel/Secure MCP action, Git
publication, or external-project mutation occurred.

CatDesk independent final review is required before any separately authorized
live designated-target operation. This review itself creates no such
authority.
