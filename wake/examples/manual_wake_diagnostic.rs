use catdesk_wake::{runtime, store::Store};

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let event = store.produce(
        "manual-dev13-testchat-20260917-1500",
        "catdesk",
        "test",
        "MANUAL WAKE DEBUG — dev.13 Selenium delivery test — does not count as natural acceptance.",
    )?;
    println!(
        "{}",
        serde_json::to_string(&event).map_err(|_| "SERIALIZATION_FAILED")?
    );
    Ok(())
}
