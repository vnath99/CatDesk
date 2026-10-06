use catdesk_wake::{runtime, store::Store};

fn main() -> Result<(), String> {
    let root = runtime::default_root()?;
    let store = Store::open(&root)?;
    runtime::start_installed(&store)?;
    let status = runtime::status(&store)?;
    println!(
        "VERSION={};HOST={};PID={}",
        status.version, status.host, status.pid
    );
    Ok(())
}
