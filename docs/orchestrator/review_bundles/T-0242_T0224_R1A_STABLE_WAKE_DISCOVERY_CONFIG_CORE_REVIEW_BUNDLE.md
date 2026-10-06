# T-0242 stable wake discovery/config core

Added `src/stable_wake_bootstrap.rs`, a daemon/MCP/release-independent bounded
reader for `stable-root/review-events` and exact project-target resolver. It
accepts schema v1 project-scoped events only, caps count/size/fields, rejects
symlink/traversal/oversize/malformed input, deterministically deduplicates
record IDs, and does not consume events. Target resolution requires the exact
project, canonical `https://chatgpt.com/c/` shape, and SHA-256 match.

The module has no browser, target mutation, daemon, MCP, tunnel, manifest,
promotion, or LKG dependency. Tests use temporary roots and cover duplicate
event preservation plus target-digest refusal. R1B retains claim/receipt,
browser execution, desktop checks, and installation ownership. No live host
mutation occurred. Full verification remains for independent execution.
