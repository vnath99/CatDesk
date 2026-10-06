# T-0036-R9 Current Conversation Cached-Schema Bridge Review Bundle

## Design

The existing exposed `autonomy_project_registry_bind` operation now supports a third, mutually exclusive compatibility shape for an already-open connector with cached arguments:

```text
projectId=<registered-project>
decision=CHAT_TARGET_URL=https://chatgpt.com/c/<conversation-id>
expectedSha256=<current-target-digest>
```

`decision` is accepted only when it starts immediately with the exact fixed `CHAT_TARGET_URL=` prefix and the remainder is already the exact canonical approved ChatGPT conversation URL. The value is passed to the existing central target validator and CAS implementation. No generic decision channel or new mutation tool was added.

## Security boundaries

This mode accepts only `projectId`, `decision`, and `expectedSha256`. It rejects absent digest, stale digest, whitespace, prefix alteration, preamble/suffix text, malformed URL, newer T-0097 target fields, legacy thread/workspace/Git/profile/resolution fields, and arbitrary unrelated fields. It returns only project ID and the resulting non-secret target digest. The T-0097 `conversationUrl` mode and legacy thread-binding mode remain separate and unchanged.

## Deterministic coverage

Tests cover a valid cached project-conversation CAS update, stale digest, altered prefix, whitespace, suffix/preamble, malformed query URL, mixed/unrelated field rejection, preserved canonical thread binding, T-0097 target mode, legacy mode, and schema exposure of all three shapes.

## One-time live invocation

After the external project is already registered, invoke the existing connector tool exactly once with the three cached fields above, replacing the URL and digest with the operator-approved project target and the current durable target digest. Do not include any other field. Persist the returned digest and let normal project-scoped dispatch select it. Do not use a shell, browser, manual wake bridge, global fallback, or additional retry from this worker.
