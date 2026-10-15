//! Read-only fixed broker/namespace diagnostic. This binary has no install,
//! service-control, or caller-selected path operation.

fn main() {
    if std::env::args_os().len() != 1 {
        println!("{}", serde_json::json!({"error":"CLI_ARGUMENTS_INVALID"}));
        std::process::exit(2);
    }
    match catdesk_wake::install_broker_preflight::inspect_fixed_broker_read_only() {
        Ok(snapshot) => println!(
            "{}",
            serde_json::to_string(&snapshot).expect("fixed bounded snapshot serializes")
        ),
        Err(error) => {
            println!("{}", serde_json::json!({"error":error}));
            std::process::exit(1);
        }
    }
}
