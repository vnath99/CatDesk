# T-0412R3 — V5 MSVC / Windows SDK Environment Repair

## Status

IN PROGRESS — direct ChatGPT work under session `adc-t0412r3-v5-msvc-sdk-env-repair-20260926`, contract `fnv1a64:2d1876d29e0cbca1`.

No acceptance, reviewed build attestation, promotion, reload, or recovery success is claimed by this checkpoint.

## Triggering evidence

Accepted T-0412R2 removed the redundant active-generation `builds/<same-attempt>` source/target nesting and completed `COMPLETED_VERIFIED`.

A fresh protected V5 attempt was then prepared and confirmed from the exact accepted R2 independent-final-review authority. New active attempt:

`28b63d4e107e48a9ae7fd32719e7cd04`

It terminated:

- state: `BUILD_FAILED_OR_AMBIGUOUS`
- failureCode: `REVIEWED_BUILD_FAILED`
- phase: `CARGO_BUILD`
- exitCode: `101`
- classification: `CARGO_LINK_LIBRARY_NOT_FOUND`
- attestation: none

This is a different persisted classification from the pre-R2 generic `CARGO_LINK_FAILED` output-path failure. The current classifier maps this class to the fixed diagnostic `MSVC_OR_WINDOWS_SDK_LIBRARY_ENV_MISSING`. Because raw stderr is deliberately not durable authority, R3 does not claim the exact missing library name.

## Design

The existing V5 worker intentionally uses `.env_clear()`. Before R3 it restored Cargo/Rust/system-drive/temp values but deliberately omitted MSVC/Windows SDK library/include variables. R3 keeps the inherited environment closed and derives only product-owned values from fixed local Windows installation roots.

Current candidate implementation in `src/reviewed_build.rs`:

- introduces `ReviewedWindowsToolchainEnvironment`;
- derives the system drive from the existing Win32 `GetWindowsDirectoryW` helper, not an environment variable;
- checks fixed Visual Studio 2022 edition roots under Program Files;
- chooses the highest numeric MSVC toolset version deterministically;
- checks the fixed Windows Kits 10 root and chooses the highest numeric SDK Lib version;
- validates every selected directory component as an ordinary directory and rejects symlink/reparse components;
- requires ordinary non-reparse `link.exe`, `libcmt.lib`, `ucrt.lib`, and `kernel32.lib`;
- constructs bounded `PATH`, `LIB`, `LIBPATH`, and `INCLUDE` values with `std::env::join_paths`;
- keeps `.env_clear()` and writes only those derived values into the Cargo child;
- does not read ambient `LIB`, `LIBPATH`, `INCLUDE`, `VCINSTALLDIR`, or `VSINSTALLDIR`;
- updates the fixed environment-policy digest material to include `validated-fixed-root-msvc-sdk-environment`, so a later reviewed build cannot silently reuse the old environment policy.

## Security boundaries preserved

R3 does not take caller-selected Visual Studio, SDK, library, include, linker, or shell paths. It does not call `cmd.exe`, PowerShell, `vcvars*.bat`, or inherit ambient toolchain variables. Reviewed-source authority, protected generation directories, isolated offline Cargo cache, worker job containment, final output-handle acquisition, candidate publication, attestation, promotion authority, Wake target, and external Secure MCP ownership are unchanged.

## Verification state

Not yet verified.

The generic shell wrapper rejected `cargo fmt --check` before Cargo launched, so that is not a formatting/code failure. The full autonomy finalizer was intentionally not started near the active 20-minute Wake turn deadline.

Focused regression now present: `reviewed_toolchain_version_selection_is_numeric_and_requires_complete_layout` proves numeric version ordering (`14.10.2` over `14.9.1`) and refuses a higher incomplete/non-numeric layout from selection. It has not yet been executed.

Required next:
1. run approved formatting;
2. run the focused resolver/policy regressions;
3. run full Cargo tests;
4. run strict Clippy;
5. run git diff check;
6. repair only concrete R3 verification failures;
7. finalize R3 through independent review and require `COMPLETED_VERIFIED`;
8. only then create fresh reviewed-source/build authority and a new protected V5 attempt.

