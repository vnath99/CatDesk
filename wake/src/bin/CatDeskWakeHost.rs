use catdesk_wake::{Result, runtime, store::Store};
use std::path::Path;

fn open_runtime_store(root: &Path) -> Result<Store> {
    #[cfg(feature = "test-support")]
    if let Some(parent) = std::env::var_os("CATDESK_WAKE_TEST_ONLY_ROOT_PARENT") {
        return Store::open_scoped_for_test(root, Path::new(&parent));
    }
    Store::open(root)
}

fn main() {
    if let Err(error) = execute() {
        println!("{}", serde_json::json!({"error": error}));
        std::process::exit(1);
    }
}
fn execute() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = runtime::default_root()?;
    let store = open_runtime_store(&root)?;
    match args.as_slice() {
        [command] if command == "init" => store.initialize()?,
        [command, workspace] if command == "set-source" => {
            runtime::set_review_source(&store, workspace)?
        }
        [command] if command == "register-install" => runtime::register_install(&store)?,
        [command] if command == "reviewed-install-status" => {
            println!(
                "{}",
                serde_json::to_string(&runtime::reviewed_install_status(&store)?)
                    .map_err(|_| "STATUS_SERIALIZATION_FAILED")?
            );
            return Ok(());
        }
        [command] if command == "activate-reviewed-install" => {
            println!(
                "{}",
                serde_json::to_string(&runtime::activate_reviewed_install(&store)?)
                    .map_err(|_| "STATUS_SERIALIZATION_FAILED")?
            );
            return Ok(());
        }
        [command, staging, expected] if command == "publish-reviewed-install" => {
            println!(
                "{}",
                serde_json::to_string(&runtime::publish_reviewed_install(
                    &store, staging, expected,
                )?)
                .map_err(|_| "STATUS_SERIALIZATION_FAILED")?
            );
            return Ok(());
        }
        [command, id] if command == "retry-pre-submit" => store.retry_pre_submit(id)?,
        [command, id] if command == "event-status" => {
            println!(
                "{}",
                serde_json::json!({
                    "delivery": store.delivery(id)?,
                    "timer": store.timer(id)?,
                })
            );
            return Ok(());
        }
        [command] if command == "readiness-history" => {
            println!(
                "{}",
                serde_json::to_string(&runtime::readiness_history(&store)?)
                    .map_err(|_| "STATUS_SERIALIZATION_FAILED")?
            );
            return Ok(());
        }
        [command] if command == "queue-health" => {
            println!(
                "{}",
                serde_json::to_string(&runtime::queue_health(&store)?)
                    .map_err(|_| "STATUS_SERIALIZATION_FAILED")?
            );
            return Ok(());
        }
        [command, id, generation, digest] if command == "test-event" => {
            let config = store.config()?;
            let mut expected = config
                .targets
                .get("catdesk")
                .cloned()
                .ok_or("TARGET_NOT_CONFIGURED")?;
            expected.generation = generation
                .parse()
                .map_err(|_| "CONFIG_GENERATION_INVALID")?;
            expected.digest = digest.clone();
            let message = format!(
                "MANUAL WAKE DEBUG — CatDesk diagnostic test-event {} — NOT natural acceptance.",
                id
            );
            let event = store.produce_expected(id, "catdesk", "test", &message, &expected)?;
            runtime::start_installed(&store)?;
            println!(
                "{}",
                serde_json::to_string(&event).map_err(|_| "EVENT_SERIALIZATION_FAILED")?
            );
            return Ok(());
        }
        [command] if command == "--host" => runtime::run_resilient(&store)?,
        [command] if command == "start" => runtime::start_installed(&store)?,
        [command] if command == "pause" => runtime::control(&store, "PAUSED")?,
        [command] if command == "resume" => runtime::control(&store, "RUNNING")?,
        [command] if command == "stop" => runtime::control(&store, "STOPPED")?,
        [command] if command == "status" => {
            println!(
                "{}",
                serde_json::to_string(&runtime::status(&store)?)
                    .map_err(|_| "STATUS_SERIALIZATION_FAILED")?
            );
            return Ok(());
        }
        [command, expected, url] if command == "set-target" => {
            let target = store.set_target(
                "catdesk",
                expected.parse().map_err(|_| "CONFIG_GENERATION_INVALID")?,
                url,
            )?;
            println!(
                "{}",
                serde_json::to_string(&target).map_err(|_| "CONFIG_SERIALIZATION_FAILED")?
            );
            return Ok(());
        }
        [command, url] if command == "validate-target" => {
            catdesk_wake::protocol::validate_target(url)?
        }
        _ => return Err("CLI_ARGUMENTS_INVALID".into()),
    }
    println!("{}", serde_json::json!({"ok": true}));
    Ok(())
}
