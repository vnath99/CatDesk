//! Closed operator lifecycle composition for the separately-installed stable
//! supervisor.  It deliberately has no path, image, hash, pipe, port,
//! principal, task, service, shell, or tunnel input.

use serde_json::{Value, json};

use crate::control_plane_supervisor::{
    ControlPlaneSupervisorStoreV1, SupervisorActivationReadinessV1, SupervisorInstallerWriterV1,
    assess_fixed_supervisor_activation_readiness, planned_reviewed_supervisor_startup_action_path,
    require_stable_supervisor_runtime_capability, stable_supervisor_runtime_capability_descriptor,
};
use crate::windows_supervisor_startup::{
    FixedSupervisorStartupDefinitionV1, SupervisorStartupAuthorityV1,
    SupervisorStartupDefinitionStateV1, SupervisorStartupErrorV1,
    classify_fixed_supervisor_startup_definition,
    compensate_staged_fixed_supervisor_startup_definition, fixed_supervisor_startup_authority,
    register_or_update_fixed_supervisor_startup_definition,
};

/// The only operator-visible grammar under `operator supervisor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SupervisorOperatorActionV1 {
    Status,
    Preflight,
    Activate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SupervisorLifecycleReasonV1 {
    Ready,
    ReviewedSupervisorImageUnavailable,
    PrincipalPolicyUnproven,
    RuntimeOwnershipUnproven,
    RootUnavailable,
    StateInvalid,
    ReceiptOrImageInvalid,
    StartupAuthorityUnavailable,
    StartupPolicyUnproven,
    StartupRegistrationFailed,
    CompensationFailed,
    ElevationRequired,
    PortAmbiguous,
}

impl SupervisorLifecycleReasonV1 {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "SUPERVISOR_LIFECYCLE_READY",
            Self::ReviewedSupervisorImageUnavailable => "REVIEWED_SUPERVISOR_IMAGE_UNAVAILABLE",
            Self::PrincipalPolicyUnproven => "SUPERVISOR_PRINCIPAL_POLICY_UNPROVEN",
            Self::RuntimeOwnershipUnproven => "SUPERVISOR_RUNTIME_OWNERSHIP_UNPROVEN",
            Self::RootUnavailable => "SUPERVISOR_ROOT_UNAVAILABLE",
            Self::StateInvalid => "SUPERVISOR_STATE_INVALID",
            Self::ReceiptOrImageInvalid => "SUPERVISOR_RECEIPT_OR_IMAGE_INVALID",
            Self::StartupAuthorityUnavailable => "SUPERVISOR_STARTUP_AUTHORITY_UNAVAILABLE",
            Self::StartupPolicyUnproven => "SUPERVISOR_STARTUP_POLICY_UNPROVEN",
            Self::StartupRegistrationFailed => "SUPERVISOR_STARTUP_REGISTRATION_FAILED",
            Self::CompensationFailed => "SUPERVISOR_ACTIVATION_COMPENSATION_FAILED",
            Self::ElevationRequired => "ELEVATION_REQUIRED",
            Self::PortAmbiguous => "SUPERVISOR_3201_PORT_AMBIGUOUS",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)] // deterministic test-only startup classification seam
enum FixedStartupAuthorityV1 {
    Native(SupervisorStartupAuthorityV1),
    Unavailable,
    ElevationRequired,
    PortAmbiguous,
    AvailableForTestOnly,
}

/// Parses only `operator supervisor status|preflight|activate` with no extra
/// words.  In particular no caller can select a lifecycle authority.
pub(crate) fn parse_supervisor_operator_action(
    args: &[String],
) -> Result<SupervisorOperatorActionV1, String> {
    if args.len() != 3
        || args.first().map(String::as_str) != Some("operator")
        || args.get(1).map(String::as_str) != Some("supervisor")
    {
        return Err("operator supervisor accepts exactly one fixed action".into());
    }
    match args[2].as_str() {
        "status" => Ok(SupervisorOperatorActionV1::Status),
        "preflight" => Ok(SupervisorOperatorActionV1::Preflight),
        "activate" => Ok(SupervisorOperatorActionV1::Activate),
        _ => Err("operator supervisor accepts only status, preflight, or activate".into()),
    }
}

/// The fixed role capability re-verifies the accepted reviewed-main-image
/// envelope against the exact safely-opened installed object, then binds only
/// those bytes to `stable-supervisor-runtime-v1`. It is deliberately not the
/// worker digest bridge and never opens build output, siblings, PATH, or a
/// current-directory artifact.
fn production_reviewed_supervisor_image()
-> Result<ReviewedSupervisorImageV1, SupervisorLifecycleReasonV1> {
    reviewed_supervisor_image_from_role_binding(
        crate::reviewed_build::verified_reviewed_stable_supervisor_image(),
    )
}

fn reviewed_supervisor_image_from_role_binding(
    binding: Result<crate::reviewed_build::ReviewedStableSupervisorImageV1, String>,
) -> Result<ReviewedSupervisorImageV1, SupervisorLifecycleReasonV1> {
    let (bytes, sha256, length) = binding
        .map_err(|_| SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable)?
        .into_fixed_installer_payload()
        .map_err(|_| SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable)?;
    if length == 0 || bytes.len() as u64 != length || sha256.len() != 64 {
        return Err(SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable);
    }
    Ok(ReviewedSupervisorImageV1 { bytes, sha256 })
}

fn fixed_startup_authority() -> FixedStartupAuthorityV1 {
    match fixed_supervisor_startup_authority() {
        Ok(authority) => FixedStartupAuthorityV1::Native(authority),
        Err(SupervisorStartupErrorV1::ElevationRequired) => {
            FixedStartupAuthorityV1::ElevationRequired
        }
        Err(_) => FixedStartupAuthorityV1::Unavailable,
    }
}

fn principal_and_runtime_policy() -> Result<(), SupervisorLifecycleReasonV1> {
    crate::windows_supervisor_control_pipe::require_fixed_pipe_principal_policy(
        crate::windows_supervisor_control_pipe::fixed_pipe_principal_policy_descriptor(),
    )
    .map_err(|_| SupervisorLifecycleReasonV1::PrincipalPolicyUnproven)?;
    require_stable_supervisor_runtime_capability(stable_supervisor_runtime_capability_descriptor())
        .map_err(|_| SupervisorLifecycleReasonV1::RuntimeOwnershipUnproven)
}

fn readiness_reason(readiness: SupervisorActivationReadinessV1) -> SupervisorLifecycleReasonV1 {
    match readiness {
        SupervisorActivationReadinessV1::Ready => SupervisorLifecycleReasonV1::Ready,
        SupervisorActivationReadinessV1::RootUnavailable
        | SupervisorActivationReadinessV1::CurrentReceiptMissing => {
            SupervisorLifecycleReasonV1::RootUnavailable
        }
        SupervisorActivationReadinessV1::StateInvalid => SupervisorLifecycleReasonV1::StateInvalid,
        SupervisorActivationReadinessV1::PrincipalPolicyUnproven => {
            SupervisorLifecycleReasonV1::PrincipalPolicyUnproven
        }
        SupervisorActivationReadinessV1::RuntimeOwnershipUnproven => {
            SupervisorLifecycleReasonV1::RuntimeOwnershipUnproven
        }
        SupervisorActivationReadinessV1::ReceiptOrImageInvalid
        | SupervisorActivationReadinessV1::FixedPolicyInvalid => {
            SupervisorLifecycleReasonV1::ReceiptOrImageInvalid
        }
    }
}

fn preflight_with(
    image: Result<ReviewedSupervisorImageV1, SupervisorLifecycleReasonV1>,
    startup: FixedStartupAuthorityV1,
    readiness: SupervisorActivationReadinessV1,
    policy: Result<(), SupervisorLifecycleReasonV1>,
) -> SupervisorLifecycleReasonV1 {
    if let Err(reason) = policy {
        return reason;
    }
    if image.is_err() {
        return SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable;
    }
    match startup {
        FixedStartupAuthorityV1::Native(_) => {}
        FixedStartupAuthorityV1::Unavailable => {
            return SupervisorLifecycleReasonV1::StartupAuthorityUnavailable;
        }
        FixedStartupAuthorityV1::ElevationRequired => {
            return SupervisorLifecycleReasonV1::ElevationRequired;
        }
        FixedStartupAuthorityV1::PortAmbiguous => {
            return SupervisorLifecycleReasonV1::PortAmbiguous;
        }
        FixedStartupAuthorityV1::AvailableForTestOnly => {}
    }
    readiness_reason(readiness)
}

fn production_preflight() -> SupervisorLifecycleReasonV1 {
    let policy = principal_and_runtime_policy();
    let image = production_reviewed_supervisor_image();
    if let Err(reason) = policy {
        return reason;
    }
    if image.is_err() {
        return SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable;
    }
    let startup = fixed_startup_authority();
    if startup_reason(&startup) != SupervisorLifecycleReasonV1::Ready {
        return startup_reason(&startup);
    }
    let startup_policy = fixed_startup_policy_precondition(
        &startup,
        &image.as_ref().expect("image was checked above").sha256,
    );
    if startup_policy != SupervisorLifecycleReasonV1::Ready {
        return startup_policy;
    }
    preflight_with(
        image,
        startup,
        assess_fixed_supervisor_activation_readiness(),
        Ok(()),
    )
}

/// Returns only fixed, non-secret categories. It is read-only: no root create,
/// process launch, port probe, pipe client, receipt/state write, or worker/
/// tunnel action occurs here.
fn fixed_status() -> Value {
    let readiness = assess_fixed_supervisor_activation_readiness();
    let image = production_reviewed_supervisor_image();
    let startup = fixed_startup_authority();
    let image_reason = image
        .as_ref()
        .map(|_| SupervisorLifecycleReasonV1::Ready)
        .unwrap_or_else(|reason| *reason);
    let startup_policy = image
        .as_ref()
        .map(|image| fixed_startup_policy_precondition(&startup, &image.sha256))
        .unwrap_or(image_reason);
    let startup_definition = image
        .as_ref()
        .map(|image| fixed_startup_definition_status(&startup, &image.sha256))
        .unwrap_or("SUPERVISOR_STARTUP_DEFINITION_NOT_EVALUATED");
    json!({
        "action": "supervisor_status",
        "readiness": readiness.as_str(),
        "reviewedSupervisorImage": image_reason.as_str(),
        "startup": startup_reason(&startup).as_str(),
        "startupPolicy": startup_policy.as_str(),
        "startupDefinition": startup_definition,
        "frontDoor3201": "SUPERVISOR_3201_NOT_PROBED",
        "activationBlocked": production_preflight().as_str(),
    })
}

fn startup_reason(startup: &FixedStartupAuthorityV1) -> SupervisorLifecycleReasonV1 {
    match startup {
        FixedStartupAuthorityV1::Native(_) => SupervisorLifecycleReasonV1::Ready,
        FixedStartupAuthorityV1::Unavailable => {
            SupervisorLifecycleReasonV1::StartupAuthorityUnavailable
        }
        FixedStartupAuthorityV1::ElevationRequired => {
            SupervisorLifecycleReasonV1::ElevationRequired
        }
        FixedStartupAuthorityV1::AvailableForTestOnly => SupervisorLifecycleReasonV1::Ready,
        FixedStartupAuthorityV1::PortAmbiguous => SupervisorLifecycleReasonV1::PortAmbiguous,
    }
}

/// Called only by the closed operator facade. Production fails before the
/// crate-private installer writer because supervisor-image authority is absent.
fn production_activate() -> SupervisorLifecycleReasonV1 {
    activate_fixed_authority(
        production_reviewed_supervisor_image(),
        fixed_startup_authority(),
        principal_and_runtime_policy(),
        fixed_state_precondition(),
    )
}

fn fixed_state_precondition() -> SupervisorLifecycleReasonV1 {
    let store = match ControlPlaneSupervisorStoreV1::fixed_read_only() {
        Ok(store) => store,
        Err(_) => return SupervisorLifecycleReasonV1::RootUnavailable,
    };
    if store.load().is_err() {
        return SupervisorLifecycleReasonV1::StateInvalid;
    }
    SupervisorLifecycleReasonV1::Ready
}

fn fixed_startup_definition_status(
    startup: &FixedStartupAuthorityV1,
    reviewed_manifest_sha256: &str,
) -> &'static str {
    let FixedStartupAuthorityV1::Native(authority) = startup else {
        return "SUPERVISOR_STARTUP_DEFINITION_NOT_AVAILABLE";
    };
    let action = match planned_reviewed_supervisor_startup_action_path(reviewed_manifest_sha256) {
        Ok(action) => action,
        Err(_) => return "SUPERVISOR_STARTUP_DEFINITION_ACTION_INVALID",
    };
    let definition = match FixedSupervisorStartupDefinitionV1::from_protected_installer_action(
        authority, &action,
    ) {
        Ok(definition) => definition,
        Err(_) => return "SUPERVISOR_STARTUP_DEFINITION_INVALID",
    };
    match classify_fixed_supervisor_startup_definition(authority, &definition) {
        Ok(SupervisorStartupDefinitionStateV1::Absent) => "SUPERVISOR_STARTUP_DEFINITION_ABSENT",
        Ok(SupervisorStartupDefinitionStateV1::ExactOwned) => {
            "SUPERVISOR_STARTUP_DEFINITION_EXACT_OWNED"
        }
        Ok(SupervisorStartupDefinitionStateV1::ForeignOrAmbiguous) => {
            "SUPERVISOR_STARTUP_DEFINITION_FOREIGN_OR_AMBIGUOUS"
        }
        Err(_) => "SUPERVISOR_STARTUP_DEFINITION_READ_FAILED",
    }
}

fn fixed_startup_policy_precondition(
    startup: &FixedStartupAuthorityV1,
    reviewed_manifest_sha256: &str,
) -> SupervisorLifecycleReasonV1 {
    let FixedStartupAuthorityV1::Native(authority) = startup else {
        return startup_reason(startup);
    };
    let action = match planned_reviewed_supervisor_startup_action_path(reviewed_manifest_sha256) {
        Ok(action) => action,
        Err(_) => return SupervisorLifecycleReasonV1::ReceiptOrImageInvalid,
    };
    let definition = match FixedSupervisorStartupDefinitionV1::from_protected_installer_action(
        authority, &action,
    ) {
        Ok(definition) => definition,
        Err(_) => return SupervisorLifecycleReasonV1::StartupPolicyUnproven,
    };
    match classify_fixed_supervisor_startup_definition(authority, &definition) {
        Ok(
            SupervisorStartupDefinitionStateV1::Absent
            | SupervisorStartupDefinitionStateV1::ExactOwned,
        ) => SupervisorLifecycleReasonV1::Ready,
        Ok(SupervisorStartupDefinitionStateV1::ForeignOrAmbiguous) | Err(_) => {
            SupervisorLifecycleReasonV1::StartupPolicyUnproven
        }
    }
}

/// The sole production installer composition. Its root is fixed by the
/// writer, image bytes/digest come only from the distinct reviewed-supervisor
/// capability, and startup authority is a fixed internal classification.
fn activate_fixed_authority(
    image: Result<ReviewedSupervisorImageV1, SupervisorLifecycleReasonV1>,
    startup: FixedStartupAuthorityV1,
    policy: Result<(), SupervisorLifecycleReasonV1>,
    state_precondition: SupervisorLifecycleReasonV1,
) -> SupervisorLifecycleReasonV1 {
    if let Err(reason) = policy {
        return reason;
    }
    let image = match image {
        Ok(image) => image,
        Err(reason) => return reason,
    };
    match startup_reason(&startup) {
        SupervisorLifecycleReasonV1::Ready => {}
        reason => return reason,
    }
    if state_precondition != SupervisorLifecycleReasonV1::Ready {
        return state_precondition;
    }
    if image.bytes.is_empty() || image.bytes.len() > 256 * 1024 * 1024 || image.sha256.len() != 64 {
        return SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable;
    }
    // Every scheduler observation is complete before the first protected
    // installer mutation.  A fixed-name foreign task is therefore refused
    // before an image/receipt can change.  The prior exact definition is kept
    // only to permit the narrow post-install action-path update after a second
    // native exact read; no blind Task Scheduler overwrite exists.
    let startup_plan = match &startup {
        FixedStartupAuthorityV1::Native(authority) => {
            let action = match planned_reviewed_supervisor_startup_action_path(&image.sha256) {
                Ok(action) => action,
                Err(_) => return SupervisorLifecycleReasonV1::ReceiptOrImageInvalid,
            };
            let definition =
                match FixedSupervisorStartupDefinitionV1::from_protected_installer_action(
                    authority, &action,
                ) {
                    Ok(definition) => definition,
                    Err(_) => return SupervisorLifecycleReasonV1::StartupPolicyUnproven,
                };
            let state = match classify_fixed_supervisor_startup_definition(authority, &definition) {
                Ok(SupervisorStartupDefinitionStateV1::Absent) => {
                    SupervisorStartupDefinitionStateV1::Absent
                }
                Ok(SupervisorStartupDefinitionStateV1::ExactOwned) => {
                    SupervisorStartupDefinitionStateV1::ExactOwned
                }
                Ok(SupervisorStartupDefinitionStateV1::ForeignOrAmbiguous) | Err(_) => {
                    return SupervisorLifecycleReasonV1::StartupPolicyUnproven;
                }
            };
            Some((authority.clone(), definition, state))
        }
        FixedStartupAuthorityV1::AvailableForTestOnly => None,
        _ => return startup_reason(&startup),
    };
    // Capture both independently validated protected receipts before any
    // mutation. The fixed-purpose snapshot records each receipt as an exact
    // present object or a positively established handle-relative absence.
    let writer = SupervisorInstallerWriterV1::fixed_policy();
    let receipt_snapshot = match writer.snapshot_exact_install_receipts() {
        Ok(snapshot) => snapshot,
        Err(_) => return SupervisorLifecycleReasonV1::ReceiptOrImageInvalid,
    };
    // Prepare only an inert side-by-side version before any Scheduler
    // mutation. In particular, scheduler registration failure below leaves
    // `supervisor-current.json` untouched; LKG is recovery material and is
    // never used as transaction rollback.
    let prepared = match writer.prepare_exact_reviewed_image(&image.bytes, &image.sha256) {
        Ok(receipt) => receipt,
        Err(_) => return SupervisorLifecycleReasonV1::ReceiptOrImageInvalid,
    };
    if let Some((authority, previous, previous_state)) = startup_plan {
        let action =
            match planned_reviewed_supervisor_startup_action_path(&prepared.manifest_sha256) {
                Ok(action) => action,
                Err(_) => return SupervisorLifecycleReasonV1::StartupRegistrationFailed,
            };
        let next = match FixedSupervisorStartupDefinitionV1::from_protected_installer_action(
            &authority, &action,
        ) {
            Ok(definition) => definition,
            Err(_) => return SupervisorLifecycleReasonV1::StartupRegistrationFailed,
        };
        let staged = next.staged_disabled();
        if register_or_update_fixed_supervisor_startup_definition(
            &authority,
            &previous,
            previous_state,
            &staged,
        )
        .is_err()
        {
            return SupervisorLifecycleReasonV1::StartupRegistrationFailed;
        }
        if writer
            .commit_prepared_exact_reviewed_image(&prepared)
            .is_err()
        {
            // Compensation is intentionally narrow and revalidates the live
            // disabled task before delete/restore. A changed/foreign task is
            // left untouched and the lifecycle fails closed.
            let receipts_restored = writer
                .restore_exact_install_receipt_snapshot(&receipt_snapshot, &prepared)
                .is_ok();
            let task_restored = compensate_staged_fixed_supervisor_startup_definition(
                &authority,
                &previous,
                previous_state,
                &staged,
            )
            .is_ok();
            return if receipts_restored && task_restored {
                SupervisorLifecycleReasonV1::ReceiptOrImageInvalid
            } else {
                SupervisorLifecycleReasonV1::CompensationFailed
            };
        }
        if register_or_update_fixed_supervisor_startup_definition(
            &authority,
            &staged,
            SupervisorStartupDefinitionStateV1::ExactOwned,
            &next,
        )
        .is_err()
        {
            // Final enable/readback is after current/LKG commit.  Restore
            // both protected receipts first, then the exact prior task or
            // absence. A disabled task plus advanced current is never an
            // accepted terminal state.
            let receipts_restored = writer
                .restore_exact_install_receipt_snapshot(&receipt_snapshot, &prepared)
                .is_ok();
            let task_restored = compensate_staged_fixed_supervisor_startup_definition(
                &authority,
                &previous,
                previous_state,
                &staged,
            )
            .is_ok();
            return if receipts_restored && task_restored {
                SupervisorLifecycleReasonV1::StartupRegistrationFailed
            } else {
                SupervisorLifecycleReasonV1::CompensationFailed
            };
        }
    } else if writer
        .commit_prepared_exact_reviewed_image(&prepared)
        .is_err()
    {
        return SupervisorLifecycleReasonV1::ReceiptOrImageInvalid;
    }
    readiness_reason(assess_fixed_supervisor_activation_readiness())
}

pub(crate) fn execute_supervisor_operator_action(action: SupervisorOperatorActionV1) -> Value {
    match action {
        SupervisorOperatorActionV1::Status => fixed_status(),
        SupervisorOperatorActionV1::Preflight => json!({
            "action": "supervisor_preflight",
            "result": production_preflight().as_str(),
        }),
        SupervisorOperatorActionV1::Activate => json!({
            "action": "supervisor_activate",
            "result": production_activate().as_str(),
        }),
    }
}

#[derive(Clone, Debug)]
struct ReviewedSupervisorImageV1 {
    bytes: Vec<u8>,
    sha256: String,
}

#[cfg(test)]
fn test_reviewed_supervisor_image(bytes: &[u8]) -> ReviewedSupervisorImageV1 {
    use sha2::Digest;
    ReviewedSupervisorImageV1 {
        bytes: bytes.to_vec(),
        sha256: format!("{:x}", sha2::Sha256::digest(bytes)),
    }
}

#[cfg(test)]
fn activate_test_seam(
    root: std::path::PathBuf,
    image: Result<ReviewedSupervisorImageV1, SupervisorLifecycleReasonV1>,
    startup: FixedStartupAuthorityV1,
    policy: Result<(), SupervisorLifecycleReasonV1>,
) -> SupervisorLifecycleReasonV1 {
    if let Err(reason) = policy {
        return reason;
    }
    let image = match image {
        Ok(image) => image,
        Err(reason) => return reason,
    };
    match startup_reason(&startup) {
        SupervisorLifecycleReasonV1::Ready => {}
        reason => return reason,
    }
    if image.bytes.is_empty() || image.bytes.len() > 256 * 1024 * 1024 || image.sha256.len() != 64 {
        return SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable;
    }
    let state_precondition = test_state_precondition(&root);
    if state_precondition != SupervisorLifecycleReasonV1::Ready {
        return state_precondition;
    }
    let writer = SupervisorInstallerWriterV1::test_seam(root.clone());
    if writer
        .install_exact_reviewed_image(&image.bytes, &image.sha256)
        .is_err()
    {
        return SupervisorLifecycleReasonV1::ReceiptOrImageInvalid;
    }
    readiness_reason(
        crate::control_plane_supervisor::assess_supervisor_activation_readiness_test_seam(&root),
    )
}

#[cfg(test)]
fn test_state_precondition(root: &std::path::Path) -> SupervisorLifecycleReasonV1 {
    if !root.is_dir() {
        return SupervisorLifecycleReasonV1::RootUnavailable;
    }
    match ControlPlaneSupervisorStoreV1::open_test_seam(root.to_path_buf()) {
        Ok(store) if store.load().is_ok() => SupervisorLifecycleReasonV1::Ready,
        _ => SupervisorLifecycleReasonV1::StateInvalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn fixture_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "catdesk-supervisor-lifecycle-{label}-{}",
            Uuid::new_v4()
        ))
    }

    #[test]
    fn operator_grammar_is_exact_and_has_no_authority_arguments() {
        for (word, expected) in [
            ("status", SupervisorOperatorActionV1::Status),
            ("preflight", SupervisorOperatorActionV1::Preflight),
            ("activate", SupervisorOperatorActionV1::Activate),
        ] {
            assert_eq!(
                parse_supervisor_operator_action(&[
                    "operator".into(),
                    "supervisor".into(),
                    word.into()
                ]),
                Ok(expected)
            );
        }
        for args in [
            vec!["operator", "supervisor"],
            vec!["operator", "anything", "status"],
            // Legacy release repair is deliberately not a supervisor action:
            // it could otherwise reintroduce a version-coupled daemon/script
            // recovery owner beside the fixed lifecycle transaction.
            vec!["operator", "supervisor", "recover"],
            vec!["operator", "supervisor", "unknown"],
            vec!["operator", "supervisor", "activate", "--path", "x"],
            vec!["operator", "supervisor", "activate", "--hash", "a"],
            vec!["operator", "supervisor", "status", "--pipe", "x"],
            vec!["operator", "supervisor", "activate", "--service", "x"],
            vec!["operator", "supervisor", "activate", "--sid", "x"],
            vec!["operator", "supervisor", "activate", "--session", "x"],
            vec!["operator", "supervisor", "activate", "--port", "3201"],
            vec!["operator", "supervisor", "activate", "--startup", "x"],
            vec!["operator", "supervisor", "activate", "--policy", "x"],
            vec!["operator", "supervisor", "preflight", "--tunnel", "x"],
        ] {
            assert!(
                parse_supervisor_operator_action(
                    &args.into_iter().map(String::from).collect::<Vec<_>>()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn missing_reviewed_supervisor_role_binding_fails_closed_without_mutation() {
        assert_eq!(
            preflight_with(
                Err(SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable),
                FixedStartupAuthorityV1::Unavailable,
                SupervisorActivationReadinessV1::Ready,
                Ok(()),
            ),
            SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable
        );
        assert_eq!(
            activate_fixed_authority(
                Err(SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable),
                FixedStartupAuthorityV1::Unavailable,
                Ok(()),
                SupervisorLifecycleReasonV1::Ready,
            ),
            SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable
        );
    }

    #[test]
    fn preflight_classifies_policy_startup_and_protected_state_failures_without_mutation() {
        let image = test_reviewed_supervisor_image(b"reviewed supervisor fixture");
        assert_eq!(
            preflight_with(
                Ok(image.clone()),
                FixedStartupAuthorityV1::ElevationRequired,
                SupervisorActivationReadinessV1::Ready,
                Ok(()),
            ),
            SupervisorLifecycleReasonV1::ElevationRequired
        );
        assert_eq!(
            preflight_with(
                Ok(image.clone()),
                FixedStartupAuthorityV1::Unavailable,
                SupervisorActivationReadinessV1::Ready,
                Ok(()),
            ),
            SupervisorLifecycleReasonV1::StartupAuthorityUnavailable
        );
        assert_eq!(
            preflight_with(
                Ok(image.clone()),
                FixedStartupAuthorityV1::PortAmbiguous,
                SupervisorActivationReadinessV1::Ready,
                Ok(()),
            ),
            SupervisorLifecycleReasonV1::PortAmbiguous
        );
        assert_eq!(
            preflight_with(
                Ok(image.clone()),
                FixedStartupAuthorityV1::AvailableForTestOnly,
                SupervisorActivationReadinessV1::ReceiptOrImageInvalid,
                Ok(()),
            ),
            SupervisorLifecycleReasonV1::ReceiptOrImageInvalid
        );
        assert_eq!(
            preflight_with(
                Ok(image),
                FixedStartupAuthorityV1::AvailableForTestOnly,
                SupervisorActivationReadinessV1::Ready,
                Err(SupervisorLifecycleReasonV1::PrincipalPolicyUnproven),
            ),
            SupervisorLifecycleReasonV1::PrincipalPolicyUnproven
        );
    }

    #[test]
    fn explicit_reviewed_supervisor_role_binding_advances_only_to_startup_authority() {
        let image = reviewed_supervisor_image_from_role_binding(Ok(
            crate::reviewed_build::test_only_verified_stable_supervisor_image(),
        ));
        assert!(image.is_ok(), "isolated explicit role binding is accepted");
        assert_eq!(
            preflight_with(
                image,
                FixedStartupAuthorityV1::Unavailable,
                SupervisorActivationReadinessV1::Ready,
                Ok(()),
            ),
            SupervisorLifecycleReasonV1::StartupAuthorityUnavailable
        );
        assert_eq!(
            preflight_with(
                Err(SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable),
                FixedStartupAuthorityV1::Unavailable,
                SupervisorActivationReadinessV1::Ready,
                Ok(()),
            ),
            SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable
        );
    }

    #[test]
    fn typed_principal_and_runtime_policy_failures_remain_distinct_through_both_compositions() {
        let root = fixture_root("typed-policy");
        let image = test_reviewed_supervisor_image(b"reviewed supervisor fixture");
        for reason in [
            SupervisorLifecycleReasonV1::PrincipalPolicyUnproven,
            SupervisorLifecycleReasonV1::RuntimeOwnershipUnproven,
        ] {
            assert_eq!(
                preflight_with(
                    Ok(image.clone()),
                    FixedStartupAuthorityV1::AvailableForTestOnly,
                    SupervisorActivationReadinessV1::Ready,
                    Err(reason),
                ),
                reason
            );
            assert_eq!(
                activate_test_seam(
                    root.clone(),
                    Ok(image.clone()),
                    FixedStartupAuthorityV1::AvailableForTestOnly,
                    Err(reason),
                ),
                reason
            );
            assert!(
                !root.exists(),
                "policy failure may not create a fixture root"
            );
        }
    }

    #[test]
    fn test_activation_requires_all_authorities_before_mutating_fixture() {
        let root = fixture_root("gates");
        let image = test_reviewed_supervisor_image(b"reviewed supervisor fixture");
        for (image, startup, policy) in [
            (
                Err(SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable),
                FixedStartupAuthorityV1::AvailableForTestOnly,
                Ok(()),
            ),
            (
                Ok(image.clone()),
                FixedStartupAuthorityV1::Unavailable,
                Ok(()),
            ),
            (
                Ok(image.clone()),
                FixedStartupAuthorityV1::AvailableForTestOnly,
                Err(SupervisorLifecycleReasonV1::PrincipalPolicyUnproven),
            ),
        ] {
            assert_ne!(
                activate_test_seam(root.clone(), image, startup, policy),
                SupervisorLifecycleReasonV1::Ready
            );
            assert!(!root.exists(), "a rejected fixture may not create a root");
        }
        let malformed = ReviewedSupervisorImageV1 {
            bytes: b"reviewed supervisor fixture".to_vec(),
            sha256: "bad".into(),
        };
        assert_eq!(
            activate_test_seam(
                root.clone(),
                Ok(malformed),
                FixedStartupAuthorityV1::AvailableForTestOnly,
                Ok(()),
            ),
            SupervisorLifecycleReasonV1::ReviewedSupervisorImageUnavailable
        );
        assert!(!root.exists());
        assert_eq!(
            activate_test_seam(
                root.clone(),
                Ok(image),
                FixedStartupAuthorityV1::AvailableForTestOnly,
                Ok(()),
            ),
            SupervisorLifecycleReasonV1::RootUnavailable
        );
        assert!(!root.exists(), "missing state may not be installed around");
    }

    #[test]
    fn invalid_protected_state_blocks_before_the_fixed_installer_can_write_a_receipt() {
        let root = fixture_root("invalid-state");
        let _store = ControlPlaneSupervisorStoreV1::open_test_seam(root.clone())
            .expect("create only the isolated protected root");
        fs::write(
            root.join("control-plane-supervisor.json"),
            b"malformed state",
        )
        .expect("malformed state fixture");
        let image = test_reviewed_supervisor_image(b"reviewed supervisor fixture");
        assert_eq!(
            activate_test_seam(
                root.clone(),
                Ok(image),
                FixedStartupAuthorityV1::AvailableForTestOnly,
                Ok(()),
            ),
            SupervisorLifecycleReasonV1::StateInvalid
        );
        assert!(!root.join("supervisor-current.json").exists());
        assert!(!root.join("versions").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn exact_reviewed_fixture_installs_idempotently_and_rechecks_readiness() {
        let root = fixture_root("activate");
        let store = crate::control_plane_supervisor::ControlPlaneSupervisorStoreV1::open_test_seam(
            root.clone(),
        )
        .expect("state fixture");
        store.save(&Default::default()).expect("state fixture save");
        let first = test_reviewed_supervisor_image(b"reviewed supervisor fixture one");
        assert_eq!(
            activate_test_seam(
                root.clone(),
                Ok(first.clone()),
                FixedStartupAuthorityV1::AvailableForTestOnly,
                Ok(())
            ),
            SupervisorLifecycleReasonV1::Ready
        );
        assert_eq!(
            activate_test_seam(
                root.clone(),
                Ok(first),
                FixedStartupAuthorityV1::AvailableForTestOnly,
                Ok(())
            ),
            SupervisorLifecycleReasonV1::Ready
        );
        let second = test_reviewed_supervisor_image(b"reviewed supervisor fixture two");
        assert_eq!(
            activate_test_seam(
                root.clone(),
                Ok(second),
                FixedStartupAuthorityV1::AvailableForTestOnly,
                Ok(())
            ),
            SupervisorLifecycleReasonV1::Ready
        );
        assert!(
            root.join("supervisor-lkg.json").is_file(),
            "LKG must survive update"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lifecycle_source_never_borrows_worker_trust_or_generic_lifecycle_authority() {
        let source = include_str!("supervisor_lifecycle.rs");
        for forbidden in [
            ["verified_current_reviewed_main", "_image_digest"].concat(),
            ["std::process", "::Command"].concat(),
            ["Command", "::new"].concat(),
        ] {
            assert!(
                !source.contains(&forbidden),
                "forbidden lifecycle authority: {forbidden}"
            );
        }
        assert!(source.contains("REVIEWED_SUPERVISOR_IMAGE_UNAVAILABLE"));
        assert!(source.contains("install_exact_reviewed_image"));
        let preflight = source
            .split("fn production_preflight")
            .nth(1)
            .and_then(|tail| tail.split("fn fixed_status").next())
            .expect("production preflight source");
        assert!(preflight.contains("if let Err(reason) = policy"));
        assert!(!preflight.contains("policy = principal_and_runtime_policy().is_ok()"));
        let activation = source
            .split("fn activate_fixed_authority")
            .nth(1)
            .and_then(|tail| {
                tail.split("pub(crate) fn execute_supervisor_operator_action")
                    .next()
            })
            .expect("activation composition source");
        let prepare = activation
            .find("prepare_exact_reviewed_image")
            .expect("fixed protected prepare call");
        let commit = activation
            .find("commit_prepared_exact_reviewed_image")
            .expect("fixed protected commit call");
        assert!(activation[..prepare].contains("classify_fixed_supervisor_startup_definition"));
        assert!(activation[prepare..commit].contains("staged_disabled"));
        assert!(
            activation[commit..].contains("register_or_update_fixed_supervisor_startup_definition")
        );
    }

    #[test]
    fn closed_supervisor_status_and_activation_cannot_fall_back_to_legacy_recovery_owners() {
        let source = include_str!("supervisor_lifecycle.rs");
        let status = source
            .split("fn fixed_status")
            .nth(1)
            .and_then(|tail| tail.split("fn startup_reason").next())
            .expect("fixed read-only status source");
        let activation = source
            .split("fn activate_fixed_authority")
            .nth(1)
            .and_then(|tail| {
                tail.split("pub(crate) fn execute_supervisor_operator_action")
                    .next()
            })
            .expect("closed activation source");

        // The stable surface cannot acquire authority from the versioned
        // daemon-reload/script recovery owner, browser state, or a tunnel.
        // It is intentionally independent of whether those legacy surfaces
        // still exist for historical compatibility elsewhere in the product.
        for forbidden in [
            "daemon_reload",
            "canonical_release_recovery",
            "run_fixed_powershell_invocation_blocking",
            "catdesk.ps1",
            "start-catdesk-stack.ps1",
            "--catdesk-daemon",
            "browser::",
        ] {
            assert!(
                !status.contains(forbidden) && !activation.contains(forbidden),
                "stable lifecycle must not borrow legacy recovery authority: {forbidden}"
            );
        }
        assert!(status.contains("assess_fixed_supervisor_activation_readiness"));
        assert!(activation.contains("SupervisorInstallerWriterV1::fixed_policy"));
        assert!(activation.contains("snapshot_exact_install_receipts"));
        assert!(activation.contains("restore_exact_install_receipt_snapshot"));
    }
}
