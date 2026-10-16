# T-0485 R30 — Native no-replace publication fixture proof (source only)

## Change

Windows fixture tests exercise a handle-relative `NtSetInformationFile` no-replace rename from the exact STAGED candidate to the signed package slot. The fixture writes and flushes the R29 publication-attempt marker first, opens the source and both namespace parents through retained handles, checks their physical IDs, performs one rename, and flushes both parents. It then uses the protected read-only publication inspector and retained moved-package pin to check exact bytes, ACLs and object identities.

This native call exists only inside tests. The current STAGED receipt holds read pins that deny DELETE access; the fixture drops those pins after the durable attempt marker and independently reopens the exact candidate. A production typed writer receipt and safe handoff across that gap are still required. There is no production rename, PUBLISHED record, pointer switch or install authority.

## Verification

- Four focused native fixture tests passed: exact successful move and both parent flushes; destination collision without replacement; each injected parent-flush failure with sticky restart attention; and actual post-move DACL drift refusing publication readback. The successful case also checks that retained read pins prevent moving the package, and that a same-byte replacement after pin release fails the physical-ID check.
- Full Wake all-targets/all-features suite passed: 213 library tests passed, one ignored; other enabled targets passed. Strict Wake Clippy, format check and diff check passed. Independent read-only review found no blocker in the two-file test-only diff.

Source commit `ab67d390dee529e604904daac604ca1e161e34a5` was pushed. Immediate exact-SHA GitHub readback found zero workflow runs and zero check-runs. No remote CI acceptance is claimed.

## Remaining gate

Implement and independently verify the production stage-pin-to-writer handoff, native no-replace rename, dual-parent durability, retained post-rename proof and typed PUBLISHED receipt under the signed fixed broker. The dev.84 connected owner/store anchor, exact three-job CI, protected pointer/owner/reader cutover, approved dev.85 activation and fresh Chat53 receipt remain open. Neither installer guard nor fixed-service refusal changed.
