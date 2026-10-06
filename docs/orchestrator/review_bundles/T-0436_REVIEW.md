# T-0436 — Current-host toolchain / serving parity independent review

## Scope

This bounded direct review evaluates the current-source T-0436 evidence only. It does not authorize a protected reviewed-build retry by itself and does not mutate protected reviewed-build state.

## Evidence reviewed

- Fresh protected V5 attempt `ca0ebdbb6ec64b408782cad3bb50e0e9` terminated with `REVIEWED_BUILD_TOOLCHAIN_UNAVAILABLE`, no attestation and no Cargo diagnostic.
- Preserved historical protected linker evidence placed the installed linker under `C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Tools\MSVC\14.50.35717\...`.
- Current `reviewed_windows_toolchain_environment` source contains a closed product-derived VS18 / Program Files (x86) compatibility layout; it does not introduce caller-selected roots, ambient Visual Studio variables, or `vcvars*.bat` authority.
- Focused regression `reviewed_msvc_install_roots_are_closed_and_include_vs18_x86_compatibility`: PASS.
- Ignored read-only live-host probe `reviewed_windows_toolchain_current_host_discovery_probe_is_manual_only`: PASS against this host.
- `cargo clippy --bin catdesk -- -D warnings`: PASS.
- `cargo test --bin catdesk`: PASS.
- Source-current `cargo build --release --bin catdesk`: PASS.

## Review conclusion

Current source can deterministically discover and validate the actual host toolchain while retaining the closed fixed-root policy. The failed protected attempt is therefore consistent with serving/source parity: it was executed by a serving image that predates the current compatibility resolver. The next safe discriminator is a reviewed reload to the exact source-current candidate followed by one fresh protected V5 PREPARE/CONFIRM. Success must terminate `BUILD_ATTESTED`; any fixed-vocabulary failure must stop further retries for diagnosis.

No protected build retry, promotion, LKG mutation, Wake mutation, T-0425 resume, or Git publication is part of this review.
