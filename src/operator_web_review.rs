//! Explicit no-send reconciliation of an already delivered Web review.
use super::*;
use crate::delegated::autonomy_projects::resolve_registered_project_from_control_root;

pub(super) fn reobserve(workspace: &Path, project: &str, event: &str) -> Result<Value, String> {
    let lease = crate::operator_facade::acquire_web_work_lease(workspace, project)?;
    let key = paired_key(workspace, project)?;
    let target =
        catdesk_wake::protocol::Target::new(key.target_generation, key.conversation_url.clone())?;
    lease.check_target(&target)?;
    let root = crate::wake_protocol_client::wake_root_for_workspace(workspace)?;
    let store = catdesk_wake::store::Store::open(&root)?;
    catdesk_wake::development_runtime::validate_selected_target_isolation(
        workspace,
        store.root(),
        &target.url,
    )?;
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .map_err(|_| "WEB_REVIEW_REGISTRY_UNAVAILABLE")?;
    let check_pair = || {
        if paired_key(workspace, project)? != key {
            return Err("WEB_REVIEW_TARGET_CHANGED".into());
        }
        lease.check_target(&target)
    };
    let result = catdesk_wake::runtime::reobserve_sent_response_guarded(
        &store,
        project,
        &target,
        event,
        &check_pair,
        |commit| {
            registry.with_registration_lock(|| {
                // Binding takes this registration lock before its Wake CAS. Do not
                // reacquire the global paired-target mutex while holding it.
                let scope = resolve_registered_project_from_control_root(workspace, project)
                    .map_err(|_| "WEB_REVIEW_PROJECT_UNAVAILABLE")?;
                if scope.project.chatgpt_target_url.as_deref() != Some(target.url.as_str())
                    || scope.project.chatgpt_target_sha256.as_deref()
                        != Some(target.digest.as_str())
                {
                    return Err("WEB_REVIEW_TARGET_CHANGED".into());
                }
                lease.check_target(&target)?;
                commit()
            })
        },
    )?;
    check_pair()?;
    Ok(
        json!({"action":"web_review_reobserve","projectId":project,"eventId":event,
        "target":target,"observation":result,"sendAttempted":false,"workResolved":false}),
    )
}
