//! Read-only integrated acceptance readiness for the parked host gates.
//!
//! This module composes existing state readers only.  It has no installer,
//! browser, process, pipe, scheduler, wake-owner, or target-update authority.
//! In particular a completed provider session or source test can never become
//! a live-acceptance proof merely by passing through this evaluator.

use std::path::Path;

use serde_json::{Value, json};

use crate::control_plane_supervisor::{
    SupervisorActivationReadinessV1, assess_fixed_supervisor_activation_readiness,
};
use crate::core_host_gate_evidence::{
    CoreHostGateEvidenceReadV1, read_fixed_core_host_gate_evidence,
};
use crate::delegated::autonomy_observability::read_snapshot as read_autonomy_snapshot;
use crate::mcp::{DesignatedChatTargetErrorV1, operator_read_designated_chat_target};
use crate::stable_wake_core::{ReviewState, canonical_inbox_records, workspace_readiness};
use crate::stable_wake_delivery::{ExactWakeDeliveryEvidenceV1, StableWakeDelivery};

/// The only outward classifications. `READY` means the deterministic inputs
/// are ready for the returned next live step; it is not an assertion that the
/// returned gate, or any later live gate, has already been accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoreHostAcceptanceClassificationV1 {
    Ready,
    BlockedByLiveAcceptance,
    FailedDeterministicPrerequisite,
}

impl CoreHostAcceptanceClassificationV1 {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "READY",
            Self::BlockedByLiveAcceptance => "BLOCKED_BY_LIVE_ACCEPTANCE",
            Self::FailedDeterministicPrerequisite => "FAILED_DETERMINISTIC_PREREQUISITE",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoreHostAcceptanceReasonV1 {
    SupervisorReadinessUnavailable,
    ProjectTargetUnavailable,
    ProjectTargetMismatch,
    WakeReadinessUnavailable,
    GuiObservabilityUnavailable,
    T0224NaturalDeliveryRequired,
    T0224EvidenceInvalid,
    T0223HostActivationRequired,
    T0223EvidenceInvalid,
    T0222VisibleGuiRequired,
    T0222EvidenceInvalid,
    T0152AggregateSweepRequired,
    T0152EvidenceInvalid,
    AllLiveGatesAuthoritativelyAccepted,
}

impl CoreHostAcceptanceReasonV1 {
    const fn as_str(self) -> &'static str {
        match self {
            Self::SupervisorReadinessUnavailable => "SUPERVISOR_READINESS_UNAVAILABLE",
            Self::ProjectTargetUnavailable => "PROJECT_TARGET_AUTHORITY_UNAVAILABLE",
            Self::ProjectTargetMismatch => "PROJECT_WAKE_TARGET_MISMATCH",
            Self::WakeReadinessUnavailable => "WAKE_DURABLE_EVIDENCE_UNAVAILABLE",
            Self::GuiObservabilityUnavailable => "GUI_OBSERVABILITY_UNAVAILABLE",
            Self::T0224NaturalDeliveryRequired => "T0224_NATURAL_DELIVERY_REQUIRED",
            Self::T0224EvidenceInvalid => "T0224_LIVE_EVIDENCE_INVALID",
            Self::T0223HostActivationRequired => "T0223_HOST_ACTIVATION_REQUIRED",
            Self::T0223EvidenceInvalid => "T0223_LIVE_EVIDENCE_INVALID",
            Self::T0222VisibleGuiRequired => "T0222_VISIBLE_GUI_REQUIRED",
            Self::T0222EvidenceInvalid => "T0222_LIVE_EVIDENCE_INVALID",
            Self::T0152AggregateSweepRequired => "T0152_AGGREGATE_SWEEP_REQUIRED",
            Self::T0152EvidenceInvalid => "T0152_LIVE_EVIDENCE_INVALID",
            Self::AllLiveGatesAuthoritativelyAccepted => {
                "ALL_CORE_LIVE_GATES_AUTHORITATIVELY_ACCEPTED"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoreHostAcceptanceGateV1 {
    T0224NaturalWake,
    T0223StableSupervisor,
    T0222VisibleGui,
    T0152AggregateSweep,
}

impl CoreHostAcceptanceGateV1 {
    const fn as_str(self) -> &'static str {
        match self {
            Self::T0224NaturalWake => "T-0224",
            Self::T0223StableSupervisor => "T-0223",
            Self::T0222VisibleGui => "T-0222",
            Self::T0152AggregateSweep => "T-0152",
        }
    }
}

/// Input provenance is deliberately explicit.  Only an exact durable proof is
/// eligible to satisfy a gate.  Source-test and provider completion evidence
/// stay non-authoritative even when their fields happen to look complete.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)] // Null/source-only are adversarial evidence classifications.
enum LiveGateEvidenceV1 {
    Missing,
    Stale,
    NullReceipt,
    SourceTestOnly,
    ProviderSessionOnly,
    Invalid,
    ExactDurable {
        project_id: String,
        session_id: String,
        record_id: String,
        target_sha256: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CoreHostAcceptanceInputsV1 {
    supervisor_readiness: SupervisorActivationReadinessV1,
    project_target: Result<String, DesignatedChatTargetErrorV1>,
    wake_target_sha256: Result<String, ()>,
    gui_observability_available: bool,
    t0224: LiveGateEvidenceV1,
    t0223: LiveGateEvidenceV1,
    t0222: LiveGateEvidenceV1,
    t0152: LiveGateEvidenceV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CoreHostAcceptancePreflightV1 {
    classification: CoreHostAcceptanceClassificationV1,
    reason: CoreHostAcceptanceReasonV1,
    next_live_gate: Option<CoreHostAcceptanceGateV1>,
    t0224_accepted: bool,
    t0223_accepted: bool,
    t0222_accepted: bool,
    t0152_accepted: bool,
}

/// A compact read-only view of the canonical evaluator.  Consumers such as
/// the native GUI render this data; they do not receive evaluator inputs and
/// therefore cannot recreate or relax the acceptance decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CoreHostAcceptanceGatePresentationV1 {
    pub(crate) gate: &'static str,
    pub(crate) accepted: bool,
    pub(crate) reason: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CoreHostAcceptancePresentationV1 {
    pub(crate) classification: &'static str,
    pub(crate) reason: &'static str,
    pub(crate) next_live_gate: Option<&'static str>,
    pub(crate) gates: [CoreHostAcceptanceGatePresentationV1; 4],
}

impl CoreHostAcceptancePreflightV1 {
    pub(crate) fn presentation(&self) -> CoreHostAcceptancePresentationV1 {
        let gate = |gate: CoreHostAcceptanceGateV1, accepted: bool| {
            let reason = if accepted {
                "EXACT_DURABLE_LIVE_EVIDENCE"
            } else if self.next_live_gate == Some(gate) {
                self.reason.as_str()
            } else if self.classification
                == CoreHostAcceptanceClassificationV1::FailedDeterministicPrerequisite
            {
                "ORDERED_DETERMINISTIC_PREREQUISITE_UNMET"
            } else {
                "ORDERED_LIVE_GATE_PENDING"
            };
            CoreHostAcceptanceGatePresentationV1 {
                gate: match gate {
                    CoreHostAcceptanceGateV1::T0224NaturalWake => "T-0224",
                    CoreHostAcceptanceGateV1::T0223StableSupervisor => "T-0223",
                    CoreHostAcceptanceGateV1::T0222VisibleGui => "T-0222/T-0139",
                    CoreHostAcceptanceGateV1::T0152AggregateSweep => "T-0152",
                },
                accepted,
                reason,
            }
        };
        CoreHostAcceptancePresentationV1 {
            classification: self.classification.as_str(),
            reason: self.reason.as_str(),
            next_live_gate: self.next_live_gate.map(CoreHostAcceptanceGateV1::as_str),
            gates: [
                gate(
                    CoreHostAcceptanceGateV1::T0224NaturalWake,
                    self.t0224_accepted,
                ),
                gate(
                    CoreHostAcceptanceGateV1::T0223StableSupervisor,
                    self.t0223_accepted,
                ),
                gate(
                    CoreHostAcceptanceGateV1::T0222VisibleGui,
                    self.t0222_accepted,
                ),
                gate(
                    CoreHostAcceptanceGateV1::T0152AggregateSweep,
                    self.t0152_accepted,
                ),
            ],
        }
    }

    pub(crate) fn as_json(&self) -> Value {
        let presentation = self.presentation();
        json!({
            "action": "core_host_acceptance_preflight",
            "classification": presentation.classification,
            "reason": presentation.reason,
            "nextLiveGate": presentation.next_live_gate,
            "liveAcceptance": {
                "T0224": self.t0224_accepted,
                "T0223": self.t0223_accepted,
                "T0222": self.t0222_accepted,
                "T0152": self.t0152_accepted,
            },
        })
    }
}

fn deterministic_failure(reason: CoreHostAcceptanceReasonV1) -> CoreHostAcceptancePreflightV1 {
    CoreHostAcceptancePreflightV1 {
        classification: CoreHostAcceptanceClassificationV1::FailedDeterministicPrerequisite,
        reason,
        next_live_gate: None,
        t0224_accepted: false,
        t0223_accepted: false,
        t0222_accepted: false,
        t0152_accepted: false,
    }
}

fn live_result(
    classification: CoreHostAcceptanceClassificationV1,
    reason: CoreHostAcceptanceReasonV1,
    gate: Option<CoreHostAcceptanceGateV1>,
    accepted: [bool; 4],
) -> CoreHostAcceptancePreflightV1 {
    CoreHostAcceptancePreflightV1 {
        classification,
        reason,
        next_live_gate: gate,
        t0224_accepted: accepted[0],
        t0223_accepted: accepted[1],
        t0222_accepted: accepted[2],
        t0152_accepted: accepted[3],
    }
}

fn exact_binding(
    evidence: &LiveGateEvidenceV1,
    expected_project: &str,
    expected_session: &str,
    expected_target: &str,
) -> Result<bool, ()> {
    match evidence {
        LiveGateEvidenceV1::Missing => Ok(false),
        LiveGateEvidenceV1::ExactDurable {
            project_id,
            session_id,
            record_id,
            target_sha256,
        } if project_id == expected_project
            && session_id == expected_session
            && !record_id.is_empty()
            && target_sha256 == expected_target =>
        {
            Ok(true)
        }
        LiveGateEvidenceV1::ExactDurable { .. }
        | LiveGateEvidenceV1::Stale
        | LiveGateEvidenceV1::NullReceipt
        | LiveGateEvidenceV1::SourceTestOnly
        | LiveGateEvidenceV1::ProviderSessionOnly
        | LiveGateEvidenceV1::Invalid => Err(()),
    }
}

fn evaluate(inputs: &CoreHostAcceptanceInputsV1) -> CoreHostAcceptancePreflightV1 {
    if inputs.supervisor_readiness != SupervisorActivationReadinessV1::Ready {
        return deterministic_failure(CoreHostAcceptanceReasonV1::SupervisorReadinessUnavailable);
    }
    let target = match &inputs.project_target {
        Ok(target) => target,
        Err(_) => {
            return deterministic_failure(CoreHostAcceptanceReasonV1::ProjectTargetUnavailable);
        }
    };
    match &inputs.wake_target_sha256 {
        Ok(wake_target) if wake_target == target => {}
        Ok(_) => return deterministic_failure(CoreHostAcceptanceReasonV1::ProjectTargetMismatch),
        Err(()) => {
            return deterministic_failure(CoreHostAcceptanceReasonV1::WakeReadinessUnavailable);
        }
    }
    if !inputs.gui_observability_available {
        return deterministic_failure(CoreHostAcceptanceReasonV1::GuiObservabilityUnavailable);
    }

    let (project, session) = match &inputs.t0224 {
        LiveGateEvidenceV1::ExactDurable {
            project_id,
            session_id,
            ..
        } if project_id == "catdesk" && !session_id.is_empty() => {
            (project_id.as_str(), session_id.as_str())
        }
        LiveGateEvidenceV1::ExactDurable { .. } => {
            return live_result(
                CoreHostAcceptanceClassificationV1::BlockedByLiveAcceptance,
                CoreHostAcceptanceReasonV1::T0224EvidenceInvalid,
                Some(CoreHostAcceptanceGateV1::T0224NaturalWake),
                [false; 4],
            );
        }
        LiveGateEvidenceV1::Missing => {
            return live_result(
                CoreHostAcceptanceClassificationV1::Ready,
                CoreHostAcceptanceReasonV1::T0224NaturalDeliveryRequired,
                Some(CoreHostAcceptanceGateV1::T0224NaturalWake),
                [false; 4],
            );
        }
        _ => {
            return live_result(
                CoreHostAcceptanceClassificationV1::BlockedByLiveAcceptance,
                CoreHostAcceptanceReasonV1::T0224EvidenceInvalid,
                Some(CoreHostAcceptanceGateV1::T0224NaturalWake),
                [false; 4],
            );
        }
    };
    if exact_binding(&inputs.t0224, project, session, target).is_err() {
        return live_result(
            CoreHostAcceptanceClassificationV1::BlockedByLiveAcceptance,
            CoreHostAcceptanceReasonV1::T0224EvidenceInvalid,
            Some(CoreHostAcceptanceGateV1::T0224NaturalWake),
            [false; 4],
        );
    }

    let accepted = [true, false, false, false];
    match exact_binding(&inputs.t0223, project, session, target) {
        Ok(true) => {}
        Ok(false) => {
            return live_result(
                CoreHostAcceptanceClassificationV1::Ready,
                CoreHostAcceptanceReasonV1::T0223HostActivationRequired,
                Some(CoreHostAcceptanceGateV1::T0223StableSupervisor),
                accepted,
            );
        }
        Err(()) => {
            return live_result(
                CoreHostAcceptanceClassificationV1::BlockedByLiveAcceptance,
                CoreHostAcceptanceReasonV1::T0223EvidenceInvalid,
                Some(CoreHostAcceptanceGateV1::T0223StableSupervisor),
                accepted,
            );
        }
    }

    let accepted = [true, true, false, false];
    match exact_binding(&inputs.t0222, project, session, target) {
        Ok(true) => {}
        Ok(false) => {
            return live_result(
                CoreHostAcceptanceClassificationV1::Ready,
                CoreHostAcceptanceReasonV1::T0222VisibleGuiRequired,
                Some(CoreHostAcceptanceGateV1::T0222VisibleGui),
                accepted,
            );
        }
        Err(()) => {
            return live_result(
                CoreHostAcceptanceClassificationV1::BlockedByLiveAcceptance,
                CoreHostAcceptanceReasonV1::T0222EvidenceInvalid,
                Some(CoreHostAcceptanceGateV1::T0222VisibleGui),
                accepted,
            );
        }
    }

    let accepted = [true, true, true, false];
    match exact_binding(&inputs.t0152, project, session, target) {
        Ok(true) => live_result(
            CoreHostAcceptanceClassificationV1::Ready,
            CoreHostAcceptanceReasonV1::AllLiveGatesAuthoritativelyAccepted,
            None,
            [true; 4],
        ),
        Ok(false) => live_result(
            CoreHostAcceptanceClassificationV1::Ready,
            CoreHostAcceptanceReasonV1::T0152AggregateSweepRequired,
            Some(CoreHostAcceptanceGateV1::T0152AggregateSweep),
            accepted,
        ),
        Err(()) => live_result(
            CoreHostAcceptanceClassificationV1::BlockedByLiveAcceptance,
            CoreHostAcceptanceReasonV1::T0152EvidenceInvalid,
            Some(CoreHostAcceptanceGateV1::T0152AggregateSweep),
            accepted,
        ),
    }
}

fn wake_evidence(workspace: &Path, target: &str) -> Result<LiveGateEvidenceV1, ()> {
    let records = canonical_inbox_records(workspace, "catdesk").map_err(|_| ())?;
    let candidates = records
        .iter()
        .filter(|record| {
            record.unread
                && record.state == ReviewState::CompletedVerified
                && record.next_action == "independent_final_review"
        })
        .collect::<Vec<_>>();
    let [record] = candidates.as_slice() else {
        return Ok(if candidates.is_empty() {
            LiveGateEvidenceV1::Missing
        } else {
            LiveGateEvidenceV1::Stale
        });
    };
    let delivery = StableWakeDelivery::open(workspace).map_err(|_| ())?;
    let Some(evidence) = delivery
        .read_exact_sent_evidence(&record.record_id)
        .map_err(|_| ())?
    else {
        // A final review record proves provider completion only.  It is not
        // browser delivery evidence until the receipt reader proves it.
        return Ok(LiveGateEvidenceV1::ProviderSessionOnly);
    };
    wake_proof(evidence, target)
}

fn wake_proof(
    evidence: ExactWakeDeliveryEvidenceV1,
    expected_target: &str,
) -> Result<LiveGateEvidenceV1, ()> {
    if evidence.project_id != "catdesk"
        || evidence.session_id.is_empty()
        || evidence.record_id.is_empty()
        || evidence.browser_sent_at_unix <= 0.0
        || evidence.receipt_schema_version != 1
        || evidence.message_sha256.len() != 64
        || evidence.target_sha256 != expected_target
    {
        return Ok(LiveGateEvidenceV1::Invalid);
    }
    Ok(LiveGateEvidenceV1::ExactDurable {
        project_id: evidence.project_id,
        session_id: evidence.session_id,
        record_id: evidence.record_id,
        target_sha256: evidence.target_sha256,
    })
}

fn ledger_evidence(evidence: CoreHostGateEvidenceReadV1) -> LiveGateEvidenceV1 {
    match evidence {
        CoreHostGateEvidenceReadV1::Missing => LiveGateEvidenceV1::Missing,
        CoreHostGateEvidenceReadV1::Invalid => LiveGateEvidenceV1::Invalid,
        CoreHostGateEvidenceReadV1::Exact(evidence) => LiveGateEvidenceV1::ExactDurable {
            project_id: evidence.project_id,
            session_id: evidence.session_id,
            record_id: evidence.record_id,
            target_sha256: evidence.target_sha256,
        },
    }
}

/// Production read-only evaluation.  It reads existing fixed supervisor,
/// project/wake-target, inbox/receipt, and GUI-observability authorities.  No
/// implementation here can promote a missing host or visible-GUI proof: those
/// later live gates deliberately remain `Missing` until a future reviewed
/// durable authority exists.
pub(crate) fn read_fixed_core_host_acceptance_preflight(
    workspace: &Path,
) -> CoreHostAcceptancePreflightV1 {
    let wake = workspace_readiness(workspace);
    let target = operator_read_designated_chat_target(workspace).map(|target| target.sha256);
    let wake_target = match (&target, wake.discovery_available, wake.target_available) {
        (Ok(target), true, true) if wake.target_sha256.as_deref() == Some(target.as_str()) => {
            Ok(target.clone())
        }
        (Ok(_), true, true) => Ok(wake.target_sha256.unwrap_or_default()),
        _ => Err(()),
    };
    let t0224 = match &target {
        Ok(target) if wake_target.is_ok() => {
            wake_evidence(workspace, target).unwrap_or(LiveGateEvidenceV1::Invalid)
        }
        _ => LiveGateEvidenceV1::Missing,
    };
    let snapshot = read_autonomy_snapshot(workspace, "LOCAL_READ_ONLY");
    let ledger = read_fixed_core_host_gate_evidence();
    evaluate(&CoreHostAcceptanceInputsV1 {
        supervisor_readiness: assess_fixed_supervisor_activation_readiness(),
        project_target: target,
        wake_target_sha256: wake_target,
        gui_observability_available: !snapshot.stale,
        t0224,
        // Only a fixed protected ledger projection can supply later-gate
        // evidence.  Missing or invalid records remain non-authoritative;
        // this adapter never infers proof from source, GUI, queue, or review
        // workflow state.
        t0223: ledger_evidence(ledger.t0223),
        t0222: ledger_evidence(ledger.t0222),
        t0152: ledger_evidence(ledger.t0152),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: &str = "a8c2c0c3a4b5d6e7f8091029384756abcdefabcdefabcdefabcdefabcdefabcd";

    fn exact(project: &str, session: &str, target: &str) -> LiveGateEvidenceV1 {
        LiveGateEvidenceV1::ExactDurable {
            project_id: project.into(),
            session_id: session.into(),
            record_id: "review-fresh".into(),
            target_sha256: target.into(),
        }
    }

    fn inputs() -> CoreHostAcceptanceInputsV1 {
        CoreHostAcceptanceInputsV1 {
            supervisor_readiness: SupervisorActivationReadinessV1::Ready,
            project_target: Ok(TARGET.into()),
            wake_target_sha256: Ok(TARGET.into()),
            gui_observability_available: true,
            t0224: LiveGateEvidenceV1::Missing,
            t0223: LiveGateEvidenceV1::Missing,
            t0222: LiveGateEvidenceV1::Missing,
            t0152: LiveGateEvidenceV1::Missing,
        }
    }

    #[test]
    fn deterministic_ready_live_missing_is_ready_for_the_next_live_gate() {
        let result = evaluate(&inputs());
        assert_eq!(
            result.classification,
            CoreHostAcceptanceClassificationV1::Ready
        );
        assert_eq!(
            result.reason,
            CoreHostAcceptanceReasonV1::T0224NaturalDeliveryRequired
        );
        assert!(!result.t0224_accepted);
    }

    #[test]
    fn stale_null_source_and_provider_only_evidence_never_accept_t0224() {
        for evidence in [
            LiveGateEvidenceV1::Stale,
            LiveGateEvidenceV1::NullReceipt,
            LiveGateEvidenceV1::SourceTestOnly,
            LiveGateEvidenceV1::ProviderSessionOnly,
        ] {
            let mut fixture = inputs();
            fixture.t0224 = evidence;
            let result = evaluate(&fixture);
            assert_eq!(
                result.classification,
                CoreHostAcceptanceClassificationV1::BlockedByLiveAcceptance
            );
            assert_eq!(
                result.reason,
                CoreHostAcceptanceReasonV1::T0224EvidenceInvalid
            );
            assert!(!result.t0224_accepted);
        }
    }

    #[test]
    fn target_and_project_session_mismatch_are_blocked() {
        for evidence in [
            exact("catdesk", "session-a", "b"),
            exact("other-project", "session-a", TARGET),
        ] {
            let mut fixture = inputs();
            fixture.t0224 = evidence;
            let result = evaluate(&fixture);
            assert_eq!(
                result.classification,
                CoreHostAcceptanceClassificationV1::BlockedByLiveAcceptance
            );
            assert_eq!(
                result.reason,
                CoreHostAcceptanceReasonV1::T0224EvidenceInvalid
            );
        }

        let mut fixture = inputs();
        fixture.t0224 = exact("catdesk", "session-a", TARGET);
        fixture.t0223 = exact("catdesk", "other-session", TARGET);
        let result = evaluate(&fixture);
        assert_eq!(
            result.reason,
            CoreHostAcceptanceReasonV1::T0223EvidenceInvalid
        );
    }

    #[test]
    fn structurally_sent_receipt_must_match_the_current_authoritative_target() {
        let prior_target = "b".repeat(64);
        let current_target_receipt = ExactWakeDeliveryEvidenceV1 {
            record_id: "review-fresh".into(),
            project_id: "catdesk".into(),
            session_id: "session-a".into(),
            browser_sent_at_unix: 1.0,
            message_sha256: "a".repeat(64),
            target_sha256: TARGET.into(),
            receipt_schema_version: 1,
        };
        let mut fixture = inputs();
        fixture.t0224 =
            wake_proof(current_target_receipt, TARGET).expect("bounded receipt classification");

        let result = evaluate(&fixture);
        assert_eq!(
            result.classification,
            CoreHostAcceptanceClassificationV1::Ready
        );
        assert_eq!(
            result.reason,
            CoreHostAcceptanceReasonV1::T0223HostActivationRequired
        );
        assert!(result.t0224_accepted);

        let prior_target_receipt = ExactWakeDeliveryEvidenceV1 {
            record_id: "review-fresh".into(),
            project_id: "catdesk".into(),
            session_id: "session-a".into(),
            browser_sent_at_unix: 1.0,
            message_sha256: "a".repeat(64),
            target_sha256: prior_target,
            receipt_schema_version: 1,
        };
        let mut fixture = inputs();
        fixture.t0224 =
            wake_proof(prior_target_receipt, TARGET).expect("bounded receipt classification");
        let result = evaluate(&fixture);
        assert_eq!(
            result.classification,
            CoreHostAcceptanceClassificationV1::BlockedByLiveAcceptance
        );
        assert_eq!(
            result.reason,
            CoreHostAcceptanceReasonV1::T0224EvidenceInvalid
        );
        assert!(!result.t0224_accepted);
    }

    #[test]
    fn project_and_wake_target_disagreement_stays_a_deterministic_failure() {
        let mut fixture = inputs();
        fixture.wake_target_sha256 = Ok("b".repeat(64));
        fixture.t0224 = exact("catdesk", "session-a", TARGET);

        let result = evaluate(&fixture);
        assert_eq!(
            result.classification,
            CoreHostAcceptanceClassificationV1::FailedDeterministicPrerequisite
        );
        assert_eq!(
            result.reason,
            CoreHostAcceptanceReasonV1::ProjectTargetMismatch
        );
        assert!(!result.t0224_accepted);
    }

    #[test]
    fn deterministic_prerequisite_failure_precedes_all_live_claims() {
        let mut fixture = inputs();
        fixture.supervisor_readiness = SupervisorActivationReadinessV1::RootUnavailable;
        fixture.t0224 = exact("catdesk", "session-a", TARGET);
        let result = evaluate(&fixture);
        assert_eq!(
            result.classification,
            CoreHostAcceptanceClassificationV1::FailedDeterministicPrerequisite
        );
        assert_eq!(
            result.reason,
            CoreHostAcceptanceReasonV1::SupervisorReadinessUnavailable
        );
        assert!(!result.t0224_accepted);
    }

    #[test]
    fn fully_bound_durable_fixture_evidence_is_the_only_full_acceptance_path() {
        let mut fixture = inputs();
        fixture.t0224 = exact("catdesk", "session-a", TARGET);
        fixture.t0223 = exact("catdesk", "session-a", TARGET);
        fixture.t0222 = exact("catdesk", "session-a", TARGET);
        fixture.t0152 = exact("catdesk", "session-a", TARGET);
        let result = evaluate(&fixture);
        assert_eq!(
            result.classification,
            CoreHostAcceptanceClassificationV1::Ready
        );
        assert_eq!(
            result.reason,
            CoreHostAcceptanceReasonV1::AllLiveGatesAuthoritativelyAccepted
        );
        assert_eq!(result.as_json()["liveAcceptance"]["T0152"], true);
    }

    #[test]
    fn evaluation_is_pure_and_does_not_mutate_fixture_evidence() {
        let fixture = inputs();
        let before = fixture.clone();
        let _ = evaluate(&fixture);
        assert_eq!(fixture, before);
    }

    #[test]
    fn output_is_bounded_and_has_no_authority_fields() {
        let text = evaluate(&inputs()).as_json().to_string();
        for forbidden in ["browser", "path", "command", "tunnel", "activate", "wake"] {
            assert!(
                !text.to_ascii_lowercase().contains(forbidden),
                "{forbidden}"
            );
        }
    }
}
