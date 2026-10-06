use catdesk_wake::{runtime, store::Store};

fn main() -> Result<(), String> {
    let id = std::env::args().nth(1).ok_or("EVENT_ID_REQUIRED")?;
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    store.retire_stale(&id)?;
    println!("RETIRED|{id}");
    Ok(())
}
