# T-0434 — Current source serving authority review

## Scope

This review freezes the current CatDesk source as the candidate authority for the
next protected reviewed build after Wake dev.69 live acceptance. It does not
install, reload, promote, recover, resume T-0425, mutate Wake, change the
canonical target, or touch the externally owned Secure MCP runtime.

The current critical path is:

1. fresh current-source review authority;
2. protected V5 BUILD_ATTESTED;
3. reviewed promotion / serving parity;
4. durable reviewed_promotion LKG and supported recovery proof;
5. resume the SAME paused T-0425 only after the serving boundary is proven.

## Accepted predecessor evidence

- T-0429 direct ChatGPT pause/resume continuity is independently reviewed. The
  accepted discriminator applies only when a PAUSED session is already
  direct-ChatGPT-owned: provider route WAITING_FOR_CHATGPT, provider turn count
  greater than zero, current task present, no provider thread/handle, and the
  exact current queue task still WORKER_RUNNING. That path restores
  WAITING_FOR_CHATGPT without provider restart reconciliation. Inconsistent
  queue evidence fails closed and provider-owned work retains restart
  reconciliation.
- T-0432/T-0433 Wake resilience is accepted under immutable dev.69. The live
  recoverable review-source fault preserved the same dev.69 process/singleton
  and incremented recovery telemetry. The fresh natural post-recovery canary
  then completed generation-21 EXACT_USER_MESSAGE_APPENDED -> SENT, Wake timer
  COMPLETE at 102 seconds, browser CLOSED_AFTER_SUCCESS, queueDepth 0, with no
  USER replay.
- Historical generation-20 R6 remains immutable SUBMITTING/no-receipt forensic
  evidence and is not reused as acceptance authority.

## Current-source additions requiring inclusion in the fresh authority

Current source also contains the narrow Wake-delivery inspection work added
while resolving R6:

- catdesk_wake_delivery_status is a bounded read-only MCP projection using
  the validated Wake Store delivery API for one exact event ID.
- existing catdesk_turn_timer STATUS exposes the same bounded delivery
  projection only for Wake-event timer inspection, allowing already-open
  connector catalogs to inspect delivery phase/receipt evidence without raw
  filesystem access.
- manual timer START/STOP semantics remain separate; STOP cannot complete a
  Wake-event timer.
- no caller-selected Wake root, path, target, browser profile, credentials,
  receipt mutation, or arbitrary Store content is exposed.

The source-current controller serving this chat has already compiled these
surfaces and the local MCP self-check reports 91 tools. This temporary serving
fact is not itself reviewed-promotion/LKG authority.

## Reviewed-build boundary

The protected reviewed-build V5 policy remains the authority for producing the
candidate used for promotion. A prior T-0429 review record cannot be reused for
a new protected build after later source changes because fresh review/source
binding is required. This T-0434 review is intended to provide that fresh
binding only if its own verifier is green.

No protected build retry is authorized by this document unless CatDesk
finalizes this exact session as COMPLETED_VERIFIED with:

- APPROVED_PROJECT_TESTS passing;
- strict CARGO_CLIPPY passing;
- authoritative GIT_DIFF captured;
- independent final review available;
- exact task output attribution intact.

## T-0425 hold

adc-t0425-same-tab-post-submit-receipt-20260926 remains PAUSED stateVersion 7.
Its execution queue still contains the exact
T-0425-SAME-TAB-POST-SUBMIT-RECEIPT task as WORKER_RUNNING with provider turn
count 1 and no provider thread captured. Do not resume it during this review or
during protected build/promotion. The first live resume is reserved for
post-promotion serving-parity acceptance of T-0429.

## Conditional verdict

CURRENT_SOURCE_ELIGIBLE_FOR_PROTECTED_V5_BUILD only if CatDesk finalization
for adc-t0434-current-source-serving-authority-20260928 returns
COMPLETED_VERIFIED and the required verifier profiles above pass. Otherwise this
document confers no build or promotion authority.
