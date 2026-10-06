# T-0197 — final-link output-object handoff feasibility

## Preserved evidence

T-0193's `first_authority_same_length_regular_swap_exposes_unbound_cargo_output`
remains unchanged. It fires only after the release directory is pinned and
before the first child open; it proves a same-name 20-byte regular-file swap
becomes both built-output and candidate evidence. The conditional reparse test
also remains unchanged. Neither test is moved behind an earlier baseline open.

## Current trusted chain

`trusted_toolchain` binds only the selected final `cargo.exe` and `rustc.exe`
with canonical path, non-reparse opened-file identity, SHA-256, bounded length,
and version digest. T-0195 first prefers the exact Program Files pair and,
only when both are absent, uses the OS-token profile's fixed Rustup resolver to
construct concrete tools under one profile-local `.rustup\toolchains` root.
`run_reviewed_build_worker` pins those two tools, starts Cargo suspended,
assigns Cargo to the kill-on-close Job, resumes it, waits for Cargo to exit,
then revalidates the same tool evidence before calling `open_built_output`.

That chain is intentionally not final-link authority. Cargo asks Rustc to link;
the final linker may be compiler-internal `rust-lld`, target-default
`link.exe`, or an explicitly configured linker. Current reviewed-build state
does not contain a trusted linker selection record, linker PID, or linker
output handle. Cargo's successful exit and the Job's descendant inclusion show
only process-tree containment, not which descendant created a later pathname
entry.

## Windows object-lifecycle conclusion

An ordinary linker creates/truncates/writes/closes its own output pathname.
The parent does not create an inherited final-output handle for it, and an
unmodified linker does not return its private handle over a CatDesk-authenticated
channel. Once that handle closes, the post-Cargo RootDirectory/no-follow open
can bind a safe *current* regular object but cannot prove producer provenance;
the preserved T-0193 test demonstrates exactly this same-name replacement.

Duplicating a descendant's output handle is not a safe substitute: CatDesk has
neither an authenticated linker PID nor a handle value/capability to duplicate.
Enumerating process handles would be global, racy, privilege-sensitive, and
would not establish that the candidate handle was the final link output. It is
therefore not a reviewed authority primitive. Allowing FILE_SHARE_WRITE or
FILE_SHARE_DELETE would additionally leave same-object mutation/replacement
unclosed; closing those shares only after an untrusted pathname lookup moves
the race rather than binds origin.

## Broker feasibility decision

A broker could only be authoritative if the reviewed policy chose and attested
one concrete linker, forced Rustc to invoke that broker as the linker, supplied
an unspoofable per-attempt inherited-handle/nonce protocol, and made the broker
return a restrictive handle for the same file object before the final producer
released it. The current policy has none of these links. Adding them would
change compiler/linker selection, Rustc invocation policy, process identity,
IPC, and output ownership simultaneously; a wrapper cannot merely observe an
unknown Cargo/Rustc descendant after the fact. It would also require proving
that build-script and target configuration cannot bypass the wrapper.

No narrowly scoped secure implementation exists from the current Cargo/Job/
Windows primitives. T-0193 therefore remains blocked. The next acceptable
design boundary is a distinct, attested build-execution protocol with a
policy-pinned linker/broker as the only final-link executable and an inherited
non-delete/non-write-share output object handoff, or an isolated build identity
(for example service/AppContainer build root) that prevents same-user mutation
until CatDesk obtains the handle. A pathname, mtime, Cargo log, or post-close
handle discovery is not an acceptable fallback.

## Source/test mapping

| Claim | Source | Deterministic evidence | Result |
| --- | --- | --- | --- |
| Cargo/Rustc identity is continuous | `trusted_toolchain`, `open_attested_tool`, worker post-build revalidation | T-0195 tool/static tests | Preserved |
| Job owns Cargo descendants | `BuildJob::assign` / `resume` | Existing Job process tests | Containment only |
| First output child authority is post-Cargo | `open_built_output` | `built_output_first_authority_seam_precedes_every_child_acquisition` | Proven |
| Same-name regular swap is not producer-bound | `open_built_output` -> candidate copy | `first_authority_same_length_regular_swap_exposes_unbound_cargo_output` | Attacker accepted; blocker |
| Reparse does not become authority | shared R7C relative no-follow open | `first_authority_reparse_child_is_rejected_when_supported` | Rejected when live creation permitted |

## Status

T-0197 is **not complete**. No live reviewed-build worker, promotion, reload,
or external integration was invoked. This is a concrete fail-closed design
escalation, not a claim that continuous final-link identity has been proven.
