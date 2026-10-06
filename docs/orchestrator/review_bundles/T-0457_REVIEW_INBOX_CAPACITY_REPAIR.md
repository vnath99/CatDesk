# T-0457 — Review inbox automatic retention repair

Fresh review authority: `adc-t0457r1-review-inbox-auto-retention-20261005` supersedes the stale intermediate capacity-only checkpoint from `adc-t0457-review-inbox-capacity-20261005`.

## Problem

The live CatDesk autonomous review inbox reached its historical hard cap of 512 records while finalizing R15.

Observed live state before one-time bootstrap recovery:
- total review records: 512
- unread/actionable: 166
- acknowledged: 346
- R15 had already completed its provider turn and passed into post-verification finalization
- finalization failed reproducibly with `review_inbox_unavailable`
- providerTurnCount remained 1, proving retry did not relaunch Codex

The review inbox is not the Wake transport queue. It is a durable local review-notification index. Wake/ChatGPT consumes unread actionable records; acknowledgement only changes `unread=true` to `false`. Historical acknowledged records were intentionally retained as review/audit evidence, but no retention policy existed, so acknowledged history accumulated until the fixed 512-record limit deadlocked new review publication.

A one-time operator bootstrap compaction reduced the currently serving inbox from 512 to 384 records while preserving all 166 unread records and creating a full 512-record backup. That bootstrap is not the permanent design.

## Permanent repair

`src/delegated/autonomy_state.rs` now uses one bounded retention policy shared by both autonomous and delegated review emitters:

- absolute live-inbox limit: `AUTONOMY_REVIEW_INBOX_RECORD_LIMIT = 4096`
- automatic compaction trigger: `3584`
- automatic compaction target: `3072`

Before a new review is appended at or above the high-water trigger, CatDesk removes only the **oldest acknowledged** records until the target is reached or no more acknowledged records are available.

Properties:
- unread/actionable records are never removed by automatic retention;
- recent acknowledged records remain available for downstream review-authority use;
- record ordering of retained records is preserved;
- existing-record/idempotent replay lookup occurs before compaction;
- if the inbox becomes saturated with genuinely unread work and cannot be reduced below the hard limit, CatDesk still fails closed rather than discarding actionable work;
- both autonomous and delegated review emitters use the same policy.

This makes ordinary review/Wake operation self-maintaining; an operator should not need to compact the inbox manually again.

## Why acknowledged records are not deleted immediately

Acknowledged review records can temporarily serve as bounded authorization evidence for a subsequent reviewed action (for example a reviewed promotion). Immediate deletion on claim/ack would therefore be unsafe. The high-water retention policy keeps a large recent acknowledged window while retiring only old acknowledged history.

Authoritative per-session events, verification/completion artifacts, and other durable state remain separate from the live review notification index.

## Regression coverage

1. `review_inbox_accepts_record_beyond_legacy_512_boundary`
   - preloads 512 valid records;
   - appends record 513 through the production emitter;
   - proves the historical 512 boundary no longer deadlocks review publication.

2. `review_inbox_auto_compacts_old_acknowledged_records_and_preserves_unread`
   - preloads the automatic-compaction high-water population;
   - mixes acknowledged and unread records;
   - appends one new review;
   - proves oldest acknowledged records are retired to the target;
   - proves every unread record remains;
   - proves a recent acknowledged record remains;
   - proves the new review is appended unread/actionable.

## Safety boundaries

This source repair does not:
- delete or rewrite the live protected inbox through MCP;
- discard unread records;
- alter R15 artifacts/provider output;
- stage/commit/push Git;
- bind publication authority;
- alter Wake target/profile;
- alter protected-build state.

The manual bootstrap backup remains:
`.catdesk/autonomy/review-inbox-full-20261005-before-capacity-repair.json`

## Next

After independent review, include this source repair in the next reviewed CatDesk build/promotion. Meanwhile, because the one-time bootstrap freed 128 live slots, the existing serving binary can finish T-0457 and then resume the already-passed R15 finalization without another Codex provider turn.
