# Fault-Injection And Security Test Matrix

Status: T-0022 implementation note
Date: 2026-07-25

## Purpose

T-0022 adds a deterministic test harness for the delegated CatDesk worker loop.
The tests exercise the security and recovery promises from T-0014 through
T-0021 without requiring a live browser adapter or a remote model.

## Scenario Matrix

| Scenario | Test Evidence |
| --- | --- |
| Qwen-style bug fix and multi-file task | `fault_injection::tests::qwen_style_multi_file_bug_fix_and_patch_revision_are_verified` |
| Malformed tool call | `fault_injection::tests::malformed_tool_call_forbidden_path_command_stale_patch_and_conflict_fail_closed` |
| Forbidden path | `fault_injection::tests::malformed_tool_call_forbidden_path_command_stale_patch_and_conflict_fail_closed` |
| Forbidden command | `fault_injection::tests::malformed_tool_call_forbidden_path_command_stale_patch_and_conflict_fail_closed` |
| Stale patch | `fault_injection::tests::malformed_tool_call_forbidden_path_command_stale_patch_and_conflict_fail_closed` |
| Patch conflict | `fault_injection::tests::malformed_tool_call_forbidden_path_command_stale_patch_and_conflict_fail_closed` |
| Patch revision comparison | `fault_injection::tests::qwen_style_multi_file_bug_fix_and_patch_revision_are_verified` |
| False completion claim | `fault_injection::tests::false_completion_restart_outcome_unknown_context_and_prompt_injection_are_handled` |
| Restart and checkpoint restoration | `fault_injection::tests::false_completion_restart_outcome_unknown_context_and_prompt_injection_are_handled` |
| Duplicate mutation | `fault_injection::tests::false_completion_restart_outcome_unknown_context_and_prompt_injection_are_handled` |
| `OUTCOME_UNKNOWN` mutation replay | `fault_injection::tests::false_completion_restart_outcome_unknown_context_and_prompt_injection_are_handled` |
| Context overflow and compaction | `fault_injection::tests::false_completion_restart_outcome_unknown_context_and_prompt_injection_are_handled` |
| Prompt injection | `fault_injection::tests::false_completion_restart_outcome_unknown_context_and_prompt_injection_are_handled` |
| Simulated provider switch | `fault_injection::tests::provider_switch_disclosure_long_job_cancel_git_staging_and_no_push_merge_are_guarded` |
| Remote-disclosure policy block | `fault_injection::tests::provider_switch_disclosure_long_job_cancel_git_staging_and_no_push_merge_are_guarded` |
| Long job cancellation | `fault_injection::tests::provider_switch_disclosure_long_job_cancel_git_staging_and_no_push_merge_are_guarded` |
| Unrelated staged Git content | `fault_injection::tests::provider_switch_disclosure_long_job_cancel_git_staging_and_no_push_merge_are_guarded` |
| No push or merge | `fault_injection::tests::provider_switch_disclosure_long_job_cancel_git_staging_and_no_push_merge_are_guarded` |
| Secrets redacted | `fault_injection::tests::false_completion_restart_outcome_unknown_context_and_prompt_injection_are_handled` and `fault_injection::tests::provider_switch_disclosure_long_job_cancel_git_staging_and_no_push_merge_are_guarded` |

## Notes

- The Qwen scenario is represented as a Qwen-style deterministic repair flow.
  It validates the CatDesk-side contract that matters for this sprint: bounded
  model output becomes a patch proposal, the bad patch fails verification, the
  revised multi-file patch is applied by CatDesk, and CatDesk verifies the final
  claim against the real diff.
- Browser adapter testing remains deferred.
- No push, merge, PR, release, deploy, publish, persistent configuration, or
  provider credential flow is part of this test ticket.
