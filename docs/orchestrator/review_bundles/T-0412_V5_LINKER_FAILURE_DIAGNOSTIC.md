# T-0412 V5 linker-failure diagnostic

## Classification

`V5_LINKER_FAILURE_REQUIRES_BOUNDED_HOST_DIAGNOSTIC`

## Immutable failure evidence

Generation `465286253071480b8fb9afa95c296d26` remains unchanged. Its terminal
result is `BUILD_FAILED_OR_AMBIGUOUS` / `REVIEWED_BUILD_FAILED`, with phase
`CARGO_BUILD`, exit code `101`, and fixed-vocabulary classification
`CARGO_LINK_FAILED`. Its retained stderr metadata is a 4,096-byte truncated
capture and SHA-256 `2c2c562fb8024c212a03884efb8bdbfee3452a2dab1aa29b9b5aa8c3a0f1ba96`;
raw stderr was deliberately not persisted.

## Current worker and authority seams

The V5 worker invokes only the attested Cargo executable with fixed
`build --release --locked --offline`, a pinned isolated target, attested Rustc,
isolated Cargo home, toolchain-directory-only `PATH`, OS-derived `SystemDrive`,
and per-attempt `TEMP`/`TMP`, after `env_clear()`. It intentionally omits
`LIB`, `LIBPATH`, `INCLUDE`, `VCINSTALLDIR`, and `VSINSTALLDIR`.

The existing V4 host evidence established that the former scrubbed environment
without `SystemDrive` produced `linker 'link.exe' not found`; the current worker
now derives and sets `SystemDrive` through `GetWindowsDirectoryW`, not through
an inherited environment value. Therefore ordinary executable discovery is less
likely than it was under that former baseline. It is not, however, proven by
the new terminal record.

`CARGO_LINK_FAILED` is deliberately broad: the classifier requires a linker
invocation shape plus failure/exit indication. It does not retain the invoked
linker path, Windows linker code, missing library name, SDK version, or raw
error text. Consequently the durable evidence cannot distinguish a residual
`link.exe` discovery failure from missing MSVC/Windows SDK library environment
or another linker exit with certainty.

The job object contains Cargo and descendants and kills them on closure, but it
does not provide linker selection or library authority. The final-link/output
gate occurs only after Cargo succeeds; it preserves pinned-target, opened-file,
remeasurement, create-new candidate, and later attestation boundaries and is not
on the failed path. Existing dedicated-producer/handle handoff seams are
separate, unprovisioned/test feasibility work and must not be repurposed as a
linker discovery mechanism.

## One bounded next diagnostic

Before any production repair, run one host-only, disposable diagnostic using
the exact attested Cargo/Rustc, fixed V5 source snapshot and isolated cache, and
the same env-cleared worker environment. Drain stderr in memory, emit and retain
only one fixed-vocabulary outcome plus digest/length/truncation metadata:

- `LINKER_EXECUTABLE_NOT_FOUND` for the exact linker-not-found signature;
- `MSVC_OR_WINDOWS_SDK_LIBRARY_ENV_MISSING` for fixed missing-library signatures;
- `OTHER_LINKER_EXIT` otherwise.

It must persist no raw stderr, environment values, paths, URLs, credentials, or
caller-selected inputs, must use a disposable non-authority target, and must
not publish a candidate or alter the failed generation. This single diagnostic
would distinguish the two proposed repair classes without adding PATH/shell,
registry-wide caller selection, inherited developer-shell variables, or network
fallback.

## Recommendation

No production repair is justified from the present record alone. If the bounded
diagnostic reports missing MSVC/SDK libraries, the smallest future repair should
be a new policy-bound, OS-derived and validated fixed library-environment
closure, never inherited developer-shell values. If it reports executable-not-
found, any repair must instead select and attest an exact fixed linker path
without PATH search. Both would require separate authorization and a current
policy identity; neither is performed here.

## Scope

No product-source, protected reviewed-build state, live Wake, tunnel, release,
LKG, daemon, credentials, Git history, or external-project mutation occurred.

