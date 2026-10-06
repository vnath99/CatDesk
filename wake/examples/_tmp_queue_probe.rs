use catdesk_wake::{runtime, store::Store};
fn main() {
    let root = runtime::default_root().expect("root");
    let store = Store::open(&root).expect("store");
    println!("CONTROL_ROOT={}", root.display());
    for event in store.events().expect("events") {
        println!(
            "EVENT {} gen={} type={}",
            event.event_id, event.target_generation, event.event_type
        );
        match store.delivery(&event.event_id).expect("delivery") {
            Some(d) => println!(
                "DELIVERY {} phase={:?} reason={:?} receipt={}",
                event.event_id,
                d.phase,
                d.reason,
                d.receipt
                    .as_ref()
                    .map(|r| r.evidence.as_str())
                    .unwrap_or("none")
            ),
            None => println!("DELIVERY {} none", event.event_id),
        }
    }
    for id in [
        "manual-wake-dev57-queued-sequential-003",
        "manual-wake-dev57-queue-sequencing-003",
    ] {
        match store.delivery(id).expect("delivery") {
            Some(d) => println!(
                "EXACT {} phase={:?} reason={:?} receipt={} owner={}",
                id,
                d.phase,
                d.reason,
                d.receipt
                    .as_ref()
                    .map(|r| r.evidence.as_str())
                    .unwrap_or("none"),
                d.owner
            ),
            None => println!("EXACT {} absent", id),
        }
        match store.timer(id).expect("timer") {
            Some(t) => println!(
                "TIMER {} state={:?} started={} completed={:?}",
                id, t.response_state, t.started_utc, t.completed_utc
            ),
            None => println!("TIMER {} absent", id),
        }
    }
}
