use catdesk_wake::{
    Result,
    install_authority::{InstallStoreGuard, require_install_mutation_broker},
    runtime,
    store::Store,
};
use std::path::Path;

#[cfg(windows)]
#[derive(Debug, Eq, PartialEq)]
enum InstallBrokerAction {
    Preflight,
    Submit(std::path::PathBuf),
}

#[cfg(windows)]
fn parse_install_broker_action(args: &[String]) -> Result<Option<InstallBrokerAction>> {
    if args.first().map(String::as_str) != Some("install-broker") {
        return Ok(None);
    }
    match args {
        [_, action] if action == "preflight" => Ok(Some(InstallBrokerAction::Preflight)),
        [_, action, flag, path]
            if action == "submit"
                && flag == "--approval-file"
                && !path.is_empty()
                && !path.chars().any(char::is_control)
                && Path::new(path).is_absolute() =>
        {
            Ok(Some(InstallBrokerAction::Submit(path.into())))
        }
        _ => Err("WAKE_INSTALL_BROKER_ARGUMENTS_INVALID".into()),
    }
}

#[cfg(windows)]
fn install_broker_action(args: &[String]) -> Result<Option<serde_json::Value>> {
    use catdesk_wake::{
        install_broker_client::connect_fixed_broker_for_approval,
        install_broker_policy::load_fixed_broker_policy_read_only,
        install_broker_preflight::inspect_fixed_broker_read_only,
    };
    use std::{io::Read, os::windows::fs::OpenOptionsExt};
    let Some(action) = parse_install_broker_action(args)? else {
        return Ok(None);
    };
    let preflight = inspect_fixed_broker_read_only()?;
    let policy = load_fixed_broker_policy_read_only()?;
    match action {
        InstallBrokerAction::Preflight => Ok(Some(
            serde_json::json!({"action":"install_broker_preflight",
            "broker":preflight,"policy":policy.readback()?,"sameState":policy.same_state_configuration_status()?,
            "stateStoreOpened":false,"serviceStarted":false,"installationAttempted":false}),
        )),
        InstallBrokerAction::Submit(path) => {
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|_| "WAKE_INSTALL_APPROVAL_UNAVAILABLE")?;
            use std::os::windows::fs::MetadataExt;
            if !metadata.is_file()
                || metadata.file_attributes() & 0x400 != 0
                || metadata.len() == 0
                || metadata.len() > 16 * 1024
            {
                return Err("WAKE_INSTALL_APPROVAL_INVALID".into());
            }
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .custom_flags(0x0020_0000)
                .open(path)
                .map_err(|_| "WAKE_INSTALL_APPROVAL_UNAVAILABLE")?;
            let mut bytes = Vec::new();
            (&mut file)
                .take(16 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "WAKE_INSTALL_APPROVAL_UNAVAILABLE")?;
            if bytes.is_empty() || bytes.len() > 16 * 1024 {
                return Err("WAKE_INSTALL_APPROVAL_INVALID".into());
            }
            // Fixed policy/service/pipe attestation precedes the one write.
            // No state-root, service-start, signing or settings argument exists.
            connect_fixed_broker_for_approval(&policy)?
                .submit_same_state_approval_v3(&bytes)
                .map(Some)
        }
    }
}

#[cfg(not(windows))]
fn install_broker_action(args: &[String]) -> Result<Option<serde_json::Value>> {
    if args.first().map(String::as_str) == Some("install-broker") {
        Err("WAKE_BROKER_WINDOWS_REQUIRED".into())
    } else {
        Ok(None)
    }
}

fn open_runtime_store(root: &Path) -> Result<Store> {
    #[cfg(feature = "test-support")]
    if let Some(parent) = std::env::var_os("CATDESK_WAKE_TEST_ONLY_ROOT_PARENT") {
        return Store::open_scoped_for_test(root, Path::new(&parent));
    }
    Store::open(root)
}

fn guarded_install_store(
    root: &Path,
    pid: &str,
    fingerprint: &str,
) -> Result<(InstallStoreGuard, Store)> {
    let pid = pid
        .parse()
        .map_err(|_| "WAKE_STORE_AUTHORITY_PROCESS_INVALID")?;
    let guard = InstallStoreGuard::acquire(root, pid, fingerprint)?;
    guard.require_lock_owner_proof()?;
    // A reviewed host is not an authorization decision for the requesting
    // process. Keep every CLI install mutation closed until a protected broker
    // and independent package approval are deployed.
    require_install_mutation_broker(root)?;
    let store = open_runtime_store(root)?;
    guard.recheck_live()?;
    Ok((guard, store))
}

fn main() {
    if let Err(error) = execute() {
        println!("{}", serde_json::json!({"error": error}));
        std::process::exit(1);
    }
}
fn execute() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Broker operations resolve only the fixed protected policy/service. Do
    // this before default_root/Store::open can initialize an unrelated store.
    if let Some(result) = install_broker_action(&args)? {
        println!("{result}");
        return Ok(());
    }
    if let [command, workspace] = args.as_slice()
        && command == "--dev-foreground-host"
    {
        return catdesk_wake::development_runtime::run_foreground_host(Path::new(workspace));
    }
    if let [command, workspace] = args.as_slice()
        && command == "--dev-host"
    {
        return catdesk_wake::development_runtime::run_host(Path::new(workspace));
    }
    let root = runtime::default_root()?;
    #[cfg(feature = "test-support")]
    if args.as_slice() == ["test-owner-proof-host"] {
        use std::io::Write;
        let store = open_runtime_store(&root)?;
        let root_pin = catdesk_wake::install_authority::HostRootPin::acquire(store.root())?;
        let lease = store.host_lock()?;
        let _proof = catdesk_wake::install_authority::HostOwnerProofServer::start(
            store.root(),
            &root_pin,
            &lease,
        )?;
        println!("OWNER_PROOF_READY");
        std::io::stdout()
            .flush()
            .map_err(|_| "OWNER_PROOF_TEST_READY_FAILED")?;
        std::thread::sleep(std::time::Duration::from_secs(60));
        return Ok(());
    }
    if args.as_slice() == ["store-provenance"] {
        println!(
            "{}",
            serde_json::to_string(&runtime::store_provenance(&root)?)
                .map_err(|_| "STATUS_SERIALIZATION_FAILED")?
        );
        return Ok(());
    }
    // The authority guard is acquired before Store::open, which may create
    // missing state subdirectories. Its root and lock handles stay open until
    // the native operation completes, so pathname retargeting cannot turn a
    // successful preflight into a write to a different store.
    match args.as_slice() {
        [command, pid, fingerprint]
            if command == "install-authority-probe" || command == "install-object-probe" =>
        {
            let pid: u32 = pid
                .parse()
                .map_err(|_| "WAKE_STORE_AUTHORITY_PROCESS_INVALID")?;
            let guard = InstallStoreGuard::acquire_for_probe(&root, pid, fingerprint)?;
            guard.recheck_live()?;
            if command == "install-authority-probe" {
                guard.require_lock_owner_proof()?;
            }
            println!(
                "{}",
                serde_json::json!({
                    "objectMatch": true,
                    "lockOwnerProven": command == "install-authority-probe"
                })
            );
            return Ok(());
        }
        [command, pid, fingerprint] if command == "install-init" => {
            let (guard, store) = guarded_install_store(&root, pid, fingerprint)?;
            store.initialize()?;
            guard.recheck_live()?;
            println!("{}", serde_json::json!({"ok":true}));
            return Ok(());
        }
        [command, workspace, pid, fingerprint] if command == "install-set-source" => {
            let (guard, store) = guarded_install_store(&root, pid, fingerprint)?;
            runtime::set_review_source(&store, workspace)?;
            guard.recheck_live()?;
            println!("{}", serde_json::json!({"ok":true}));
            return Ok(());
        }
        [
            command,
            staging,
            expected,
            host_hash,
            gui_hash,
            adapter_hash,
            bridge_hash,
            pid,
            fingerprint,
        ] if command == "publish-reviewed-install" => {
            let expected_hashes = runtime::ReviewedInstallExpectedHashes::new(
                host_hash,
                gui_hash,
                adapter_hash,
                bridge_hash,
            )?;
            let (guard, store) = guarded_install_store(&root, pid, fingerprint)?;
            let publication = runtime::publish_reviewed_install(
                &store,
                &guard,
                staging,
                expected,
                &expected_hashes,
            )?;
            guard.recheck_live()?;
            println!(
                "{}",
                serde_json::to_string(&publication).map_err(|_| "STATUS_SERIALIZATION_FAILED")?
            );
            return Ok(());
        }
        [
            command,
            host_hash,
            gui_hash,
            adapter_hash,
            bridge_hash,
            pid,
            fingerprint,
        ] if command == "activate-reviewed-install" => {
            let expected_hashes = runtime::ReviewedInstallExpectedHashes::new(
                host_hash,
                gui_hash,
                adapter_hash,
                bridge_hash,
            )?;
            let (guard, store) = guarded_install_store(&root, pid, fingerprint)?;
            let handoff = runtime::activate_reviewed_install(&store, &guard, &expected_hashes)?;
            println!(
                "{}",
                serde_json::to_string(&handoff).map_err(|_| "STATUS_SERIALIZATION_FAILED")?
            );
            return Ok(());
        }
        [command, ..]
            if matches!(
                command.as_str(),
                "install-authority-probe"
                    | "install-object-probe"
                    | "install-init"
                    | "install-set-source"
                    | "publish-reviewed-install"
                    | "activate-reviewed-install"
                    | "register-install"
                    | "init"
                    | "set-source"
            ) =>
        {
            return Err("CLI_ARGUMENTS_INVALID".into());
        }
        _ => {}
    }
    let store = open_runtime_store(&root)?;
    match args.as_slice() {
        [command] if command == "reviewed-install-status" => {
            println!(
                "{}",
                serde_json::to_string(&runtime::reviewed_install_status(&store)?)
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

#[cfg(all(test, windows))]
mod install_broker_cli_tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).into()).collect()
    }
    #[test]
    fn fixed_native_broker_cli_has_no_root_service_or_signing_override() {
        assert_eq!(
            parse_install_broker_action(&args(&["install-broker", "preflight"])).unwrap(),
            Some(InstallBrokerAction::Preflight)
        );
        assert!(matches!(
            parse_install_broker_action(&args(&[
                "install-broker",
                "submit",
                "--approval-file",
                r"C:\frozen.json"
            ]))
            .unwrap(),
            Some(InstallBrokerAction::Submit(_))
        ));
        for values in [
            vec!["install-broker"],
            vec!["install-broker", "preflight", "--start"],
            vec![
                "install-broker",
                "submit",
                "--approval-file",
                "relative.json",
            ],
            vec!["install-broker", "submit", "--root", r"C:\state"],
            vec![
                "install-broker",
                "submit",
                "--approval-file",
                r"C:\frozen.json",
                "--sign",
            ],
        ] {
            assert!(parse_install_broker_action(&args(&values)).is_err());
        }
        assert_eq!(
            parse_install_broker_action(&args(&["status"])).unwrap(),
            None
        );
    }
}
