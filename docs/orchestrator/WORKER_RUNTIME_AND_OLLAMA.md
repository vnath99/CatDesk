# Worker Runtime And Ollama Adapter

Status: T-0016 implementation
Date: 2026-07-25

## Scope

T-0016 adds the first CatDesk-owned worker runtime primitives and a local Ollama adapter. It does not run repository mutations directly through a provider. Providers receive bounded context and CatDesk-owned tool definitions, then return normalized events.

## Runtime Harness

The initial runtime harness supports:

- worker session creation;
- repeated provider turns;
- status snapshots;
- safe-boundary cancellation;
- budget checks for turns, elapsed time, input bytes, and output bytes;
- checkpoint text in the session snapshot;
- restoration of active checkpointed runs through the T-0014 journal;
- durable T-0013 event emission;
- tool-call journaling through the T-0014 idempotency lifecycle.

The harness currently uses the fake provider for deterministic offline worker-loop tests. T-0017 will connect this loop to the patch/diff engine.

## Provider Event Normalization

Provider output normalizes into:

- text delta;
- tool call;
- completion claim;
- malformed response;
- cancellation acknowledgement;
- terminal provider error.

Malformed strict tool-call envelopes are rejected before tool execution.

## CatDesk-Owned Tool Surface

CatDesk constructs the tool definitions supplied to provider turns:

- `read`;
- `search`;
- `patch.preview`;
- `patch.apply`.

The provider receives schemas and may request tool calls, but it receives no direct filesystem, shell, Git, patch, or job authority.

## Structured Tool Calls

Strict text-envelope providers must emit exactly one JSON object:

```json
{
  "schema_version": "catdesk.tool-call.v1",
  "tool_call_id": "tc-read",
  "tool": "read",
  "arguments": {
    "path": "src/main.rs"
  }
}
```

The parser rejects surrounding text, unknown schemas, unknown tools, missing IDs, and non-object arguments.

## Ollama Adapter

The Ollama adapter supports:

- model discovery through `/api/tags`;
- non-streaming chat normalization through `/api/chat`;
- Ollama tool-call normalization;
- stream-mode request support for NDJSON text responses;
- configurable `keep_alive`.

The stream-mode helper requests Ollama streaming responses and parses returned NDJSON without adding new crate dependencies.

## Live Qwen Smoke

The live smoke test is ignored by default and must be run explicitly:

```powershell
cargo test delegated::runtime::tests::ollama_qwen_live_smoke_returns_normalized_response -- --ignored --nocapture
```

On 2026-07-25 it passed against:

- Ollama `0.24.0`;
- model `qwen3.5:9b`.

The smoke asks Qwen for a tiny text response and verifies it normalizes to a provider event. It does not expose repository files, shell access, Git access, or CatDesk mutation tools to the model.

## Offline Fake Provider

The fake provider supports deterministic tests for:

- multi-turn text/tool/completion scenarios;
- malformed output rejection;
- cancellation at a safe boundary;
- checkpoint restoration.

This keeps normal CI independent from local Ollama availability.
