use catdesk_wake::{runtime, store::Store};
use std::thread;
use std::time::Duration;

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    runtime::control(&store, "STOPPED")?;
    for _ in 0..100 {
        let status = runtime::status(&store)?;
        if status.host == "STOPPED" && store.host_lock().is_ok() {
            println!("STOPPED");
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("HOST_STOP_TIMEOUT".into())
}
