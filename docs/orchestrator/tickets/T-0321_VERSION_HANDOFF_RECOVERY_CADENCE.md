# T-0321 — Version-handoff recovery cadence

## Objective

Make a reviewed CatDesk version handoff easy to resume from ChatGPT Web without
turning the public lifecycle into release, connector, credential, or tunnel
authority.

## Fixed operational contract

1. A new binary arrives only through the existing reviewed promotion handoff.
2. Once that handoff reports `PROMOTED_CANONICAL_READY`, the only normal
   ChatGPT-Web follow-up is `./catdesk.ps1 recover`.
3. The command verifies the canonical local daemon and re-adopts the existing
   official runtime. It does not build, install a candidate, reconnect/create a
   tunnel, or expose tunnel configuration.
4. If bounded transport verification remains unavailable, the autostart
   supervisor performs one public recovery call, reports
   `EXTERNAL_RUNTIME_PENDING`, and returns to ordinary health polling. It never
   converts that external observation into a daemon restart burst.

## Acceptance

- Preserve reviewed-promotion and external-runtime non-ownership boundaries.
- Add deterministic supervisor coverage for the explicit
  `TRANSPORT_VERIFICATION_FAILED` state.
- Pass supervisor, lifecycle, bootstrap, and reviewed-promotion fixtures.
- Run a literal public host `status` followed by `recover`, without manual
  tunnel or process manipulation.

## Out of scope

This ticket does not prove watchdog-only destructive recovery (T-0319), create
or manage an official runtime, modify connector settings, or authorize an
unreviewed binary.
