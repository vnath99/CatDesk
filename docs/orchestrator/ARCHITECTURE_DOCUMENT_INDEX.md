# CatDesk Architecture, Security, and Diagram Index

This index classifies architecture/security documents by their present
authority. It does not change a ticket's acceptance status or authorize host
actions. Status is reconciled from the milestone tracker and current plan.

## CURRENT

The five root documents are the only primary continuity references:

| Artifact | Primary responsibility |
| --- | --- |
| [Project handoff](../../CATDESK_PROJECT_HANDOFF.md) | Rapid resumption and next action |
| [Milestones](../../CATDESK_MILESTONES.md) | Long-term scope and acceptance dashboard |
| [Commands](../../CATDESK_IMPORTANT_COMMANDS.md) | Verified operational/developer syntax |
| [Activity](../../CATDESK_ACTIVITY_LOG.md) | Chronological audited actions |
| [Tickets](../../CATDESK_TICKETS.md) | Compact status/evidence index |

[CANONICAL_CURRENT_ARCHITECTURE.md](CANONICAL_CURRENT_ARCHITECTURE.md) remains the supporting technical design/security authority. Local `.catdesk/current_plan.md`, `.catdesk/session.md` and `.catdesk/todo.md` retain native runtime compatibility and original journals; they are not competing master documentation and may be absent in a clone. Old handoff/commands below their compatibility notices are HISTORICAL. [Inventory/migration](../verification/REPOSITORY_RECONCILIATION_2026_11_14.md).

## SUPERSEDED

| Artifact | Why it is superseded | Permitted use |
| --- | --- | --- |
| [`ARCHITECTURE_DECISION_WORKER_RUNTIME.md`](ARCHITECTURE_DECISION_WORKER_RUNTIME.md) | Earlier worker-runtime decision predates the current provider-reset, stable-control-plane, and durable acceptance reconciliation. | Background rationale only. |
| [`MCP_WAKE_BRIDGE_SEMANTICS.md`](MCP_WAKE_BRIDGE_SEMANTICS.md) | Earlier wake handoff description predates the current owner-selector and canonical review-inbox constraints. | Historical implementation detail only. |
| [`OPERATOR_GATE_T0032_DAEMON_RELAUNCH.md`](OPERATOR_GATE_T0032_DAEMON_RELAUNCH.md) | Earlier daemon-relaunch operator gate predates the stable-supervisor recovery model. | Provenance only; never a current command authority. |

## HISTORICAL / EVIDENCE

| Artifact family | Status | Correct use |
| --- | --- | --- |
| [`review_bundles/`](review_bundles/) | Historical ticket evidence | Use a named bundle to understand its reviewed scope and result; it cannot override a CURRENT source or authorize replay of a host procedure. |
| [`SUPERVISOR_MCP_SURFACE.md`](SUPERVISOR_MCP_SURFACE.md) | Historical/source-scope reference | Compare source surface design only against the current stable-control-plane acceptance map. |
| [`FAULT_INJECTION_SECURITY_TESTS.md`](FAULT_INJECTION_SECURITY_TESTS.md) | Historical test-design reference | Retain for test provenance; do not infer production authority. |
| [`CONTEXT_AND_HANDOFF.md`](CONTEXT_AND_HANDOFF.md) | Historical handoff snapshot | Use only to recover context that remains consistent with CURRENT sources. |
| [`docs/tunnel/`](../tunnel/) | Historical/operator transport references | Read alongside the CURRENT external-tunnel non-ownership rule; none grants CatDesk authority to create, configure, restart, or duplicate Secure MCP/tunnel runtime. |
| `docs/images/*` screenshots and GIFs | Historical product illustrations | Never use as architecture, health, or acceptance evidence. |

## Classification rule

When material conflicts, prefer the CURRENT sources in the table above, then
stop and reconcile the discrepancy before source, lifecycle, provider, wake,
or host mutation. A document being retained as HISTORICAL or SUPERSEDED does
not make its commands, diagrams, task plans, or observed host state reusable.
