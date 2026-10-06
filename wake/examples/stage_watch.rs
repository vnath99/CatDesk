use catdesk_wake::{runtime, store::Store};
use std::{
    thread,
    time::{Duration, Instant},
};

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut last = String::new();
    while Instant::now() < deadline {
        let status = runtime::status(&store)?;
        let current = format!(
            "BROWSER={};SUBMISSION={};ATTENTION={}",
            status.browser,
            status.submission,
            status.attention.as_deref().unwrap_or("NONE")
        );
        if current != last {
            println!("{current}");
            last = current;
        }
        thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}
