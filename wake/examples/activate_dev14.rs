use catdesk_wake::{runtime, store::Store};
fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let before = runtime::reviewed_install_status(&store)?;
    println!(
        "BEFORE={}",
        serde_json::to_string(&before).map_err(|_| "SERIALIZE_FAILED".to_string())?
    );
    let handoff = runtime::activate_reviewed_install(&store)?;
    println!(
        "HANDOFF={}",
        serde_json::to_string(&handoff).map_err(|_| "SERIALIZE_FAILED".to_string())?
    );
    runtime::start_installed(&store)?;
    let after = runtime::status(&store)?;
    println!(
        "AFTER={}",
        serde_json::to_string(&after).map_err(|_| "SERIALIZE_FAILED".to_string())?
    );
    Ok(())
}
