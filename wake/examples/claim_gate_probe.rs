use catdesk_wake::{
    runtime,
    store::{Phase, Store},
};

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let config = store.config()?;
    let events = store.events()?;
    let status = runtime::status(&store)?;
    println!(
        "HOST={};PID={};QUEUE={};TARGETS={}",
        status.host,
        status.pid,
        events.len(),
        config.targets.len()
    );
    for event in events {
        let delivery = store.delivery(&event.event_id)?;
        let target_match = config
            .targets
            .get(&event.project_id)
            .is_some_and(|t| t.generation == event.target_generation);
        println!(
            "EVENT={};PROJECT={};GEN={};DELIVERY={};TARGET_MATCH={}",
            event.event_id,
            event.project_id,
            event.target_generation,
            match delivery.as_ref().map(|d| &d.phase) {
                Some(Phase::Claimed) => "CLAIMED",
                Some(Phase::Submitting) => "SUBMITTING",
                Some(Phase::Sent) => "SENT",
                Some(Phase::Attention) => "ATTENTION",
                Some(Phase::Stale) => "STALE",
                None => "NONE",
            },
            target_match
        );
    }
    Ok(())
}
