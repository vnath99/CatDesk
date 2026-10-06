use catdesk_wake::{runtime, store::Store};

fn main() -> Result<(), String> {
    let id = std::env::args().nth(1).ok_or("EVENT_ID_REQUIRED")?;
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let delivery = store.delivery(&id)?.ok_or("DELIVERY_NOT_FOUND")?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "eventId": delivery.event.event_id,
            "targetGeneration": delivery.event.target_generation,
            "phase": delivery.phase,
            "owner": delivery.owner,
            "updatedUtc": delivery.updated_utc,
            "reason": delivery.reason,
            "receipt": delivery.receipt
        }))
        .map_err(|_| "STATUS_SERIALIZATION_FAILED")?
    );
    Ok(())
}
