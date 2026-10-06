# T-0039 R8 wake retry live-canary review bundle

## Scope and candidate identity

This T-0087 worker review made no product, source, browser, profile/storage,
wake durable-state/config, tunnel, Scheduler, daemon, release, or Git change.
It did not invoke SeleniumBase, any Python wake script,
`catdesk_wake_bridge_run_once`, browser tooling, reload/promotion, or Qwen.

The reviewed combined R6+R7 candidate is
`target/t0039-r7-candidate/release/catdesk.exe`. Its locally read SHA-256 is:

`b98b644c8afaa352a38fe692abbe25291c203cd29596a4277fb45500978ddf79`

This exactly matches the candidate identity in the approved T-0087 contract.

## Reviewed R7 readiness acceptance contract

The production bridge owns pre-submit readiness in one browser/CDP/profile
context. It may use at most three independent 30-second readiness windows:
the first attempt, then one same-context reload/activation boundary and a new
30-second window, then a second same-context boundary and a final new
30-second window. There is no outer 3 x 3 retry multiplication and no new
browser context.

Authentication redirect/login control, CAPTCHA/security control, exact-target
drift, browser network error, or receipt ambiguity remains fail-closed. In
particular, auth, CAPTCHA/security, and target drift stop immediately rather
than consuming the remaining readiness budget.

After readiness, W13 retains one submit boundary: exact target, idle state,
one empty actionable editor, and absent expected digest are rechecked before
the durable `SUBMITTING` callback immediately precedes one click/Enter. There
is no retry after that boundary. A successful durable send requires schema-4
state `SENT`, positive `browser_sent_at_unix`, receipt schema 1, exact
record/message/target hashes, and target SHA-256
`acbac6dc7101db8e3dfff84d0d043e497f89cb31964c1e1d7f071eef4e5737d4`.

## Dispatch ownership

This fresh review record is the worker's only output. It performs no live wake
or manual proof. After `COMPLETED_VERIFIED`, final live proof is owned solely
by CatDesk's normal automatic dispatcher, which must submit exactly once for a
fresh record. The prior failed R6 record remains preserved and must never be
retried. CatDesk independently verifies the resulting durable state and exact
receipt.
