# T-0036-R8 Static-Catalog Chat Target CAS Bridge Review Bundle

## Design

The already exposed `autonomy_project_registry_bind` operation now accepts exactly one of two request shapes:

- the unchanged legacy direct thread-binding shape; or
- a project chat-target CAS shape containing only `projectId`, exact `conversationUrl`, and optional `expectedCurrentTargetSha256`.

The target mode delegates directly to the reviewed central `bind_project_chat_target` URL validator and compare-and-swap implementation. It is not a generic project mutation surface. Its output is bounded to `projectId` and non-secret `targetSha256`; no conversation URL, workspace, thread ID, Git identity, or profile data is returned.

## Security boundaries

Target mode rejects missing/partial fields, all mixed legacy thread/workspace/Git/profile/resolution fields, arbitrary unrelated fields, malformed or non-conversation URLs, query/fragment/userinfo/port/host drift, stale digests, and absent projects. Legacy binding is selected only when target fields are absent and retains its existing behavior. No new MCP operation, shell path, executable/auth selection, protected-state access, browser action, or global wake fallback was added.

## Deterministic coverage

Tests cover legacy thread-binding compatibility, valid direct/project conversation target CAS, stale expected hash, partial and mixed mode rejection, invalid URL forms, missing/wrong project rejection, unrelated-field rejection, bounded response shape, and schema `oneOf` exposure. Existing central target CAS, W13/R7 receipt, and project uniqueness regressions remain in the full suite.

## Follow-up live binding sequence

For a reviewed external project, use the exposed bind operation in target mode only after its project registration has completed. Submit `projectId` and the exact approved conversation URL, optionally with the durable current digest for CAS. Persist and compare the returned target digest; then let normal project-scoped automatic dispatch select that record. Do not include thread/workspace/Git/profile fields in target mode and do not use a shell, manual wake bridge, browser, or global fallback. This worker performed no live binding or external-project mutation.
