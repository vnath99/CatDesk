//! Argument-free read-only inventory of possible dev.84 Wake owners and roots.
//!
//! A successful snapshot is forensic evidence only. It cannot identify the
//! historical connected store, supply an owner challenge, or authorize an
//! install, even when the process and root candidates look stable.

fn main() {
    if std::env::args_os().len() != 1 {
        println!("{}", serde_json::json!({"error":"CLI_ARGUMENTS_INVALID"}));
        std::process::exit(2);
    }
    let inventory = catdesk_wake::install_broker_legacy_owner::inspect_legacy_owner_candidates();
    println!(
        "{}",
        serde_json::to_string(&inventory).expect("bounded diagnostic snapshot serializes")
    );
}
