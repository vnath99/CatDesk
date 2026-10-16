# T-0485 R37 — Independent ChatGPT queue review identity boundary

Date: 2026-10-16. Source audited: `06f347d3e3bf9a87842f6c4d124ca4a9b65f00c4` (tree `9a11122271792b741b607175b2784c7b613ea114`). Status: `READ_ONLY_SOURCE_AUDIT`; independent ChatGPT review, decision, ACK, Wake delivery, and next-task authority remain open.

## Finding

The exact queue package can be disclosed for read-only inspection under a local operator grant, but the current MCP request cannot establish which ChatGPT conversation or assistant turn read it. A proposed decision or ACK carried in another request would be a claim by the same shared MCP principal, not independently authenticated review. No source route should turn that claim into Wake or next-task authority.

| Source | Observed boundary |
| --- | --- |
| `src/operator_codex_cli/queue/review_reader.rs:895-1022` | The reader checks the current disclosure grant, registered package and exact `packageSha256`, then returns bounded untrusted historical evidence with `independentReview=PENDING` and `wakeReady=false`. The grant authorizes disclosure to the shared MCP principal; it does not identify a reviewer. |
| `src/server.rs:267-280,2045-2051,2114-2123,2161-2172` | `POST /mcp` compares one optional static bearer token and passes a JSON-RPC request to the MCP handler without a platform-authenticated caller, conversation or turn identity. With no token configured, this function allows the request; neither case proves reviewer independence. |
| `src/mcp.rs:134-140,487-530,749-769,1355-1364,8313-8389` | JSON-RPC contains method, params and id. The package-read tool accepts caller-provided project/receipt/digest/page selectors, and its own description disclaims conversation identification, independent review, inbox event and Wake authority. The handler has no trusted reviewer context to attach to a decision. |
| `src/delegated/autonomy_supervisor.rs:3489-3494`; `src/delegated/autonomy_state.rs:2174-2191` | The existing autonomy review-inbox ACK marks an unrelated record `unread=false`. It does not attest a queue package digest, ChatGPT reviewer, review verdict or assistant turn. |
| `wake/src/store.rs:99-107`; `wake/adapter.py:120-150,190-194` | Wake's receipt binds event, target generation/digest and USER message digest. The adapter confirms the exact USER message append and may observe response completion. Neither observation captures assistant decision content or independent reviewer identity. |
| `src/operator_codex_cli/queue/review_invite.rs:1-9,191-200` | The persisted invitation is historical `NOT_PUBLISHED` evidence; it has no protected Wake target generation and creates no ChatGPT ACK, independent decision or next-task authorization. |

An agent holding the shared token could read the package and send a self-authored verdict with any claimed `conversationId`, `reviewer`, or `ACK`. The server would have no trusted fact that distinguishes it from a separate ChatGPT review. A Wake `SENT` receipt or completed response timer would only show invitation delivery/turn activity, so neither may be promoted to approval.

## Minimum trusted attestation interface before implementation

A separately provisioned verifier must receive a platform-authenticated assertion or signed callback from a trust root outside the producing queue process and shared MCP token. The assertion must bind the actual reviewer conversation and assistant turn, identify its relation to the exact registered project ChatGPT target, and distinguish that reviewer from the producer. It must bind the immutable queue receipt and exact verified package SHA-256 to a stated review decision and its durable ACK or event identity, with a purpose, audience, issue/expiry, and replay-resistant nonce or unique event ID. The verifier must check authenticity and scope rather than trusting identity fields supplied in ordinary MCP params or document text.

Before any decision becomes current authority, the consumer must recheck the package digest, live disclosure/revocation state, current registered project/target and relevant reviewer binding. Any later Wake publisher must independently prove the installed protected Wake store, current target generation, exact target and current authorization, and then obtain a fresh delivery receipt. A historical invitation, review read, assertion replay, inbox unread flag, USER receipt, or timer alone cannot authorize a next task.

No such authenticated ChatGPT reviewer assertion or trusted callback is present in the audited source. This checkpoint adds no decision endpoint, ACK promotion, Wake mutation, live queue operation, or security-setting change. A same-user local operator opinion remains `OPERATOR_REVIEW_ONLY` and does not satisfy the independent ChatGPT gate.
