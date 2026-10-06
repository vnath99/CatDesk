# T-0415 Stable Publication + Wake Receipt Review

Scope: current T-0415 source for stable GitHub publication authority and post-submit Chrome-error Wake receipt recovery.

Publication invariants:
- CURRENT_GITHUB_PUBLICATION_AUTHORITY.json remains UNBOUND until a fresh final current-source recovery snapshot is independently reviewed.
- Production authorization selects a safe descriptor path plus exact SHA-256 through the stable pointer rather than a numbered R12 hardcode.
- The reconstruction manifest rejects the stable pointer from its own hash graph.
- The executor rejects overlap among manifest entries, manifest path, and supplemental reviewed paths before Git access.
- Legacy R12 descriptor and journal compatibility remains.

Wake invariants:
- manual-wake-mcp-1791078438284 reached Chat44 but a transient chrome-error page interrupted post-submit receipt finalization.
- Current source may reopen only the already-authorized canonical conversation to continue receipt proof.
- Recovery never types the USER message again or clicks Send again.
- Non-Chrome target drift remains fail-closed.
- manual-wake-mcp-1791080752815 later completed with EXACT_USER_MESSAGE_APPENDED, queue depth zero, and browser closed after success; it remains manual diagnostic evidence.

Verification already observed green for focused publication regressions, installed-runtime Wake Python harness, root binary tests, full workspace/all-target/all-feature tests, strict Clippy, and git diff --check. Standalone fmt is not separately claimed because its direct invocation was blocked by the command safety surface.

Next: independent final review -> reviewed build/promotion -> fresh final manifest/descriptor/review + bound stable pointer -> final independent review -> publication PREPARE/CONFIRM/RESULT -> exact feature-branch non-force push -> remote HEAD equals local HEAD.
