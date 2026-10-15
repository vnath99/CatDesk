# T-0485 R9 — Single-message broker request receive source

## Scope

`wake/src/install_broker_request.rs` adds a dormant Windows receive boundary for the proposed fixed broker endpoint. It takes an already connected `FixedBrokerPipeEndpoint`, reads one bounded message, and calls `BrokerProtectedPolicyV1::inspect_connected_operator` on that **same pipe handle immediately after the read**, before parsing the request. The endpoint and protected policy types prevent a caller-selected pipe name or operator SID from entering the production function. The resulting frame retains the received bytes and token evidence but grants no install authority.

The reader accepts 1–16,384 bytes. It reads into a 16,385-byte buffer so a complete one-byte-oversized message is refused, and it refuses a partial message (`ReadFile` failure, including `ERROR_MORE_DATA`) without continuing with another read. It never treats a client-supplied identity or signature as token admission.

## Focused verification

`cargo test --locked --offline --manifest-path wake/Cargo.toml --all-features install_broker_request::tests -- --nocapture` exited 0 with three Windows pipe fixtures:

- One complete message reaches admission exactly once.
- An oversized message never reaches admission.
- An admission denial discards the received frame.

These tests use unique local pipe names and a test-only admission closure. The production function remains unable to pass on this host without the separately provisioned fixed service, protected namespace, and policy.

## Open production gates

This source does not create a service, connect an endpoint, set a bounded connect/read deadline, parse or verify a signed approval, fetch independent CI, protect a ledger, copy packages, switch a pointer, or start WakeHost. A future service must serialize each request read and impersonation with all other access to that pipe instance, finish acting on the admitted frame before another read, and reject a disconnected/reconnected or stalled client. It must use the protected ledger and exact observed state before any mutation. The production `WAKE_INSTALL_BROKER_UNPROVISIONED` holds remain unchanged.
