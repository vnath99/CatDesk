# T-0129-R1 Qwen 3.8 Continuation Compatibility — Independent Review

## Scope

This review closes the remaining first-class local-model inventory/probe portion of T-0135 after the Qwen continuation and bounded-history work was independently accepted in T-0136/T-0145.

## Implementation reviewed

- `src/mcp.rs` registers a new read-only MCP tool named `ollama_model_probe`.
- The tool accepts only an optional exact `exactModelId` field; additional properties are rejected.
- The tool does not accept a base URL, executable path, authentication value, shell command, or arbitrary network target.
- Runtime authority is the existing `OllamaAdapter::list_models()` `/api/tags` path against the fixed loopback URL `http://127.0.0.1:11434`.
- The call is bounded by a five-second host timeout.
- Output contains only bounded model IDs and non-secret availability/count/match metadata. The inventory is capped at 64 returned model IDs and each model ID is limited to 256 ASCII non-whitespace/non-control bytes.
- Exact-model proof is strict string equality. Duplicate exact identities fail closed as ambiguous. Invalid provider inventory fails closed without projecting raw provider errors.
- The MCP descriptor is explicitly `readOnlyHint=true`, `openWorldHint=false`, and `destructiveHint=false`.

## Deterministic coverage

`src/mcp.rs` now covers:

- accepted exact model ID syntax (`qwen3.8:27b`);
- empty, whitespace-containing, and oversized model IDs;
- bounded inventory truncation;
- invalid provider model inventory rejection;
- MCP registration and closed input schema;
- read-only/open-world/destructive annotations;
- the updated exact read-only and full tool-list ordering expectations.

## Independent verification

Fresh host verification after formatting:

- `cargo fmt --check` — PASS
- `cargo test` — PASS
- `cargo build` — PASS

The first verification attempt failed only `cargo fmt --check`; tests and build were already green. `cargo fmt` was then run through CatDesk's allowlisted command surface, followed by a fresh complete verification pass.

## Safety/invariants

- No Codex/cloud provider was invoked.
- No unrestricted shell was enabled.
- No Git publication, reset, cleanup, or dirty-workspace normalization was performed.
- No browser/wake/Scheduler action was performed.
- No Secure MCP tunnel ownership or configuration was changed.
- No external project was accessed or modified.
- Existing Qwen continuation safeguards, duplicate-tool suppression, bounded delegated reads, and the 49,152-byte provider-input ceiling remain unchanged.

## Live acceptance still required

The source implementation is independently verified. Final T-0135/T-0138 host acceptance still requires loading an isolated reviewed build and invoking the new read-only probe to positively prove exact installed model `qwen3.8:27b` through the MCP surface. That live step must preserve the externally owned Secure MCP tunnel and must not promote the candidate to canonical release as part of this review.
