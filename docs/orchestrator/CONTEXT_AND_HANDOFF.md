# Context, Compaction, And Handoff

Status: T-0015 implementation
Date: 2026-07-25

## Scope

T-0015 adds provider-neutral context assembly primitives for the CatDesk-owned worker loop. The implementation is deterministic, bounded, and disclosure-aware. It does not call a live provider.

## Context Classes

| Class | Purpose | Examples |
| --- | --- | --- |
| `PERMANENT` | Stable constraints that must survive compaction | execution contract summary, repository instructions |
| `TASK_STABLE` | Durable task state that changes infrequently | checkpoint summaries, artifact references |
| `TURN_DYNAMIC` | Current-turn evidence | ranged file excerpts, bounded command output, recent tool results |

## Context Item Types

- contract summary;
- repository instruction;
- file excerpt;
- command output;
- artifact reference;
- checkpoint summary.

Each item records a source, summary, content hash, byte count, estimated token count, prompt-injection label, and redaction status.

## Bounded File Access

Context assembly uses ranged file excerpts, not whole-repository payloads. Excerpts require:

- a contained relative path;
- a valid line range;
- a maximum line count from `ContextBudgetPolicyV1`;
- content hashing.

Repository instruction discovery currently checks stable instruction files such as `AGENTS.md`, `CLAUDE.md`, `CODEX.md`, and `.github/copilot-instructions.md`. These files are labeled as untrusted project content to preserve prompt-injection awareness.

## Command Output Policy

Command output is bounded by `max_command_output_bytes`. If the same raw output appears again in the same context builder, CatDesk sends only an artifact-style hash reference instead of repeating the raw text.

Large unchanged outputs can therefore be represented by source, summary, and hash across turns instead of repeatedly consuming provider context.

## Budget And Provider Limits

`ContextBudgetPolicyV1` owns generic CatDesk limits:

- maximum bundle bytes;
- maximum item bytes;
- maximum excerpt lines;
- maximum command-output bytes;
- maximum estimated tokens;
- disclosure policy.

`ProviderContextLimitsV1` owns provider-specific limits behind a generic boundary. The context policy must fit inside provider limits before a request can be sent.

## Disclosure Policy

The default policy is `LOCAL_ONLY`. Remote API or browser providers require `REMOTE_ALLOWED`; otherwise validation fails before any provider request is assembled.

The redacted inspection view removes common token, password, API key, and secret assignments before display or provider handoff review.

## Compaction

Compaction rebuilds a bounded bundle from:

- the execution contract summary;
- a checkpoint summary;
- selected recent task-stable or turn-dynamic context items.

The checkpoint preserves mandatory constraints:

- objective;
- allowed paths;
- forbidden paths;
- acceptance criteria;
- current step;
- latest checkpoint;
- unresolved failure;
- remaining budgets;
- recent artifacts.

## Provider Handoff

`ProviderHandoffContextV1` carries compact state from one provider adapter to another:

- source provider;
- destination provider;
- checkpoint;
- compacted context items;
- pending tool-call IDs;
- disclosure policy.

The handoff does not replay completed mutations. Tool execution remains CatDesk-owned.

## Implementation

Rust module:

- `src/delegated/context.rs`

Export:

- `src/delegated/mod.rs`

## Verification Coverage

Focused tests cover:

- bounded and hashed file excerpts;
- prompt-injection labeling for repository instructions;
- unchanged command output referenced instead of resent;
- explicit remote/browser disclosure;
- compaction preserving objective and restrictions;
- replacement provider handoff from compact checkpoint;
- redacted context-inspection view.
