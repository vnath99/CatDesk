# T-0286 / T-0224-R1C — post-target-repair natural wake canary

## Scope

This is the one permitted T-0286 mutation: a factual review bundle for the
ordinary autonomous completion event. No product source, test, wake
configuration/state/profile/owner, target, browser, tunnel, scheduler,
service, daemon, release, external project, signing/provenance, or Git state
was changed by this task.

The durable queue records that the guarded operator wake-target compare and
swap completed with this canonical target SHA-256:

```text
d40b8d5b1995bb781f1c8e4f56bc6e6397f649678ad8ce9cb21232fdeba82d98
```

That CAS result establishes the configured target identity for the canary. It
does not itself prove a browser delivery. Live browser delivery remains
unaccepted until the ordinary completion path creates one fresh project-scoped
review record and the normal event-driven owner persists the exact durable
receipt below.

## Natural-event acceptance contract

Acceptance requires all of the following for the one fresh T-0286
project-scoped review record:

| Evidence | Required value |
| --- | --- |
| Event source | Ordinary autonomous `COMPLETED_VERIFIED` / final-review emission only |
| Review records | Exactly one fresh project-scoped record |
| Manual invocation | None; `catdesk_wake_bridge_run_once` must not be invoked |
| Wake delivery schema | Schema 4 |
| Wake delivery status | `SENT` |
| Browser timestamp | Positive `browser_sent_at_unix` |
| Receipt schema | 1 |
| Binding | Exact review-record and message binding for this canary only |
| Target binding | Exact SHA-256 `d40b8d5b1995bb781f1c8e4f56bc6e6397f649678ad8ce9cb21232fdeba82d98` |

`CHATGPT_NOT_IDLE` is only a pre-submit defer/retry condition under the
existing durable retry policy. It is not a successful submission and must not
be converted into a duplicate attempt. Target drift, login, CAPTCHA, security
interstitials, and any post-submit receipt ambiguity must fail closed without a
duplicate submit or a synthetic `SENT` receipt.

## Evidence to inspect after ordinary completion

After this task reaches its normal terminal/final-review path, inspect only the
durable project-scoped review and wake-delivery records for the newly emitted
T-0286 event. Verify the exact record/message/target binding and all values in
the table. A missing, stale, mismatched, ambiguous, non-`SENT`, or zero-time
record is not acceptance; retain the bounded fail-closed classification for
independent review.

No provider, operator, or ChatGPT action in this task launches a browser,
invokes the wake bridge, changes the target, or manufactures wake evidence. The
hourly deadman remains an independent fallback rather than evidence of this
canary's delivery.

## Verification and attribution

The only attributable file is this bundle. Rust verification and the
authoritative diff check were completed after this documentation write:

| Check | Result |
| --- | --- |
| `cargo test --all-targets --all-features --no-fail-fast` | PASS (860 tests; optional Python-advisor tests ignored) |
| `git diff --check` | PASS; only pre-existing broad-worktree CRLF warnings were emitted |

No separate `rust_full` command is configured or exposed in this workspace,
so the all-target/all-feature suite is the available Rust verification. Cargo
also emitted the pre-existing non-fatal `<USER_PROFILE>
warning. No browser-profile contents were read.

## Independent-review request

After ordinary autonomous completion has emitted the fresh record, request
independent review of the exact durable schema-4/receipt-1 evidence. Do not
accept live browser delivery solely from this bundle or from the guarded target
CAS.
