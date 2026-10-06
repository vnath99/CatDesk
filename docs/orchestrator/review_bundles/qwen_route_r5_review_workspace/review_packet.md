# Independent review packet — T-0364 Qwen initial route R5

This packet is intentionally self-contained so the local delegated reviewer can assess the exact narrow source change without access to the parent workspace. It is review-only and not deployment authority.

## Objective

Review whether a fresh CatDesk autonomous contract can explicitly start on the already-existing local routine provider (Ollama/Qwen) without:
- launching Codex,
- fabricating Codex exhaustion,
- creating a provider handoff,
- changing historical Codex-default behavior,
- creating a new provider/trust domain,
- weakening workspace/Git/lease/verification/review boundaries.

## Exact source changes

### autonomous_contract.rs

Validation changed from accepting only `chatgpt_web_codex_autonomous` to:

```rust
if !matches!(
    self.mode.as_str(),
    "chatgpt_web_codex_autonomous" | "chatgpt_web_qwen_autonomous"
) || self.objective.trim().is_empty()
    || self.objective.len() > 8_000
{
    return Err(ContractPolicyError::Validation(
        "autonomous contract mode or objective is invalid".into(),
    ));
}
```

New helper:

```rust
pub fn starts_on_routine_provider(&self) -> bool {
    self.mode == "chatgpt_web_qwen_autonomous"
}
```

Regression asserts historical Codex mode remains valid/default, explicit Qwen mode is valid, and an unknown mode is rejected.

### autonomy_supervisor.rs

Immediately after normal session creation and before contract persistence:

```rust
let mut snapshot = self
    .store
    .create_session(&session_id, queue)
    .map_err(|_| "autonomous session already exists or could not be created".to_string())?;
if contract.starts_on_routine_provider() {
    snapshot.provider_route = AutonomousProviderRouteV1::QwenFallbackActive;
    if self.store.save_session(&snapshot).is_err() {
        let _ = self.store.remove_unapproved_session(&session_id);
        return Err("autonomous routine-provider route could not be persisted".into());
    }
}
if self.store.save_contract(&session_id, &contract).is_err() {
    // existing cleanup path continues
}
```

Regression creates two contracts:
- `chatgpt_web_qwen_autonomous` -> persisted `QwenFallbackActive`.
- historical/default contract -> remains `CodexPreferred`.

### autonomous_controller.rs

The existing Qwen branch remains guarded by provider=Ollama and persisted route=QwenFallbackActive, but now distinguishes deliberate initial-Qwen mode:

```rust
} else if self.provider.provider_id() == ProviderIdV1::Ollama
    && snapshot.provider_route == AutonomousProviderRouteV1::QwenFallbackActive
{
    if self.policy.contract().starts_on_routine_provider() {
        format!(
            "This approved CatDesk task explicitly selected the local routine provider. Approved objective:\n{}\n\nOrdered work:\n{}\n\nExecute only task {} inside the approved workspace. Do not change Git branches, publish Git changes, access credentials, or modify files outside the contract. CatDesk independently verifies completion.",
            bounded_contract_text(&self.policy.contract().objective, 4_096),
            self.policy.contract().ordered_steps.join("\n"),
            task.task_id
        )
    } else {
        let handoff = self
            .store
            .load_provider_handoff(&self.session_id)
            .map_err(RuntimeError::from)?;
        format!(
            "Continue the existing approved CatDesk task after Codex plan-credit exhaustion. Do not repeat completed work. CatDesk verification remains authoritative. Completed task IDs: {}. Continue from the first incomplete ordered step:\n{}",
            handoff.completed_task_ids.join(", "),
            handoff.ordered_steps.join("\n")
        )
    }
}
```

Controller regression:
- changes contract mode to `chatgpt_web_qwen_autonomous`;
- persists route `QwenFallbackActive`;
- explicitly clears provider thread / expected Codex thread / Codex eligibility;
- records approved contract hash;
- proves no provider handoff exists before execution;
- one `run_once` launches Ollama only;
- deterministic first Qwen completion requeues normally, provider_turn_count becomes 1;
- no Codex launch occurs;
- no provider handoff exists afterward.

## Verification evidence

- cargo fmt --check — PASS.
- focused provider-mode test — PASS.
- focused supervisor route-persistence test — PASS.
- focused controller no-Codex/no-handoff test — PASS.
- autonomous_contract tests — 9/9 PASS.
- autonomy_supervisor tests — 26/26 PASS.
- autonomous_controller tests — 52/52 PASS.
- cargo clippy -q --bin catdesk -- -D warnings — PASS.
- git diff --check — PASS (only ordinary Windows LF/CRLF warnings).

Logs:
- .catdesk/logs/1789866726-58477cbd-7e17-4b2b-9f14-88aabb1f860d.log
- .catdesk/logs/1789866838-9562e4da-b2a8-4bf7-a81e-b11db9160dda.log
- .catdesk/logs/1789866851-bc0961ee-55a7-40d6-901e-97aec50695d9.log
- .catdesk/logs/1789866857-31156c1d-71e6-49fe-ab2b-0f6076443a82.log
- .catdesk/logs/1789866869-b38d0f52-01ab-4286-9420-444f1c071ec1.log
- .catdesk/logs/1789866897-f460405d-17c8-4b73-9669-1b880e858186.log
- .catdesk/logs/1789866902-a1ed2391-fedd-4158-8831-d7b8cef0e0be.log

## Review questions

1. Does the explicit mode remain fail-closed and backward compatible?
2. Does reusing QwenFallbackActive introduce a security/provenance ambiguity that should block deployment?
3. Is setting the provider route before approval/start safe, given the session is still unapproved and provider execution remains gated by normal approval/start?
4. Is the deliberate-Qwen controller branch sufficiently distinct from true Codex-exhaustion fallback?
5. Is any synthetic exhaustion/handoff evidence created? It should not be.
6. Does this change alter any Git, filesystem, provider, model, lease, verification, or review authority beyond initial provider selection?
7. Identify any concrete correctness/security bug that should be fixed before a development serving cutover.

## Required reviewer output

Create only `independent_review.md` in this review workspace. State PASS or NEEDS_FIX, concrete findings, and residual risks. Do not modify this packet.
