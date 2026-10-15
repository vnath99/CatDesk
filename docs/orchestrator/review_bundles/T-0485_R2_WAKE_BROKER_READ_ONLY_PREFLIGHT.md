# T-0485 R2 — Fixed Wake broker read-only preflight

**Scope:** `wake/src/install_broker_preflight.rs`, `wake/src/bin/CatDeskWakeBrokerPreflight.rs`, and Wake library export. This is a diagnostic snapshot of a future separately provisioned service, not the service itself. Existing `WAKE_INSTALL_BROKER_UNPROVISIONED` production holds remain.

## Source behavior

The zero-argument diagnostic uses compiled fixed service name, command line, image location and `C:\ProgramData\CatDesk\WakeProtectedInstallV1` root. It does not take a caller path or write to SCM/ProgramData. It queries the SCM service type, start policy, LocalSystem account, restricted service SID and exact service descriptor. It opens every fixed ProgramData and Program Files ancestor by pinned relative Windows handles without reparse traversal, records physical object IDs, and checks owners/non-null DACLs. At the protected root, image parent and image file it requires exact fixed owner/DACL policies. The image is opened without write or delete sharing, hashed within a size bound, and its identity and security are checked again after reading. Successful output explicitly says `READ_ONLY_IDENTITY_SNAPSHOT_NOT_INSTALL_AUTHORITY`.

On this host, a real read-only invocation exited **1** with `{"error":"WAKE_BROKER_SERVICE_UNPROVISIONED"}`; the service is absent. An extra path argument exited **2** as `CLI_ARGUMENTS_INVALID`. Neither probe altered SCM, ProgramData, installed WakeHost or the live target.

## Verification

- Focused Windows policy tests: **3 passed**, covering candidate service path/account/SID/start/security rejection, distinct protected DACL policy versus broad writer, and component path-escape rejection.
- Final full independent Wake all-features suite: **exit 0**, 74 library tests plus bin/integration groups; two manual tests ignored.
- Final full root all-features suite: **exit 0**, main 1,169 passed/26 ignored plus integration groups. Strict root/Wake all-target/all-feature Clippy, both format checks, root all-bin build, Python `pytest -q --tb=short` and diff check also exited 0 for the combined source.
- Independent read-only review found and confirmed fixes for SID-string allocation bounds, aligned SCM/security buffers and image-file ACL. It found no remaining concrete pointer or fixed-path bypass in this diagnostic scope.

## Authority still missing

The snapshot does **not** prove Authenticode signature, integrity labels, ancestor effective access, worker and descendant denial, inherited writable handles, service runtime/client-token checks, protected trust-root provisioning, anti-replay ledger, pinned candidate staging, owner-pinned handoff, protected runtime executable readers or approved release. A source diagnostic cannot provision its own broker. Exact source-commit three-job GitHub Windows CI and operator-controlled signed bootstrap are outstanding. No dev.85 installation or fresh Wake acceptance follows from this result.

## Later R8 client-read policy adjustment

The dormant connected client must perform this preflight before sending approval bytes. The exact SCM service descriptor therefore now grants IU query-config/status and `READ_CONTROL` (`0x00020005`), without service mutation rights. The fixed Program Files broker image parent and image file now grant IU read/execute (`0x001200A9`), without write/delete/DACL/owner rights; SYSTEM retains full access and the restricted service SID retains read/execute. Three focused preflight tests still pass, including rejection of the earlier insufficient IU service ACE, missing IU image read and a broad IU writer. These source expectations do not create or modify any installed object; complete ancestor effective-access and actual worker-token denial remain open.
