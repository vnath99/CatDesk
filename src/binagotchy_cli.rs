//! Console Binagotchy companion for CatDesk.
//!
//! This process mode is deliberately presentation/control only. It reads the
//! same durable autonomy and WakeHost state as the product, mutates the
//! designated ChatGPT conversation only through the existing paired CAS
//! authority, and controls only the independently installed WakeHost. It never
//! constructs `AppState`, binds MCP, owns the secure tunnel, or starts a second
//! CatDesk daemon.

use crate::core_host_acceptance_preflight::read_fixed_core_host_acceptance_preflight;
use crate::delegated::autonomy_observability::read_snapshot as read_autonomy_snapshot;
use crate::mcp::{DesignatedChatTargetErrorV1, DesignatedChatTargetV1};
use crate::state::load_app_config;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub const BINAGOTCHY_CLI_MODE_FLAG: &str = "--catdesk-binagotchy-cli";
static MANUAL_EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn parse_binagotchy_cli_mode(args: &[String]) -> Result<bool, String> {
    let count = args
        .iter()
        .filter(|arg| arg.as_str() == BINAGOTCHY_CLI_MODE_FLAG)
        .count();
    if count > 1 {
        return Err(format!(
            "{BINAGOTCHY_CLI_MODE_FLAG} may only be supplied once"
        ));
    }
    if count == 1 && args.len() != 1 {
        return Err(format!(
            "{BINAGOTCHY_CLI_MODE_FLAG} does not accept additional command-line arguments"
        ));
    }
    Ok(count == 1)
}

#[cfg(windows)]
const CLI_MUTEX_NAME: &str = "Local\\CatDesk.BinagotchyCli.V1";

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(iter::once(0))
        .collect()
}

/// Start the normal Binagotchy companion in a visible, independent console.
/// Recovery and daemon startup can call this repeatedly; an already-running
/// shell is detected before a second console is created.
pub fn launch_associated(workspace: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        type Handle = *mut std::ffi::c_void;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenMutexW(access: u32, inherit: i32, name: *const u16) -> Handle;
            fn CloseHandle(handle: Handle) -> i32;
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn OpenInputDesktop(flags: u32, inherit: i32, access: u32) -> Handle;
            fn CloseDesktop(handle: Handle) -> i32;
        }

        let desktop = unsafe { OpenInputDesktop(0, 0, 1) };
        if desktop.is_null() {
            return;
        }
        unsafe {
            CloseDesktop(desktop);
        }

        let name = wide(CLI_MUTEX_NAME);
        let existing = unsafe { OpenMutexW(0x0010_0000, 0, name.as_ptr()) };
        if !existing.is_null() {
            unsafe {
                CloseHandle(existing);
            }
            return;
        }

        if let Ok(executable) = std::env::current_exe() {
            // CREATE_NEW_CONSOLE. The companion must remain visible even when
            // launched by the hidden canonical daemon/recovery process.
            let _ = std::process::Command::new(executable)
                .arg(BINAGOTCHY_CLI_MODE_FLAG)
                .current_dir(workspace)
                .creation_flags(0x0000_0010)
                .spawn();
        }
    }
    #[cfg(not(windows))]
    let _ = workspace;
}

fn independent_wake_store() -> Result<catdesk_wake::store::Store, String> {
    let store = catdesk_wake::store::Store::open(&catdesk_wake::runtime::default_root()?)?;
    store.initialize()?;
    Ok(store)
}

fn redacted_transport_status() -> String {
    let Ok(config) = load_app_config() else {
        return "CONFIG_UNAVAILABLE".into();
    };
    format!(
        "{} / local MCP observed by CatDesk",
        config.tunnel.mode.as_str()
    )
}

fn read_target(workspace: &Path) -> Result<DesignatedChatTargetV1, DesignatedChatTargetErrorV1> {
    crate::mcp::operator_read_designated_chat_target(workspace)
}

fn validated_wake_predecessor_digest(url: &str, digest: &str) -> Result<String, String> {
    use crate::delegated::autonomy_projects::{
        canonical_project_chat_target, project_chat_target_digest,
    };
    if canonical_project_chat_target(url).ok().as_deref() != Some(url)
        || project_chat_target_digest(url) != digest
    {
        return Err("TARGET_PROTECTED_STATE_MISMATCH".into());
    }
    Ok(digest.to_owned())
}

fn update_target(workspace: &Path, url: &str) -> Result<DesignatedChatTargetV1, String> {
    let expected_sha256 = match read_target(workspace) {
        Ok(current) => current.sha256,
        Err(DesignatedChatTargetErrorV1::ProtectedStateMismatch) => {
            // The guarded paired update can repair an invalid registry digest
            // only when the existing independent Wake target still agrees on
            // the canonical predecessor URL. Obtain the old CAS identity from
            // Wake, not from the corrupt registry. The paired operation will
            // remeasure both authorities under its lock and fail closed on any
            // different kind of divergence before advancing a generation.
            let store = independent_wake_store()?;
            let wake = store
                .config()?
                .targets
                .get("catdesk")
                .cloned()
                .ok_or_else(|| "TARGET_AUTHORITY_UNAVAILABLE".to_string())?;
            validated_wake_predecessor_digest(&wake.url, &wake.digest)?
        }
        Err(error) => return Err(target_error(error).into()),
    };
    crate::mcp::operator_update_designated_chat_target(workspace, url, &expected_sha256)
        .map_err(|error| target_error(error).to_string())
}

const fn target_error(error: DesignatedChatTargetErrorV1) -> &'static str {
    match error {
        DesignatedChatTargetErrorV1::InvalidUrl => "TARGET_INVALID_URL",
        DesignatedChatTargetErrorV1::Stale => "TARGET_STALE",
        DesignatedChatTargetErrorV1::ProtectedStateMismatch => "TARGET_PROTECTED_STATE_MISMATCH",
        DesignatedChatTargetErrorV1::SynchronizationFailure => "TARGET_SYNCHRONIZATION_FAILED",
        DesignatedChatTargetErrorV1::Unavailable => "TARGET_AUTHORITY_UNAVAILABLE",
    }
}

fn normalized_manual_message() -> String {
    "MANUAL WAKE DEBUG - CatDesk diagnostic test - NOT natural acceptance.".to_string()
}

fn manual_event_id() -> String {
    let now = catdesk_wake::store::now();
    let sequence = MANUAL_EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("manual-binagotchy-{now}-{}-{sequence}", std::process::id())
}

fn print_wake_status(out: &mut impl Write) -> Result<(), String> {
    let store = independent_wake_store()?;
    let status = catdesk_wake::runtime::status(&store)?;
    let target = status.targets.get("catdesk");
    writeln!(out, "WakeHost {}", status.version).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  host:       {} (pid {})", status.host, status.pid)
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  target:     {}",
        target.map_or("none", |value| value.url.as_str())
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  generation: {}",
        target.map_or(0, |value| value.generation)
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  queue:      {}", status.queue_depth).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  historical stale: {} (forensic; not current queue health)",
        status.stale_count
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  browser:    {}", status.browser).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  login:      {}", status.login).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  submission: {}", status.submission).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  attention:  {}",
        status.attention.as_deref().unwrap_or("none")
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  host recoveries: {}", status.host_recovery_count)
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    if let Some(error) = status.last_host_error.as_deref() {
        writeln!(out, "  last host error: {error}").map_err(|_| "CLI_OUTPUT_FAILED")?;
        if let Some(observed) = status.last_host_error_utc {
            writeln!(out, "  host error utc: {observed}").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
    }
    if let Some(event_id) = status.last_event_id.as_deref() {
        writeln!(out, "  last event: {event_id}").map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    Ok(())
}

fn print_status(workspace: &Path, out: &mut impl Write) -> Result<(), String> {
    let transport = redacted_transport_status();
    let snapshot = read_autonomy_snapshot(workspace, &transport);
    writeln!(out, "CatDesk Binagotchy").map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  autonomy:   {} / actor {} / provider {}",
        snapshot.overall.label(),
        snapshot.actor.label(),
        snapshot.provider
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  project:    {} / {} / {}",
        snapshot.project_id, snapshot.task_id, snapshot.session_id
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  model:      {} / {}",
        snapshot.model, snapshot.reasoning
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  last step:  {}", snapshot.last_step).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  next:       {}", snapshot.next_action).map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(out, "  review:     {} unread", snapshot.unread_review_count)
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    match read_target(workspace) {
        Ok(target) => {
            writeln!(out, "  target:     {}", target.url).map_err(|_| "CLI_OUTPUT_FAILED")?
        }
        Err(error) => writeln!(out, "  target:     {}", target_error(error))
            .map_err(|_| "CLI_OUTPUT_FAILED")?,
    }
    print_wake_status(out)
}

fn print_acceptance(workspace: &Path, out: &mut impl Write) -> Result<(), String> {
    let presentation = read_fixed_core_host_acceptance_preflight(workspace).presentation();
    writeln!(
        out,
        "Core acceptance: {} ({})",
        presentation.classification, presentation.reason
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "  next live gate: {}",
        presentation.next_live_gate.unwrap_or("NONE")
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    for gate in presentation.gates {
        writeln!(
            out,
            "  {}: {} ({})",
            gate.gate,
            if gate.accepted {
                "ACCEPTED"
            } else {
                "NOT_ACCEPTED"
            },
            gate.reason
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    writeln!(out, "  Display is read-only; it cannot mint acceptance.")
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    Ok(())
}

fn print_queue(out: &mut impl Write) -> Result<(), String> {
    let store = independent_wake_store()?;
    let events = store.events()?;
    if events.is_empty() {
        writeln!(out, "Wake queue is empty.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        return Ok(());
    }
    writeln!(out, "Wake queue ({} event(s)):", events.len()).map_err(|_| "CLI_OUTPUT_FAILED")?;
    for event in events {
        writeln!(
            out,
            "  {}  type={}  generation={}  created={}",
            event.event_id, event.event_type, event.target_generation, event.created_utc
        )
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    Ok(())
}

fn send_manual_test(out: &mut impl Write) -> Result<(), String> {
    let store = independent_wake_store()?;
    let message = normalized_manual_message();
    let event = store.produce(&manual_event_id(), "catdesk", "test", &message)?;
    writeln!(out, "Queued MANUAL wake test: {}", event.event_id)
        .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        out,
        "This explicit test does NOT count as natural automatic-wake acceptance."
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    Ok(())
}

fn print_help(out: &mut impl Write) -> Result<(), String> {
    for line in [
        "Commands:",
        "  status                         CatDesk + independent WakeHost status",
        "  target                         Show authoritative designated ChatGPT URL",
        "  target <url>                   Set designated URL through paired CAS authority",
        "  target set <url>               Same as above",
        "  wake status                    Show WakeHost status",
        "  wake start|pause|resume|stop   Control independent WakeHost",
        "  wake queue                     Show current independent wake queue",
        "  wake send | wake test          Queue a fixed MANUAL diagnostic wake",
        "  wake retire <event-id>         Archive one exact safe pre-submit stale event",
        "  acceptance                     Show read-only core acceptance readiness",
        "  clear                          Clear this console",
        "  help                           Show commands",
        "  exit | quit | q                Close Binagotchy only (WakeHost keeps running)",
    ] {
        writeln!(out, "{line}").map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum ParsedCommand<'a> {
    Status,
    TargetShow,
    TargetSet(&'a str),
    WakeStatus,
    WakeStart,
    WakePause,
    WakeResume,
    WakeStop,
    WakeQueue,
    WakeSend,
    WakeRetire(&'a str),
    Acceptance,
    Help,
    Clear,
    Exit,
    Empty,
    Unknown,
}

fn parse_command(input: &str) -> ParsedCommand<'_> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return ParsedCommand::Empty;
    }
    match trimmed {
        "status" => return ParsedCommand::Status,
        "target" => return ParsedCommand::TargetShow,
        "acceptance" => return ParsedCommand::Acceptance,
        "help" | "?" => return ParsedCommand::Help,
        "clear" | "cls" => return ParsedCommand::Clear,
        "exit" | "quit" | "q" => return ParsedCommand::Exit,
        "wake" | "wake status" => return ParsedCommand::WakeStatus,
        "wake start" => return ParsedCommand::WakeStart,
        "wake pause" => return ParsedCommand::WakePause,
        "wake resume" => return ParsedCommand::WakeResume,
        "wake stop" => return ParsedCommand::WakeStop,
        "wake queue" => return ParsedCommand::WakeQueue,
        _ => {}
    }
    if let Some(url) = trimmed.strip_prefix("target set ").map(str::trim) {
        return if url.is_empty() {
            ParsedCommand::Unknown
        } else {
            ParsedCommand::TargetSet(url)
        };
    }
    if let Some(url) = trimmed.strip_prefix("target ").map(str::trim) {
        return if url.is_empty() {
            ParsedCommand::Unknown
        } else {
            ParsedCommand::TargetSet(url)
        };
    }
    if matches!(trimmed, "wake send" | "wake test") {
        return ParsedCommand::WakeSend;
    }
    if let Some(event_id) = trimmed.strip_prefix("wake retire ").map(str::trim) {
        return if event_id.is_empty() || event_id.split_whitespace().count() != 1 {
            ParsedCommand::Unknown
        } else {
            ParsedCommand::WakeRetire(event_id)
        };
    }
    ParsedCommand::Unknown
}

fn execute_command(
    workspace: &Path,
    command: ParsedCommand<'_>,
    out: &mut impl Write,
) -> Result<bool, String> {
    match command {
        ParsedCommand::Status => print_status(workspace, out)?,
        ParsedCommand::TargetShow => match read_target(workspace) {
            Ok(target) => {
                writeln!(out, "{}", target.url).map_err(|_| "CLI_OUTPUT_FAILED")?;
                writeln!(out, "generation digest: {}", target.sha256)
                    .map_err(|_| "CLI_OUTPUT_FAILED")?;
            }
            Err(error) => return Err(target_error(error).into()),
        },
        ParsedCommand::TargetSet(url) => {
            let target = update_target(workspace, url)?;
            writeln!(out, "Designated Chat URL updated: {}", target.url)
                .map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::WakeStatus => print_wake_status(out)?,
        ParsedCommand::WakeStart => {
            let store = independent_wake_store()?;
            catdesk_wake::runtime::start_installed(&store)?;
            writeln!(out, "WakeHost start requested.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::WakePause => {
            catdesk_wake::runtime::control(&independent_wake_store()?, "PAUSED")?;
            writeln!(out, "WakeHost paused.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::WakeResume => {
            let store = independent_wake_store()?;
            if store.config()?.targets.is_empty() {
                return Err("TARGET_NOT_CONFIGURED".into());
            }
            catdesk_wake::runtime::control(&store, "RUNNING")?;
            catdesk_wake::runtime::start_installed(&store)?;
            writeln!(out, "WakeHost resumed.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::WakeStop => {
            catdesk_wake::runtime::control(&independent_wake_store()?, "STOPPED")?;
            writeln!(out, "WakeHost stop requested.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::WakeQueue => print_queue(out)?,
        ParsedCommand::WakeSend => send_manual_test(out)?,
        ParsedCommand::WakeRetire(event_id) => {
            independent_wake_store()?.retire_stale(event_id)?;
            writeln!(out, "Retired stale wake event: {event_id}")
                .map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::Acceptance => print_acceptance(workspace, out)?,
        ParsedCommand::Help => print_help(out)?,
        ParsedCommand::Clear => {
            write!(out, "\x1b[2J\x1b[H").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
        ParsedCommand::Exit => return Ok(false),
        ParsedCommand::Empty => {}
        ParsedCommand::Unknown => {
            writeln!(out, "Unknown command. Type `help`.").map_err(|_| "CLI_OUTPUT_FAILED")?;
        }
    }
    Ok(true)
}

#[cfg(windows)]
struct CliMutex(*mut std::ffi::c_void);
#[cfg(windows)]
impl Drop for CliMutex {
    fn drop(&mut self) {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        }
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn acquire_cli_mutex() -> Result<Option<CliMutex>, String> {
    type Handle = *mut std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(attributes: *mut std::ffi::c_void, owner: i32, name: *const u16) -> Handle;
        fn GetLastError() -> u32;
        fn SetConsoleTitleW(title: *const u16) -> i32;
    }
    let name = wide(CLI_MUTEX_NAME);
    let mutex = unsafe { CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr()) };
    if mutex.is_null() {
        return Err("BINAGOTCHY_CLI_SINGLE_INSTANCE_UNAVAILABLE".into());
    }
    if unsafe { GetLastError() } == 183 {
        unsafe {
            let title = wide("CatDesk Binagotchy - already running");
            SetConsoleTitleW(title.as_ptr());
        }
        unsafe {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn CloseHandle(handle: Handle) -> i32;
            }
            CloseHandle(mutex);
        }
        return Ok(None);
    }
    unsafe {
        let title = wide("CatDesk Binagotchy");
        SetConsoleTitleW(title.as_ptr());
    }
    Ok(Some(CliMutex(mutex)))
}

pub fn run_cli(workspace: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let _mutex = match acquire_cli_mutex()? {
        Some(mutex) => mutex,
        None => return Ok(()),
    };

    let mut stdout = io::stdout().lock();
    writeln!(stdout, "CatDesk Binagotchy - command companion").map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(
        stdout,
        "Independent WakeHost control; this shell does not start a second CatDesk daemon."
    )
    .map_err(|_| "CLI_OUTPUT_FAILED")?;
    writeln!(stdout, "Type `help` for commands.\n").map_err(|_| "CLI_OUTPUT_FAILED")?;

    if let Ok(store) = independent_wake_store()
        && store
            .config()
            .is_ok_and(|config| !config.targets.is_empty())
    {
        // Opening this presentation shell is not permission to resume an
        // explicitly stopped or paused independent WakeHost.
        let _ = catdesk_wake::runtime::start_installed_only_if_desired_running(&store);
    }
    print_status(workspace, &mut stdout)?;
    writeln!(stdout).map_err(|_| "CLI_OUTPUT_FAILED")?;
    stdout.flush().map_err(|_| "CLI_OUTPUT_FAILED")?;

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        write!(stdout, "binagotchy> ").map_err(|_| "CLI_OUTPUT_FAILED")?;
        stdout.flush().map_err(|_| "CLI_OUTPUT_FAILED")?;
        let Some(line) = lines.next() else {
            break;
        };
        let line = line.map_err(|_| "CLI_INPUT_FAILED")?;
        match execute_command(workspace, parse_command(&line), &mut stdout) {
            Ok(true) => {}
            Ok(false) => break,
            Err(error) => {
                writeln!(stdout, "ERROR: {error}").map_err(|_| "CLI_OUTPUT_FAILED")?;
            }
        }
        stdout.flush().map_err(|_| "CLI_OUTPUT_FAILED")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BINAGOTCHY_CLI_MODE_FLAG, ParsedCommand, normalized_manual_message,
        parse_binagotchy_cli_mode, parse_command, validated_wake_predecessor_digest,
    };

    #[test]
    fn cli_mode_is_closed_and_zero_parameter() {
        assert_eq!(
            parse_binagotchy_cli_mode(&[BINAGOTCHY_CLI_MODE_FLAG.to_string()]),
            Ok(true)
        );
        assert_eq!(parse_binagotchy_cli_mode(&[]), Ok(false));
        assert!(
            parse_binagotchy_cli_mode(&[
                BINAGOTCHY_CLI_MODE_FLAG.to_string(),
                "attacker-input".into()
            ])
            .is_err()
        );
        assert!(
            parse_binagotchy_cli_mode(&[
                BINAGOTCHY_CLI_MODE_FLAG.to_string(),
                BINAGOTCHY_CLI_MODE_FLAG.to_string()
            ])
            .is_err()
        );
    }

    #[test]
    fn command_parser_keeps_mutation_surfaces_explicit() {
        assert_eq!(parse_command("status"), ParsedCommand::Status);
        assert_eq!(parse_command("target"), ParsedCommand::TargetShow);
        assert_eq!(
            parse_command("target set https://chatgpt.com/c/example"),
            ParsedCommand::TargetSet("https://chatgpt.com/c/example")
        );
        assert_eq!(parse_command("wake start"), ParsedCommand::WakeStart);
        assert_eq!(parse_command("wake queue"), ParsedCommand::WakeQueue);
        assert_eq!(parse_command("wake send"), ParsedCommand::WakeSend);
        assert_eq!(
            parse_command("wake send hello world"),
            ParsedCommand::Unknown
        );
        assert_eq!(
            parse_command("wake retire old-event-1"),
            ParsedCommand::WakeRetire("old-event-1")
        );
        assert_eq!(parse_command("rm -rf ."), ParsedCommand::Unknown);
    }

    #[test]
    fn presentation_launch_cannot_implicitly_restart_an_operator_stopped_wakehost() {
        // Limit source assertions to product code. Literals in this test
        // must not accidentally satisfy or invalidate their own assertions.
        let source = include_str!("binagotchy_cli.rs");
        let production = source.split("mod tests {").next().expect("product source");
        assert!(
            production
                .contains("catdesk_wake::runtime::start_installed_only_if_desired_running(&store)")
        );
        assert!(!production.contains("let _ = catdesk_wake::runtime::start_installed(&store);"));
        assert!(production.contains("ParsedCommand::WakeStart"));
        assert!(production.contains("ParsedCommand::WakeResume"));
    }

    #[test]
    fn manual_test_message_is_unambiguously_not_natural_acceptance() {
        let message = normalized_manual_message();
        assert_eq!(
            message,
            "MANUAL WAKE DEBUG - CatDesk diagnostic test - NOT natural acceptance."
        );
        assert!(message.contains("NOT natural acceptance"));
    }

    #[test]
    fn rollover_fallback_only_accepts_canonical_integrity_verified_wake_identity() {
        let old = "https://chatgpt.com/c/6ac6cbe8-6f0c-83e9-9f7d-13489d4d87f5";
        let digest = crate::delegated::autonomy_projects::project_chat_target_digest(old);
        assert_eq!(
            validated_wake_predecessor_digest(old, &digest),
            Ok(digest.clone())
        );
        assert!(validated_wake_predecessor_digest(old, &"8".repeat(64)).is_err());
        assert!(
            validated_wake_predecessor_digest(
                "https://example.com/c/6ac6cbe8-6f0c-83e9-9f7d-13489d4d87f5",
                &digest
            )
            .is_err()
        );
    }

    #[test]
    fn cli_source_never_constructs_daemon_or_tunnel_state() {
        let source = include_str!("binagotchy_cli.rs");
        for forbidden in [
            ["AppState", "::new("].concat(),
            ["start", "_services("].concat(),
            ["run_native", "_daemon("].concat(),
            ["ngrok::", "start_transport("].concat(),
            ["openai", "_tunnel"].concat(),
        ] {
            assert!(
                !source.contains(&forbidden),
                "unexpected ownership: {forbidden}"
            );
        }
    }
}
