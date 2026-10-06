# Qwen Worker Policy

This document is the canonical behavioral policy for CatDesk's local Qwen implementation worker. The executable authority remains CatDesk's controller and journal; this document explains the rules that the controller enforces.

## Purpose

Qwen is a bounded local implementation worker, not the supervisor or final reviewer. Its value is inexpensive parallel coding capacity while ChatGPT/CatDesk retain planning, safety, verification, attribution, and independent review authority.

## Tool surface

Keep Qwen's tool surface deliberately small. Qwen should use CatDesk-mediated bounded `read` and `search`, `patch.preview`, `patch.apply`, verification, and authoritative diff tools. Giving the model every CatDesk/MCP capability increases routing ambiguity and does not improve implementation reliability.

Qwen has no direct filesystem, shell, Git publication, browser, tunnel, or approval authority. It may not broaden paths, tools, budgets, or policy.

## Runtime profile

CatDesk treats Qwen 3.8 as a distinct inference profile instead of inheriting generic Ollama defaults. Normal implementation/tool turns run in non-thinking mode with Qwen's documented instruct sampling profile: `temperature=0.7`, `top_p=0.8`, `top_k=20`, `min_p=0.0`, `presence_penalty=1.5`, and `repeat_penalty=1.0`. CatDesk explicitly requests a 16,384-token Ollama context window so multi-turn tool history does not silently fall back to a small server default.

If the host must retry because a model/runtime does not support the non-thinking control, the Qwen 3.8 fallback uses the documented thinking sampling profile (`temperature=1.0`, `top_p=0.95`, `top_k=20`, `min_p=0.0`, `presence_penalty=0.0`, `repeat_penalty=1.0`) while keeping the same context floor. CatDesk should not enable expensive default/high reasoning on every routine tool turn; deeper reasoning is a supervisor-controlled resource for genuinely difficult diagnosis, not the normal execution mode.

The context floor is intentionally bounded rather than attempting Qwen's full native context. Source reads and tool results must still be compacted and bounded because a larger context window is not permission to accumulate unlimited repository state.

## Work intent

Delegated contracts are **mutation-required by default**. A supervisor may explicitly declare a non-mutating task by placing one exact marker in the objective:

- `[CATDESK_READ_ONLY]`
- `[CATDESK_NO_OP]`

The model may never self-declare or add these markers. This explicit opt-out avoids guessing intent from natural-language verbs while remaining backward-compatible with the v1 contract schema.

For a mutation-required contract, completion requires all of the following:

1. CatDesk journal evidence of at least one successfully applied `patch.apply` for this run.
2. Passed verification.
3. A captured `diff.actual`.
4. At least one final diff path that is attributable to a path changed by this run's successfully applied patch records.
5. The existing allowed-path, forbidden-path, outcome-unknown, and acceptance-criteria gates.

A clean test run or a pre-existing dirty-workspace diff cannot substitute for run-attributable implementation work.

## Reconnaissance / impatience policy

Qwen should inspect only enough source to choose the smallest safe change. For Qwen 3.8 mutation-required runs, CatDesk allows at most four successful `read`/`search` calls before the first successful `patch.preview`. A fifth reconnaissance call is rejected with an action-oriented instruction to patch.

Exact duplicate completed reads/searches remain suppressed. Once a patch proposal exists, Qwen should apply it or explicitly stop/escalate rather than continue unrelated reconnaissance. After a failed verifier, bounded source inspection is allowed to support a revised child patch.

The purpose of the budget is not to make Qwen guess. Four bounded reconnaissance calls are enough to inspect the target, nearby code, and one or two references in the normal bounded ticket. Tickets that genuinely require broad research should be split by the supervisor instead of giving the implementation worker an unbounded exploration problem.

## Completion and correction behavior

CatDesk must evaluate progress before accepting a completion claim. If Qwen tries to finish without an attributable mutation on a mutation-required task, CatDesk returns a concrete next action: make the smallest permitted `patch.preview`/`patch.apply`, then verify and capture the diff again.

If Qwen runs verification and captures a repository diff before making the required mutation, CatDesk must not tell it to provide final reasoning. It instead emits a progress-gate continuation explaining that verification/diff-only cycling cannot complete the ticket.

Malformed tool syntax, premature stops, failed verification, and missing final evidence remain bounded by existing corrective-turn and retry budgets. CatDesk, not Qwen, decides whether evidence is sufficient.

## Reliable ticket shape

Supervisors should give Qwen literal, bounded implementation contracts:

- state the exact objective and expected behavior;
- restrict allowed source/test paths;
- identify the first files/symbols worth inspecting when known;
- describe the smallest expected implementation surface;
- for `patch.preview`, require every operation to include its own `path`, `old`, and `new` fields; `targetPaths` is not a substitute for `operation.path`;
- state deterministic acceptance tests;
- explicitly use `[CATDESK_READ_ONLY]` for inspection-only jobs;
- avoid combining architecture discovery, large refactors, unrelated cleanup, and release work in one Qwen ticket.

Prefer one meaningful implementation change plus verification over many tiny hardening tickets. When Qwen exposes a systemic worker-loop failure, fix the controller/policy directly rather than repeatedly re-prompting Qwen with the same ineffective loop.

## Failure patterns this policy addresses

Observed CatDesk runs demonstrated the need for these gates:

- repeated bounded reads growing provider history without implementation progress;
- premature completion claims before required evidence;
- verification/diff-only runs reaching `COMPLETED_VERIFIED` against an already-dirty workspace without any `patch.preview` or `patch.apply`;
- a model following the existing final-diff instruction even though the diff was not attributable to the run.

These are controller-policy failures as much as model failures. The goal is to make the expected productive path the easiest path for Qwen while keeping final authority outside the model.
