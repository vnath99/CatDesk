//! Local operator workflow for a frozen batch of routine source reviews.
//! Consent lives in WorkLedger; only the existing Wake sender can submit.
use super::*;
use crate::web_work_state::{ReviewDispatchPermit, ReviewGrantRequest, ReviewMessageIdentity};
use catdesk_wake::store::Store;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GrantInput {
    project_id: String,
    authorization: ReviewGrantRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RevokeInput {
    project_id: String,
    grant_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewInput {
    project_id: String,
    grant_id: String,
    review_key: String,
    request: Value,
}

fn with_project<T>(
    workspace: &Path,
    project: &str,
    target: &protocol::Target,
    action: impl FnOnce(&WorkLedger) -> Result<T, String>,
) -> Result<T, String> {
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .map_err(|_| "WEB_REVIEW_REGISTRY_UNAVAILABLE")?;
    registry.with_registration_lock(|| {
        let scope = resolve_registered_project_from_control_root(workspace, project)
            .map_err(|_| "WEB_REVIEW_PROJECT_UNAVAILABLE")?;
        if scope.project.chatgpt_target_url.as_deref() != Some(target.url.as_str())
            || scope.project.chatgpt_target_sha256.as_deref() != Some(target.digest.as_str())
        {
            return Err("WEB_REVIEW_TARGET_CHANGED".into());
        }
        action(&WorkLedger::open(workspace, project)?)
    })
}

fn target(workspace: &Path, project: &str) -> Result<protocol::Target, String> {
    let paired = mcp::operator_read_project_web_target(workspace, project)
        .map_err(|_| "WEB_REVIEW_PAIRED_TARGET_REQUIRED")?
        .ok_or("WEB_REVIEW_PAIRED_TARGET_REQUIRED")?;
    protocol::Target::new(paired.generation, paired.url)
}

fn require_stopped(store: &Store) -> Result<(), String> {
    let control: catdesk_wake::runtime::Control =
        catdesk_wake::store::read(&store.root().join("control.json"))?;
    if control.schema_version != 1 || control.desired != "STOPPED" {
        return Err("WEB_REVIEW_HOST_NOT_STOPPED".into());
    }
    Ok(())
}

pub(crate) fn action(workspace: &Path, operation: &str, args: Value) -> Result<Value, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "WEB_REVIEW_CONTROL_UNAVAILABLE")?;
    if operation == "authorize" {
        let input: GrantInput =
            serde_json::from_value(args).map_err(|_| "WEB_REVIEW_ARGUMENTS_INVALID")?;
        // Consent cannot rebind a project or resolve a quarantined rollover.
        let lease = crate::operator_facade::acquire_web_work_lease(&workspace, &input.project_id)?;
        let target = target(&workspace, &input.project_id)?;
        lease.check_target(&target)?;
        if target != input.authorization.target {
            return Err("WEB_REVIEW_TARGET_CHANGED".into());
        }
        wake_protocol_client::preflight_registered_invitation(
            &workspace,
            &input.project_id,
            &target,
        )?;
        return with_project(&workspace, &input.project_id, &target, |ledger| {
            ledger.authorize_review(input.authorization, now())
        });
    }
    if operation == "revoke" {
        let input: RevokeInput =
            serde_json::from_value(args).map_err(|_| "WEB_REVIEW_ARGUMENTS_INVALID")?;
        // Revocation must remain available even after target drift or quarantine.
        let registry =
            AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
                .map_err(|_| "WEB_REVIEW_REGISTRY_UNAVAILABLE")?;
        return registry.with_registration_lock(|| {
            resolve_registered_project_from_control_root(&workspace, &input.project_id)
                .map_err(|_| "WEB_REVIEW_PROJECT_UNAVAILABLE")?;
            WorkLedger::open(&workspace, &input.project_id)?.revoke_review(&input.grant_id, now())
        });
    }
    if !matches!(operation, "preflight" | "run" | "resume") {
        return Err("WEB_REVIEW_ACTION_INVALID".into());
    }
    let input: ReviewInput =
        serde_json::from_value(args).map_err(|_| "WEB_REVIEW_ARGUMENTS_INVALID")?;
    let request = WorkRequest::parse(input.request)?;
    if request.project_id != input.project_id {
        return Err("WEB_REVIEW_PROJECT_SCOPE_MISMATCH".into());
    }
    let message = request.message()?;
    let identity = ReviewMessageIdentity {
        request_id: request.request_id.clone(),
        event_id: request.event_id(),
        message_sha256: protocol::digest(&message),
        review_key: input.review_key,
    };
    let lease = crate::operator_facade::acquire_web_work_lease(&workspace, &input.project_id)?;
    let expected = target(&workspace, &input.project_id)?;
    if request.expected_target_generation != expected.generation
        || request.expected_target_sha256 != expected.digest
    {
        return Err("WEB_REVIEW_TARGET_CHANGED".into());
    }
    lease.check_target(&expected)?;
    wake_protocol_client::preflight_registered_invitation(
        &workspace,
        &input.project_id,
        &expected,
    )?;
    let store = Store::open(&wake_protocol_client::wake_root_for_workspace(&workspace)?)?;
    require_stopped(&store)?;
    if store
        .timers_for_project(&input.project_id)?
        .iter()
        .any(|timer| timer.response_state != catdesk_wake::store::ResponseState::Complete)
    {
        return Err("WEB_REVIEW_PROJECT_BUSY".into());
    }
    // A stable request can never acquire another send by changing its grant.
    let resume = operation == "resume";
    if store.delivery(&identity.event_id)?.is_some_and(|delivery| {
        !resume
            || delivery.phase != catdesk_wake::store::Phase::Claimed
            || delivery.target != expected
            || delivery.event.project_id != input.project_id
            || delivery.event.event_type != "source_review"
            || delivery.message_digest != identity.message_sha256
            || delivery.event.message != message
            || delivery.event.audit_references != [format!("catdesk-review-v1:{}", input.grant_id)]
    }) || store.timer(&identity.event_id)?.is_some()
    {
        return Err("WEB_REVIEW_REQUEST_ALREADY_PUBLISHED".into());
    }
    for event in store.events()? {
        if event.event_id == identity.event_id {
            if !resume
                || event.project_id != input.project_id
                || event.event_type != "source_review"
                || event.message != message
                || event.target_generation != expected.generation
                || event.audit_references != [format!("catdesk-review-v1:{}", input.grant_id)]
            {
                return Err("WEB_REVIEW_REQUEST_ALREADY_PUBLISHED".into());
            }
        } else if event.project_id == input.project_id
            && protocol::digest(&event.message) == identity.message_sha256
        {
            return Err("WEB_REVIEW_REQUEST_ALREADY_PUBLISHED".into());
        }
    }
    let permit: ReviewDispatchPermit =
        with_project(&workspace, &input.project_id, &expected, |ledger| {
            let permit = if resume {
                ledger.resume_review(&input.grant_id, &identity, &expected, now())?
            } else {
                ledger.preflight_review(&input.grant_id, &identity, &expected, now())?
            };
            if operation != "preflight" {
                if !resume {
                    ledger.reserve_review(&input.grant_id, &identity, &expected, now())?;
                }
                ledger.prepare(
                    &identity.request_id,
                    &identity.event_id,
                    &identity.message_sha256,
                    &expected,
                )?;
                wake_protocol_client::publish_registered_scoped_review(
                    &workspace,
                    &input.project_id,
                    &identity.event_id,
                    &message,
                    &expected,
                    &input.grant_id,
                )?;
            }
            Ok(permit)
        })?;
    if operation == "preflight" {
        return Ok(json!({"status":"ELIGIBLE","projectId":input.project_id,
            "permit":permit,"browserStarted":false,"sendAttempted":false,
            "platformApprovalGranted":false}));
    }
    let check_pair = || {
        if target(&workspace, &input.project_id)? != expected {
            return Err("WEB_REVIEW_TARGET_CHANGED".into());
        }
        lease.check_target(&expected)
    };
    let mut before_submit = |event: &protocol::Event, observed: &protocol::Target| {
        if event.project_id != input.project_id
            || event.event_id != identity.event_id
            || protocol::digest(&event.message) != identity.message_sha256
            || observed != &expected
            || event.audit_references != [format!("catdesk-review-v1:{}", input.grant_id)]
        {
            return Err("WEB_REVIEW_DISPATCH_SCOPE_CHANGED".into());
        }
        with_project(&workspace, &input.project_id, &expected, |ledger| {
            lease.check_target(&expected)?;
            if ledger.check_review(&input.grant_id, &identity, &expected, now())? != permit {
                return Err("WEB_REVIEW_GRANT_SCOPE_CHANGED".into());
            }
            ledger.mark_review_dispatch_started(&permit, now())
        })
    };
    let outcome = catdesk_wake::runtime::run_scoped_review(
        &store,
        &identity.event_id,
        permit.max_execution_seconds,
        &mut before_submit,
        &check_pair,
    );
    // Never retry or refund on an uncertain result. The existing Wake receipt
    // and timer remain the evidence for read-only observation/reconciliation.
    check_pair()?;
    outcome.map(|result| {
        json!({"action":"web_review_run","projectId":input.project_id,
        "requestId":identity.request_id,"eventId":identity.event_id,
        "grantId":permit.grant_id,"result":result,"nativeSessionAction":"NONE",
        "productionAction":"NONE","platformApprovalGranted":false})
    })
}
