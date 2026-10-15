# T-0485 R22 — Read-only completed predecessor projection (source only)

## Change

Under the broker's retained global ledger lock, `inspect_completed_predecessor` now requires a nonempty, contiguous and replay-free ledger history whose latest transaction is `COMPLETE`. It checks cross-generation predecessor package continuity and exact protected policy/root identity. The protected reader retains opened handles for `current.json`, `packages`, the selected package, manifest and four artifacts, then reopens fixed names, rehashes bytes and rechecks their physical identities and ACLs before returning copied diagnostic fields. The installed package, manifest and four artifact object IDs must match the completed transaction's stage record. No lease, observation, INTENT, installation, or launch authority is returned.

The result explicitly reports that the staging parent object, newly switched pointer's physical ID/digest, and first-generation migration anchor are **unverified**. The present ledger schema cannot attest the new pointer object at `PointerSwitched`/`Complete`; this projection must not be promoted into transaction authority until that record binding is added and checked. The production SCM service still unconditionally refuses before INTENT, and both normal install guards remain closed.

## Verification

- Full independent Wake suite: 194 library tests passed, one ignored; binaries and integrations passed, with manual live tests ignored. Focused broker suite: 129 passed, one ignored. Strict Wake all-targets/all-features Clippy, formatting and diff checks passed.
- An integrated Windows temp-directory fixture calls the same projection core with test-only policy/reader ACL fixtures. It exercises a complete durable eight-record history, pinned protected package, same-length artifact tampering, unchanged ledger bytes and absence of a new INTENT. Other tests cover empty/incomplete history, package/generation drift, wrong installed object IDs, and retained-handle revalidation. The fixture's staging parent is intentionally different, proving the result reports it unverified.
- The isolated root shell/cwd test and remaining all-features workspace Rust suite passed locally, including nested Wake-local tests. Strict root all-targets/all-features Clippy, formatting and all-bin build passed. Independent source review found no concrete blocker after the diagnostic fields were narrowed.

Source commit `ee5e9e39e644966a9cef211d8b5af1038040c888` was pushed. Immediate read-only GitHub readback showed zero workflow runs and zero check-runs for that SHA. No remote CI acceptance is claimed.

## Remaining gate

The signed/provisioned LocalSystem broker, protected policy/namespace, trustworthy loaded-image and independent-time evidence, connected dev.84 owner/store reconciliation, signed exact approval, pointer-switch physical binding, package/pointer/owner/reader transaction, exact-current-source three-job Windows CI and fresh Chat53 receipt remain open. The reader fixture cannot substitute for a real protected installation. The prior HTTP 422 dispatch text and absent `workflow_dispatch` trigger are distinct; no new dispatch or Actions setting change was attempted.
