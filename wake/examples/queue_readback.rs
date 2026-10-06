use catdesk_wake::{
    runtime::{Activation, default_root},
    store::{Store, read},
};

fn main() -> Result<(), String> {
    let root = default_root()?;
    let activation: Activation = read(&root.join("activation.json"))?;
    println!("ACTIVATION|{}", activation.accept_after_utc);
    let store = Store::open(&root)?;
    let config = store.config()?;
    if let Some(target) = config.targets.get("catdesk") {
        println!("TARGET|{}|{}", target.generation, target.url);
    }
    for change in config
        .history
        .iter()
        .filter(|change| change.project_id == "catdesk")
    {
        println!("CHANGE|{}|{}", change.target.generation, change.changed_utc);
    }
    for event in store.events()? {
        let delivery = store.delivery(&event.event_id)?;
        let phase = delivery
            .as_ref()
            .map(|d| format!("{:?}", d.phase))
            .unwrap_or_else(|| "NONE".to_string());
        let reason = delivery
            .as_ref()
            .and_then(|d| d.reason.as_deref())
            .unwrap_or("-");
        println!(
            "EVENT|{}|{}|{}|{}|{}|{}",
            event.event_id,
            event.project_id,
            event.created_utc,
            event.target_generation,
            phase,
            reason
        );
    }
    Ok(())
}
