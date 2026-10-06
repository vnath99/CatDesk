# T-0060F-W4 Safe Wake Target Review Bundle

## Delivered control plane

`catdesk_wake_target_set` is a first-class, narrow MCP mutation for only:

```json
{
  "conversationUrl": "https://chatgpt.com/c/<conversation-id>",
  "expectedCurrentTargetSha256": "optional 64-hex SHA-256"
}
```

The path is fixed to `.catdesk/wake-bridge/config.json`; callers cannot supply
a file, workspace, profile, state, inbox, or browser argument. The optional
compare-and-swap value is the SHA-256 of the current normalized target. Setter
operations are serialized inside the daemon, so a stale concurrent caller is
rejected before it can overwrite a newer target.

Successful responses return only the opaque target hash and normalized
`host/path` identity. They do not return browser, profile, storage, or wake
state data.

## Validation and persistence invariants

- Only bounded ASCII `https://chatgpt.com/c/<id>` targets are accepted;
  `chat.openai.com` remains supported for the existing compatibility surface.
- Userinfo, passwords, ports, queries, fragments, controls/whitespace,
  malformed URLs, non-conversation paths, encoded path separators, and
  oversized values are rejected.
- The bridge, profile-login helper, and Rust receipt validator now use the
  same exact `/c/<conversation-id>` target shape and canonical form.
- The fixed `.catdesk` and `wake-bridge` directories plus `config.json` must
  be regular, non-symlink/non-reparse paths. Configuration must be bounded,
  unambiguous JSON object data (including no duplicate root fields) with a
  valid existing target and profile field.
- Existing configuration values, including `profile_dir`, timing/debounce
  values, schema, and unrelated fields, are retained semantically. Only
  `conversation_url` changes.
- A same-directory, create-new temporary file is synced and renamed over the
  fixed target. CAS or write failure leaves the original config untouched.

## Changed paths

- `src/mcp.rs` — MCP schema/dispatch, target validation, fixed-path safe read,
  serialized CAS, atomic update, and deterministic tests.
- `src/delegated/autonomy_runtime.rs` — receipt validation now shares the
  exact canonical wake-target requirement.
- `scripts/wake_bridge.py` and `scripts/wake_profile_login.py` — enforce the
  same target shape before browser use.
- `tests/test_wake_bridge.py`, `tests/test_wake_profile_login.py` — strict
  target regressions.

## Deterministic evidence

Focused Rust coverage verifies successful update and field preservation;
stale CAS; malformed/non-HTTPS/userinfo/query/fragment/port/wrong-host/wrong-
path targets; malformed, oversized, and unsafe config conditions; temporary
collision atomic failure; no wake state/inbox mutation; and bounded MCP output.

Python execution remains environment-dependent: the current project-local
wake venv references a missing base interpreter. This task deliberately does
not rebuild or alter that operator-owned environment.

## Post-review operator sequence

1. Build and load the independently reviewed candidate.
2. Repair the project-local wake venv only if the operator chooses to do so.
3. Use `catdesk_wake_target_set` to bind the current ChatGPT conversation,
   providing the returned target hash on subsequent guarded changes.
4. Generate one fresh normal automatic canary.
5. Stop ChatGPT work so its UI is idle, then allow the automatic dispatcher to
   attempt the exact-conversation wake once.

No live browser action, daemon reload/promotion, tunnel action, Scheduler
change, credential access, or Git publication was performed for this repair.
