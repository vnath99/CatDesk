# Architecture Decision: CatDesk-Owned Worker Runtime

Status: ACCEPTED
Date: 2026-07-25
Ticket: T-0013B

## Decision

CatDesk owns the v1 provider-neutral worker loop.

OpenClaw is preserved as optional research evidence and a possible future adapter, but it is not a required v1 runtime dependency. The T-0013A Gateway validation failed closed because the installed OpenClaw runtime did not expose a supported pre-model way to warm CatDesk MCP tools and prove the final worker-visible tool policy before a model turn.

The v1 loop is:

```text
ExecutionContractV1
-> bounded context
-> provider turn
-> normalized text/tool/completion event
-> deterministic CatDesk validation
-> CatDesk-owned tool execution
-> journaled bounded result
-> next provider turn
-> verified completion, escalation, cancellation, or failure
```

The model is an untrusted tactical reasoner. CatDesk is the trusted execution, policy, journal, patch, Git, verification, and disclosure boundary.

## Context

T-0012 proved CatDesk can expose a hardened read-only loopback MCP endpoint and OpenClaw can discover a filtered CatDesk MCP catalog through disposable configuration. T-0013 defined the initial execution contract, event envelope, approvals, and state machine. T-0013A proved OpenClaw Gateway authentication, structured RPC calls, and event frames without running a model worker, but failed the required pre-model effective-tool gate.

Required safety order:

```text
construct tools -> inspect final tools -> send model request
```

Observed OpenClaw behavior:

```text
send agent/model request -> MCP tools connect later -> inspect effective tools
```

CatDesk cannot infer worker safety from MCP discovery alone. The v1 architecture therefore removes OpenClaw from the critical path and makes CatDesk construct the exact tool definitions supplied to every provider request.

## Ownership

| Component | Owns | Must not own |
| --- | --- | --- |
| Local MCP client / future approved ChatGPT connector | Objective, execution contract review, escalation decisions, final review, approval to push or release | Local file, shell, Git, or patch execution |
| CatDesk coordinator | Contract validation, run lifecycle, approvals, budgets, cancellation, final verification, final review package | Provider-specific transport details |
| CatDesk worker runtime | Repeated model/tool loop, context assembly, tool-call normalization, bounded repair loops, checkpoints, provider handoff, worker events | Direct repository mutation without CatDesk policy checks |
| CatDesk policy and tools | Files, patches, shell profiles, Git staging/commit policy, jobs, tests, memory, repository maps, redaction | Model inference |
| Provider adapter | Transport to one model surface, streaming normalization, health, cancellation where supported, provider checkpoints where available | Repository access, shell access, Git mutation, approval decisions, acceptance verification |
| Worker model | Tactical reasoning, proposed tool calls, proposed patches, completion claims, blocker explanations | Policy decisions, contract changes, verification authority |

## Provider Boundary

All provider-specific behavior sits behind one adapter boundary. The coordinator and journal store provider-neutral state:

- run IDs, turn IDs, tool-call IDs, patch IDs, artifact IDs;
- bounded context bundle metadata;
- normalized provider events;
- normalized tool calls;
- checkpoint and handoff packets;
- patch proposal and actual diff artifacts;
- verification results.

Provider adapters receive only the bounded provider request selected by CatDesk. They never receive direct filesystem, shell, patch, Git, job, or verification authority.

## Required V1 Baseline

The first live provider is local Ollama/Qwen because it is a documented loopback transport suitable for proving CatDesk's own loop, journal, patch, idempotency, and verification behavior. Qwen is a baseline executor and test worker, not the permanent reasoning ceiling.

Future API and browser providers must plug into the same runtime contract. Browser providers, including SeleniumBase-controlled web sessions, remain deferred optional work and must not change the journal, patch engine, coordinator, or supervisor MCP protocol.

## Tool Surface Invariant

CatDesk constructs the complete model-visible tool surface for every provider turn. A provider may offer native tool-call syntax, but the allowed tools, schemas, names, descriptions, and policy checks are CatDesk-owned.

If a provider cannot accept structured tool definitions, CatDesk may require a strict tool-call envelope in model text. The parser must fail closed on malformed envelopes, unknown tools, duplicate IDs, invalid arguments, or attempts to smuggle commands through unrelated fields.

## Disclosure Policy

Local-only disclosure means bounded context remains on the machine and is sent only to a loopback provider such as Ollama. Remote disclosure means selected repository context leaves the machine through an API or browser provider and must be explicitly allowed by the execution contract.

CatDesk must redact secrets before provider disclosure, but redaction is not a substitute for scope control. Providers receive only selected paths, bounded ranges, summaries, hashes, and artifacts needed for the current turn.

## Migration From OpenClaw-Centered Plan

| Previous assumption | T-0013B replacement |
| --- | --- |
| Third-party runtime owns the model/tool loop | CatDesk owns the loop and delegates only provider turns |
| OpenClaw effective tools prove safety | CatDesk constructs and logs the exact tool definitions before each request |
| OpenClaw sessions are worker sessions | CatDesk run and worker-session records are authoritative |
| OpenClaw events are the main event cursor | CatDesk journal events are authoritative; provider events are normalized inputs |
| OpenClaw compaction/checkpointing is required | CatDesk context checkpoints are provider-neutral |
| MCP catalog warming is needed before a worker turn | CatDesk has no pre-model MCP warm-up dependency for v1 |

OpenClaw artifacts remain useful for future research, especially if a later version exposes a supported pre-model effective tool list and restart/resume controls.

## Consequences

Positive:

- CatDesk can prove the final model-visible tool surface before each provider request.
- The journal, patch lineage, idempotency, and final verification are not coupled to a third-party runtime.
- Provider replacement and handoff can be tested without rewriting repository mutation logic.
- The OpenClaw T-0013A failure becomes an architectural input, not a blocker.

Tradeoffs:

- CatDesk must implement its own durable worker loop, context compaction, provider adapter, and retry behavior.
- The first Ollama/Qwen worker may be less capable than stronger remote or browser models.
- Future browser providers still need separate reliability and disclosure hardening.

## Milestone Gate

Milestone A passes when:

- no required v1 contract assumes OpenClaw;
- provider-specific behavior is behind one adapter boundary;
- CatDesk constructs the complete model-visible tool surface;
- patch proposal, application, comparison, and actual diff evidence are explicit;
- network and context efficiency policies are documented;
- browser providers can be added later without changing the journal or coordinator.
