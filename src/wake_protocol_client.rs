//! CatDesk publishes bounded events; independent Wake owns delivery and target.
use catdesk_wake::{
    development_runtime,
    protocol::{Event, Target},
    runtime::{Activation, default_root},
    store::{Config, Store, read},
};
use std::path::{Path, PathBuf};

/// Select the explicitly prepared development Wake for this control workspace.
/// A present but invalid binding is an error, never permission to use the
/// installed Wake root instead.
pub(crate) fn wake_root_for_workspace(workspace: &Path) -> Result<PathBuf, String> {
    match development_runtime::root_for_workspace(workspace)? {
        Some(root) => Ok(root),
        None => default_root(),
    }
}

pub(crate) fn wake_runtime_mode_for_workspace(workspace: &Path) -> Result<&'static str, String> {
    Ok(
        if development_runtime::root_for_workspace(workspace)?.is_some() {
            "DEVELOPMENT_UNPROTECTED"
        } else {
            "INSTALLED_STORE"
        },
    )
}

/// The explicit project selector also makes earlier CatDesk generations fail
/// closed (unknown owner), including a rollback to a pre-protocol executable.
pub(crate) fn selected(workspace: &Path) -> bool {
    let value = read::<serde_json::Value>(&workspace.join(".catdesk/wake-bridge/owner.json"));
    value.ok().is_some_and(|v| {
        v.get("schemaVersion").and_then(|x| x.as_u64()) == Some(1)
            && v.get("owner").and_then(|x| x.as_str()) == Some("independent_v1")
    })
}

fn generation_accept_after(
    activation: &Activation,
    config: &Config,
    project: &str,
) -> Result<u64, String> {
    let target = config
        .targets
        .get(project)
        .ok_or_else(|| "TARGET_NOT_CONFIGURED".to_string())?;
    let changed_utc = config
        .history
        .iter()
        .rev()
        .find(|change| change.project_id == project && change.target == *target)
        .map(|change| change.changed_utc)
        .ok_or_else(|| "WAKE_TARGET_HISTORY_INVALID".to_string())?;
    Ok(activation.accept_after_utc.max(changed_utc))
}

/// Return the exact historical-event cutoff for the currently configured
/// project target. Startup rehydration uses this before scheduling work so
/// reviews from an older target generation never create dispatch tasks.
pub(crate) fn current_generation_accept_after_at(
    root: &Path,
    project: &str,
) -> Result<u64, String> {
    let activation: Activation = read(&root.join("activation.json"))?;
    if activation.schema_version != 1
        || activation.owner != "CatDeskWake"
        || activation.accept_after_utc == 0
    {
        return Err("WAKE_ACTIVATION_INVALID".into());
    }
    let store = Store::open(root)?;
    let config = store.config()?;
    generation_accept_after(&activation, &config, project)
}

pub(crate) fn current_generation_accept_after(
    control_workspace: &Path,
    project: &str,
) -> Result<u64, String> {
    let root = wake_root_for_workspace(control_workspace)?;
    current_generation_accept_after_at(&root, project)
}

#[cfg(test)]
pub(crate) fn current_generation_accept_after_at_for_test(
    root: &Path,
    trusted_parent: &Path,
    project: &str,
) -> Result<u64, String> {
    let activation: Activation = read(&root.join("activation.json"))?;
    if activation.schema_version != 1
        || activation.owner != "CatDeskWake"
        || activation.accept_after_utc == 0
    {
        return Err("WAKE_ACTIVATION_INVALID".into());
    }
    let store = Store::open_scoped_for_test(root, trusted_parent)?;
    let config = store.config()?;
    generation_accept_after(&activation, &config, project)
}

pub(crate) fn publish(
    control_workspace: &Path,
    project: &str,
    id: &str,
    review_created: u64,
    review_ready: bool,
) -> Result<(), String> {
    publish_registered(control_workspace, project, id, review_created, review_ready)
}

/// Publish only after a fresh central registration and independent Wake target
/// agree. The exact observed target is carried into Store publication, so a
/// concurrent rollover cannot silently retarget this review event.
pub(crate) fn publish_registered(
    control_workspace: &Path,
    project: &str,
    id: &str,
    review_created: u64,
    review_ready: bool,
) -> Result<(), String> {
    if !selected(control_workspace) {
        return Err("WAKE_OWNER_NOT_SELECTED".into());
    }
    let scope = crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
        control_workspace,
        project,
    )
    .map_err(|_| "WAKE_PROJECT_NOT_REGISTERED".to_string())?;
    let root = wake_root_for_workspace(control_workspace)?;
    let activation: Activation = read(&root.join("activation.json"))?;
    if activation.schema_version != 1
        || activation.owner != "CatDeskWake"
        || activation.accept_after_utc == 0
    {
        return Err("WAKE_ACTIVATION_INVALID".into());
    }
    let store = Store::open(&root)?;
    if let Some(url) = scope.project.chatgpt_target_url.as_deref() {
        development_runtime::validate_selected_target_isolation(
            control_workspace,
            store.root(),
            url,
        )?;
    }
    publish_registered_with_store(
        &store,
        &activation,
        project,
        scope.project.chatgpt_target_url.as_deref(),
        scope.project.chatgpt_target_sha256.as_deref(),
        id,
        review_created,
        review_ready,
    )
}

/// The queue publisher saves this exact target before its first Wake write.
/// It may then retry only against the same generation after an uncertain exit.
pub(crate) fn registered_invitation_target(
    control_workspace: &Path,
    project: &str,
    prepared_at: u64,
) -> Result<Target, String> {
    let (store, activation, registered_url, registered_sha256) =
        registered_publication_context(control_workspace, project)?;
    publication_target(
        &store,
        &activation,
        project,
        &registered_url,
        &registered_sha256,
        prepared_at,
    )
}

/// Publish only the fixed, previously persisted queue invitation text. The
/// caller holds its queue and registration locks and supplies its saved target.
pub(crate) fn publish_registered_invitation(
    control_workspace: &Path,
    project: &str,
    event_id: &str,
    prepared_at: u64,
    message: &str,
    expected: &Target,
) -> Result<Event, String> {
    let (store, activation, registered_url, registered_sha256) =
        registered_publication_context(control_workspace, project)?;
    publish_registered_invitation_with_store(
        &store,
        &activation,
        project,
        &registered_url,
        &registered_sha256,
        event_id,
        prepared_at,
        message,
        expected,
    )
}

fn registered_publication_context(
    control_workspace: &Path,
    project: &str,
) -> Result<(Store, Activation, String, String), String> {
    if !selected(control_workspace) {
        return Err("WAKE_OWNER_NOT_SELECTED".into());
    }
    let scope = crate::delegated::autonomy_projects::resolve_registered_project_from_control_root(
        control_workspace,
        project,
    )
    .map_err(|_| "WAKE_PROJECT_NOT_REGISTERED".to_string())?;
    let registered_url = scope
        .project
        .chatgpt_target_url
        .ok_or("TARGET_NOT_CONFIGURED")?;
    let registered_sha256 = scope
        .project
        .chatgpt_target_sha256
        .ok_or("TARGET_NOT_CONFIGURED")?;
    let root = wake_root_for_workspace(control_workspace)?;
    let activation: Activation = read(&root.join("activation.json"))?;
    let store = Store::open(&root)?;
    development_runtime::validate_selected_target_isolation(
        control_workspace,
        store.root(),
        &registered_url,
    )?;
    Ok((store, activation, registered_url, registered_sha256))
}

/// Apply the same activation, target and history admission used by publication
/// before a scoped review consumes its one-shot reservation. No event is sent.
pub(crate) fn preflight_registered_invitation(
    control_workspace: &Path,
    project: &str,
    expected: &Target,
) -> Result<(), String> {
    let (store, activation, url, digest) =
        registered_publication_context(control_workspace, project)?;
    if publication_target(
        &store,
        &activation,
        project,
        &url,
        &digest,
        catdesk_wake::store::now(),
    )? != *expected
    {
        return Err("WAKE_PROJECT_TARGET_CHANGED".into());
    }
    Ok(())
}

pub(crate) fn publish_registered_scoped_review(
    control_workspace: &Path,
    project: &str,
    event_id: &str,
    message: &str,
    expected: &Target,
    grant_id: &str,
) -> Result<Event, String> {
    preflight_registered_invitation(control_workspace, project, expected)?;
    let store = Store::open(&wake_root_for_workspace(control_workspace)?)?;
    store.produce_expected_with_audit(
        event_id,
        project,
        "source_review",
        message,
        expected,
        &[format!("catdesk-review-v1:{grant_id}")],
    )
}

fn publication_target(
    store: &Store,
    activation: &Activation,
    project: &str,
    registered_url: &str,
    registered_sha256: &str,
    prepared_at: u64,
) -> Result<Target, String> {
    if activation.schema_version != 1
        || activation.owner != "CatDeskWake"
        || activation.accept_after_utc == 0
    {
        return Err("WAKE_ACTIVATION_INVALID".into());
    }
    let config = store.config()?;
    let target = config.targets.get(project).ok_or("TARGET_NOT_CONFIGURED")?;
    if registered_url != target.url
        || registered_sha256 != target.digest
        || crate::delegated::autonomy_projects::canonical_project_chat_target(&target.url)
            .ok()
            .as_deref()
            != Some(target.url.as_str())
    {
        return Err("WAKE_PROJECT_TARGET_MISMATCH".into());
    }
    // A queue invitation prepared in the exact second as a target change has
    // ambiguous order. Hold it rather than delivering it in a later lineage.
    if prepared_at <= generation_accept_after(activation, &config, project)? {
        return Err("WAKE_HISTORICAL_EVENT_RETAINED".into());
    }
    Ok(target.clone())
}

#[allow(clippy::too_many_arguments)]
fn publish_registered_invitation_with_store(
    store: &Store,
    activation: &Activation,
    project: &str,
    registered_url: &str,
    registered_sha256: &str,
    event_id: &str,
    prepared_at: u64,
    message: &str,
    expected: &Target,
) -> Result<Event, String> {
    let current = publication_target(
        store,
        activation,
        project,
        registered_url,
        registered_sha256,
        prepared_at,
    )?;
    if &current != expected {
        return Err("WAKE_PROJECT_TARGET_CHANGED".into());
    }
    store.produce_expected(
        event_id,
        project,
        "engineering_attention",
        message,
        expected,
    )
}

fn publish_registered_with_store(
    store: &Store,
    activation: &Activation,
    project: &str,
    registered_url: Option<&str>,
    registered_sha256: Option<&str>,
    id: &str,
    review_created: u64,
    review_ready: bool,
) -> Result<(), String> {
    if activation.schema_version != 1
        || activation.owner != "CatDeskWake"
        || activation.accept_after_utc == 0
    {
        return Err("WAKE_ACTIVATION_INVALID".into());
    }
    let config = store.config()?;
    let target = config.targets.get(project).ok_or("TARGET_NOT_CONFIGURED")?;
    if registered_url != Some(target.url.as_str())
        || registered_sha256 != Some(target.digest.as_str())
        || crate::delegated::autonomy_projects::canonical_project_chat_target(&target.url)
            .ok()
            .as_deref()
            != Some(target.url.as_str())
    {
        return Err("WAKE_PROJECT_TARGET_MISMATCH".into());
    }
    if review_created < generation_accept_after(activation, &config, project)? {
        return Err("WAKE_HISTORICAL_EVENT_RETAINED".into());
    }
    let purpose = if review_ready {
        "review_ready"
    } else {
        "engineering_attention"
    };
    store.produce_review_expected(id, project, purpose, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use catdesk_wake::{protocol::Target, store::TargetChange};
    use std::collections::BTreeMap;

    #[test]
    fn wake_root_uses_installed_default_only_when_development_binding_is_absent() {
        let workspace =
            std::env::temp_dir().join(format!("catdesk-wake-root-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&workspace).unwrap();
        assert_eq!(wake_root_for_workspace(&workspace), default_root());
        assert_eq!(
            wake_runtime_mode_for_workspace(&workspace),
            Ok("INSTALLED_STORE")
        );

        let authority = workspace.join(".catdesk/web-development/authority.json");
        std::fs::create_dir_all(authority.parent().unwrap()).unwrap();
        std::fs::write(authority, b"{}").unwrap();
        assert!(wake_root_for_workspace(&workspace).is_err());
        assert!(wake_runtime_mode_for_workspace(&workspace).is_err());
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn project_publication_rechecks_target_and_deduplicates_exact_event() {
        let root =
            std::env::temp_dir().join(format!("catdesk-project-publish-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("fixture root");
        let wake_root = root.join("wake");
        let store = Store::open_scoped_for_test(&wake_root, &root).expect("wake store");
        store.initialize().expect("wake initialize");
        let first = store
            .set_target("alpha", 0, "https://chatgpt.com/g/project-a/c/chat-a")
            .expect("alpha target");
        let second = store
            .set_target("beta", 0, "https://chatgpt.com/g/project-b/c/chat-b")
            .expect("beta target");
        let activation = Activation {
            schema_version: 1,
            owner: "CatDeskWake".into(),
            migration_audit: "migration-v1.json".into(),
            accept_after_utc: 1,
        };
        let created = u64::MAX - 1;
        publish_registered_with_store(
            &store,
            &activation,
            "alpha",
            Some(&first.url),
            Some(&first.digest),
            "same-record",
            created,
            true,
        )
        .expect("alpha publish");
        publish_registered_with_store(
            &store,
            &activation,
            "alpha",
            Some(&first.url),
            Some(&first.digest),
            "same-record",
            created,
            true,
        )
        .expect("alpha retry is idempotent");
        assert_eq!(store.events().expect("events").len(), 1);
        assert_eq!(
            publish_registered_with_store(
                &store,
                &activation,
                "beta",
                Some(&second.url),
                Some(&second.digest),
                "same-record",
                created,
                true,
            ),
            Err("QUEUE_EVENT_ID_CONFLICT".into())
        );
        assert_eq!(
            publish_registered_with_store(
                &store,
                &activation,
                "beta",
                Some(&first.url),
                Some(&first.digest),
                "beta-record",
                created,
                true,
            ),
            Err("WAKE_PROJECT_TARGET_MISMATCH".into())
        );
        publish_registered_with_store(
            &store,
            &activation,
            "beta",
            Some(&second.url),
            Some(&second.digest),
            "beta-record",
            created,
            false,
        )
        .expect("beta progresses independently");
        assert_eq!(store.events().expect("events").len(), 2);
        let catdesk = store
            .set_target("catdesk", 0, "https://chatgpt.com/c/catdesk-review")
            .unwrap();
        publish_registered_with_store(
            &store,
            &activation,
            "catdesk",
            Some(&catdesk.url),
            Some(&catdesk.digest),
            "catdesk-review-record",
            created,
            true,
        )
        .unwrap();
        let events = store.events().unwrap();
        let review = events
            .iter()
            .find(|event| event.project_id == "catdesk")
            .unwrap();
        assert!(
            review
                .message
                .contains("for project catdesk review record catdesk-review-record")
        );
        assert!(
            review
                .message
                .contains("only this registered project's pending review")
        );
        assert!(!review.message.contains("autonomy remains frozen"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn invitation_publication_reuses_exact_event_and_refuses_retargeting() {
        let root = std::env::temp_dir().join(format!(
            "catdesk-invitation-publish-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::open_scoped_for_test(&root.join("wake"), &root).unwrap();
        store.initialize().unwrap();
        let alpha = store
            .set_target("alpha", 0, "https://chatgpt.com/c/alpha-review")
            .unwrap();
        let beta = store
            .set_target("beta", 0, "https://chatgpt.com/c/beta-review")
            .unwrap();
        let activation = Activation {
            schema_version: 1,
            owner: "CatDeskWake".into(),
            migration_audit: "migration-v1.json".into(),
            accept_after_utc: 1,
        };
        let message = "Review the exact prepared project package; no approval is implied.";
        let prepared = u64::MAX - 1;
        let first = publish_registered_invitation_with_store(
            &store,
            &activation,
            "alpha",
            &alpha.url,
            &alpha.digest,
            "queue-review-exact",
            prepared,
            message,
            &alpha,
        )
        .unwrap();
        let replay = publish_registered_invitation_with_store(
            &store,
            &activation,
            "alpha",
            &alpha.url,
            &alpha.digest,
            "queue-review-exact",
            prepared,
            message,
            &alpha,
        )
        .unwrap();
        assert_eq!(first, replay);
        assert_eq!(store.events().unwrap().len(), 1);
        assert_eq!(
            publish_registered_invitation_with_store(
                &store,
                &activation,
                "beta",
                &beta.url,
                &beta.digest,
                "queue-review-exact",
                prepared,
                message,
                &beta,
            ),
            Err("QUEUE_EVENT_ID_CONFLICT".into())
        );
        assert_eq!(
            publish_registered_invitation_with_store(
                &store,
                &activation,
                "alpha",
                &beta.url,
                &beta.digest,
                "queue-review-other",
                prepared,
                message,
                &alpha,
            ),
            Err("WAKE_PROJECT_TARGET_MISMATCH".into())
        );
        let next = store
            .set_target("alpha", alpha.generation, "https://chatgpt.com/c/alpha-new")
            .unwrap();
        assert_eq!(
            publish_registered_invitation_with_store(
                &store,
                &activation,
                "alpha",
                &next.url,
                &next.digest,
                "queue-review-exact",
                prepared,
                message,
                &alpha,
            ),
            Err("WAKE_PROJECT_TARGET_CHANGED".into())
        );
        assert_eq!(store.events().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    fn config_with_current_target(changed_utc: u64) -> Config {
        let target =
            Target::new(9, "https://chatgpt.com/c/current-chat".to_string()).expect("target");
        Config {
            schema_version: 1,
            targets: BTreeMap::from([("catdesk".to_string(), target.clone())]),
            retired_targets: BTreeMap::new(),
            history: vec![TargetChange {
                project_id: "catdesk".to_string(),
                target,
                changed_utc,
            }],
        }
    }

    #[test]
    fn current_target_generation_change_is_the_replay_cutoff() {
        let activation = Activation {
            schema_version: 1,
            owner: "CatDeskWake".into(),
            migration_audit: "migration-v1.json".into(),
            accept_after_utc: 10,
        };
        assert_eq!(
            100,
            generation_accept_after(&activation, &config_with_current_target(100), "catdesk",)
                .expect("cutoff"),
        );
    }

    #[test]
    fn later_activation_cutoff_remains_authoritative() {
        let activation = Activation {
            schema_version: 1,
            owner: "CatDeskWake".into(),
            migration_audit: "migration-v1.json".into(),
            accept_after_utc: 200,
        };
        assert_eq!(
            200,
            generation_accept_after(&activation, &config_with_current_target(100), "catdesk",)
                .expect("cutoff"),
        );
    }

    #[test]
    fn missing_current_generation_history_fails_closed() {
        let activation = Activation {
            schema_version: 1,
            owner: "CatDeskWake".into(),
            migration_audit: "migration-v1.json".into(),
            accept_after_utc: 10,
        };
        let target =
            Target::new(9, "https://chatgpt.com/c/current-chat".to_string()).expect("target");
        let config = Config {
            schema_version: 1,
            targets: BTreeMap::from([("catdesk".to_string(), target)]),
            retired_targets: BTreeMap::new(),
            history: Vec::new(),
        };
        assert_eq!(
            Err("WAKE_TARGET_HISTORY_INVALID".to_string()),
            generation_accept_after(&activation, &config, "catdesk"),
        );
    }
}
