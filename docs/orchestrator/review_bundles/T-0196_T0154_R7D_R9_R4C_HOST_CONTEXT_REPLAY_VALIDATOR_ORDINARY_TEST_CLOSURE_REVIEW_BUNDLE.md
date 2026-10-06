# T-0196 — host-context replay validator ordinary-test closure

## Contract-authoritative host evidence

The independent CatDesk host probe established that the T-0195 Rustup resolver
passes in its real host context. It also established that the prior replay
probe reached a successful untouched `validate_producer_attestation`, performed
the candidate-replay-pre-open replacement, and received exact
`REVIEWED_BUILD_ATTESTATION_UNAVAILABLE`; its only failure was the erroneous
claim that 19-byte `attacker--candidate` matched the 20-byte
`legitimate-candidate` fixture.

## Test correction

`replay_preopen_producer_attestation_rejects_same_length_swap` is now an
ordinary Windows test. The legitimate payload remains exactly 20 bytes. The
attacker is now the different 20-byte payload `attacker---candidate`.

The hook records `substitution_performed` only after the real rename of
`catdesk.exe` to `catdesk.legitimate` succeeds and the same-name attacker file
write succeeds. The negative path requires that flag, candidate-evidence hook
reachability, exact `REVIEWED_BUILD_ATTESTATION_UNAVAILABLE`, reopened
handle-derived equal length/different SHA/different stable identity, and an
unchanged independent outside sentinel. A Windows denial of the replacement is
therefore a test failure here, not substitute evidence.

The untouched positive validator call remains before installing the hook. The
host-only Rustup probe remains ignored exactly as requested; only the replay
validator test lost its ignore marker.

## Provider-environment result

The ordinary focused command was run without `--ignored`. It discovered and
executed this replay test, but this provider's sandbox token has no
profile-local Rustup installation, so it failed before the fixture's positive
validator call with `REVIEWED_BUILD_STATE_UNAVAILABLE`. This does not alter the
contract-authoritative CatDesk-host probe result and no cross-profile/PATH
fallback was introduced. Full provider `cargo test` is consequently not
claimed.

## Changed files

- `src/reviewed_build.rs`
- `docs/orchestrator/review_bundles/T-0196_T0154_R7D_R9_R4C_HOST_CONTEXT_REPLAY_VALIDATOR_ORDINARY_TEST_CLOSURE_REVIEW_BUNDLE.md`
