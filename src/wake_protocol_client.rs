//! CatDesk publishes bounded events; independent Wake owns delivery and target.
use catdesk_wake::{
    runtime::{Activation, default_root},
    store::{Config, Store, read},
};
use std::path::Path;

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

pub(crate) fn current_generation_accept_after(project: &str) -> Result<u64, String> {
    let root = default_root()?;
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
    project: &str,
    id: &str,
    review_created: u64,
    review_ready: bool,
) -> Result<(), String> {
    if project != "catdesk" {
        return Err("WAKE_PROJECT_NOT_MIGRATED".into());
    }
    let root = default_root()?;
    let activation: Activation = read(&root.join("activation.json"))?;
    if activation.schema_version != 1
        || activation.owner != "CatDeskWake"
        || activation.accept_after_utc == 0
    {
        return Err("WAKE_ACTIVATION_INVALID".into());
    }
    let store = Store::open(&root)?;
    let config = store.config()?;
    if review_created < generation_accept_after(&activation, &config, project)? {
        return Err("WAKE_HISTORICAL_EVENT_RETAINED".into());
    }
    let purpose = if review_ready {
        "review_ready"
    } else {
        "engineering_attention"
    };
    let message = format!(
        "CatDesk Wake: {purpose} for CatDesk review record {id}. Review the exact record and current wake/GUI repair plan through CatDesk. Ordinary autonomy remains frozen until wake acceptance completes."
    );
    store.produce(id, project, purpose, &message)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use catdesk_wake::{protocol::Target, store::TargetChange};
    use std::collections::BTreeMap;

    fn config_with_current_target(changed_utc: u64) -> Config {
        let target =
            Target::new(9, "https://chatgpt.com/c/current-chat".to_string()).expect("target");
        Config {
            schema_version: 1,
            targets: BTreeMap::from([("catdesk".to_string(), target.clone())]),
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
            history: Vec::new(),
        };
        assert_eq!(
            Err("WAKE_TARGET_HISTORY_INVALID".to_string()),
            generation_accept_after(&activation, &config, "catdesk"),
        );
    }
}
