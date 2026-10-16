# T-0485 R42 — Offline V2 full file identity and retained role pins

Date: 2026-10-16. Source baseline `6d602bdef28f374588ff65e24efc2d440d243aa4`; isolated source commit `1a2f12845c051608d15e2e332400e791f8269257`. Status: source-only, read-only; independent source review passed and the change is integrated in main as fbe8fb5 and 15427a8. This is not a first-generation migration admission, dev.85 install, or natural Wake receipt.

The signed V2 anchor and package approval now bind each object role to both the existing legacy identity and `FILE_ID_INFO`: a 64-bit volume serial and 128-bit file ID. The canonical signed JSON uses lowercase hex for the 128-bit ID. Missing full identities, invalid IDs, duplicate role IDs, changed observations, or a changed anchor/package link refuse verification. V1 wire and ledger semantics are unchanged.

The policy-facing V2 inspection now requires eight retained Windows handles. The pin opener denies new write/delete sharing, rejects a final-component reparse point and wrong file/directory type, reads both identities from each handle, and revalidates all roles before and after signature inspection. The fixed protected root and service image full IDs are also read from the fixed preflight's validated handles; the fixed policy root and policy file full IDs are read from the policy's retained handles. Those fixed-object IDs must match the signed observations and caller-supplied role pins. The preflight's existing V1 serialized readback shape is unchanged.

## Local verification on the isolated source

- `cargo test --locked --offline --manifest-path wake/Cargo.toml --all-features install_broker_offline -- --test-threads=1`: 10 focused tests passed, including real Windows file and directory pins, full-ID drift, eight-role comparison and new-writer denial.
- `cargo test --locked --offline --manifest-path wake/Cargo.toml --all-features offline_full_ids_come_from_the_fixed_policy_handles -- --test-threads=1`: passed with actual retained policy fixture handles.
- `cargo test --locked --offline --manifest-path wake/Cargo.toml --workspace --all-features --quiet -- --test-threads=1` under normal Windows filesystem authority: exit 0, 228 library tests passed, one ignored; all enabled targets passed. A prior restricted-sandbox run passed 227 library tests but failed the existing PowerShell store-preflight integration fixture with `WAKE_STORE_AUTHORITY_OBJECT_UNAVAILABLE`; the normal-authority exact-source rerun passed it.
- Strict all-target Wake Clippy, Wake formatting, all-bin build, and staged `git diff --check`: exit 0.

## Remaining release gates

The old-store paths are selected by a caller. These handles do not prove that the historical connected root was selected or that every token-specific store and launch entrypoint was inventoried and quarantined. Ancestor path pins, exclusive old byte-range lock ownership, process and image provenance, event disposition, paired CatDesk target readback, independent first-generation approval, protected provisioner, exact GitHub CI, transaction ledger, pointer/reader cutover and fresh Chat53 receipt remain open. An installed broker/service and first-generation authority are absent. The V2 result still ends in `NO_INSTALL_AUTHORITY` and has no ledger or install consumer.
