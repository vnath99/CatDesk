# T-0467 R1 — Signed main-image rotation readiness

**Decision: BLOCKED — current signed-main-image rotation readiness is not
proven.** The block is missing current protected-host provenance/readback, not
a demonstrated source defect. This is a source and historical-authority audit
only; it grants no installation, rotation, reload, release, or daemon action.

## Source identity and scope

- Reviewed source HEAD: `b3c8c5802f1e1031e544fe6397ff55ef2559f92c`
  (`Document serving reload compatibility and signing gate`).
- At audit start, both staged and unstaged tracked diffs were empty. Historic
  untracked material was preserved and is not attributed to this task.
- T-0466 records that the old serving controller rejects both the current
  `catdesk_daemon_reload RESULT` shape and the exact typed PREFLIGHT tuple with
  `INVALID_ARGUMENT`. That is compatible with a stale handler/schema mismatch,
  but does not prove the candidate approval invalid or establish a replacement
  route.
- T-0464's isolated bootstrap executable and T-0465's typed reload-approval
  JSON are intentionally not signed reviewed-main-image authority. They do
  not bind a product-root envelope, accepted epoch, or installed Program Files
  object.

## Fixed product authentication observed in source

`src/reviewed_build.rs` retains a single compiled public trust root with fixed
root id `catdesk-main-image-root` and fixed root version `1`. T-0215 documents
that CatDesk production code has only the Ed25519 verifying side; the signing
private key is external to CatDesk, MCP, this repository, and ChatGPT.

The parser accepts only a bounded canonical envelope: fixed line count/order,
LF termination, product, purpose, root id/version, positive epoch, policy
digest, payload SHA-256/length, review/build identities, and detached
signature. Verification requires the exact purpose/policy/root/version and a
strict signature over all signed envelope fields. Payload consumption then
compares SHA-256 and length against an already-opened regular file.

The fixed transport/receipt paths and no-follow regular-file checks are
compiled policy, not caller input. The accepted-envelope reader rejects a
missing/unsafe/non-regular receipt or an envelope that fails verification. The
worker-only digest bridge rereads both the accepted envelope and fixed
installed image before returning a bounded digest; it is not an MCP endpoint
or a general path/status reader.

## Epoch and rotation policy

For first-image bootstrap, an incoming epoch greater than the accepted epoch
is fresh; an exact same canonical envelope is only accepted as recovery; a
lower epoch or differing same-epoch envelope is refused. The distinct T-0217
rotation purpose/policy similarly requires an incoming epoch strictly greater
than the predecessor, exact pending-envelope agreement, and byte/length/hash
binding to either the predecessor or incoming installed object. Its executor
uses fixed locations and an administrator-only zero-argument boundary; it
does not accept a caller-selected path, hash, epoch, policy, or signer.

Historical records matter but do not establish current state:

- T-0215 describes the original product-root bootstrap mechanism.
- T-0217 records an historical epoch-1 installation and an unsigned epoch-2
  rotation signing request. An unsigned request is not an accepted rotation.
- T-0301 classified the old-serving-controller mismatch
  `OPERATOR_BOOTSTRAP_REQUIRED` rather than permitting raw workspace reload.
- T-0307 correctly treated the then-current accepted-envelope/image state as
  `UNOBSERVABLE_FROM_CURRENT_SAFE_SURFACE`; it did not prove the state absent.
- T-0324's per-user immutable release work is a useful future source
  foundation, but historical source-only/per-user proposals do not establish a
  trusted one-time migration from this old installed controller.

## Missing current readback — the decisive blocker

This task did not and cannot safely read Program Files/ProgramData main-image
payloads, accepted/pending/installed rotation receipts, or the live old
daemon's internal handler implementation. There is no current approved,
exposed read-only main-image status surface that would return the accepted
envelope epoch, signature/provenance result, and installed-image
SHA-256/length binding. Consequently all of the following remain unknown:

1. whether the historical epoch-1 receipt is still the accepted state;
2. whether a later signed rotation already exists or is pending/interrupted;
3. the exact current accepted epoch and installed image identity; and
4. whether a future source-current image/envelope can be consumed without
   reconciliation of an existing pending/installed rotation receipt.

It would be unsafe to replay the historical unsigned epoch-2 request, choose
an epoch, infer acceptance from source/CI, or sign/release a new image before
an authoritative current-state readback resolves those facts.

## Safe prerequisite sequence and authority separation

**Operator / externally authorized reviewed-release boundary:** obtain a
non-mutating trusted readback of the fixed accepted/rotation receipts and
installed image; establish the actual accepted epoch and exact signed payload
binding; decide any required strictly newer epoch; produce a source-current
release payload and canonical signed envelope using the externally controlled
private key; and obtain separate review of that exact artifact chain. Any
administrator-only fixed-policy consumption action needs its own explicit
authorization and post-action readback/rollback review.

**CatDesk/agent boundary:** do not access or request a private key, make a
signature/envelope/payload, select an epoch, copy/launch an executable, invoke
the bootstrap/rotation flags, use the stale raw reload API, or alter Program
Files/ProgramData, daemon, Wake, tunnel, or Git state. After an approved
non-mutating host projection exists, a separate review may validate its bounded
evidence against the fixed source policy before any installation decision.

## Conclusion

The correct next state is **`OPERATOR_APPROVED_SIGNED_BOOTSTRAP_REQUIRED`**,
pending positive current accepted-epoch and installed-image readback. T-0464
and T-0465 remain useful bootstrap/reload evidence only after a source-current
controller is safely established; they are not substitutes for product-root
signed main-image authority. No deployment action is approved by this report.
