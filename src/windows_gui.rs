//! Native Windows Binagotchy operator window.
//!
//! This is deliberately a separate process mode.  It reads the same durable,
//! redacted autonomy snapshot as the terminal UI. Its sole mutation surface
//! is the reviewed, guarded designated-control-chat target transaction; it
//! never constructs an `AppState`, starts a daemon, browser, or owns the
//! Secure MCP transport.

use crate::core_host_acceptance_preflight::{
    CoreHostAcceptancePresentationV1, read_fixed_core_host_acceptance_preflight,
};
use crate::delegated::autonomy_observability::{
    AutonomyObservabilitySnapshotV1, read_snapshot as read_autonomy_snapshot,
};
use crate::mcp::{DesignatedChatTargetErrorV1, DesignatedChatTargetV1};
use crate::state::load_app_config;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const BINAGOTCHY_GUI_MODE_FLAG: &str = "--catdesk-binagotchy-gui";

// The Win32 surface is retained only for explicit legacy/debug invocation.
// Normal CatDesk lifecycle launches use `binagotchy_cli::launch_associated`.
const MAX_GUI_TEXT_CHARS: usize = 2_048;
const MAX_GUI_FIELD_CHARS: usize = 180;
const MAX_GUI_URL_CHARS: usize = 512;
const MAX_CORE_ACCEPTANCE_TEXT_CHARS: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)] // Used by the PowerShell lifecycle equivalent and unit tests.
pub enum GuiLaunchDecision {
    Launch,
    SkipHeadless,
}

/// The GUI mode has a deliberately closed, zero-parameter shape.  The
/// workspace is inherited from the fixed lifecycle working directory rather
/// than caller-provided command-line text.
pub fn parse_binagotchy_gui_mode(args: &[String]) -> Result<bool, String> {
    let count = args
        .iter()
        .filter(|arg| arg.as_str() == BINAGOTCHY_GUI_MODE_FLAG)
        .count();
    if count > 1 {
        return Err(format!(
            "{BINAGOTCHY_GUI_MODE_FLAG} may only be supplied once"
        ));
    }
    if count == 1 && args.len() != 1 {
        return Err(format!(
            "{BINAGOTCHY_GUI_MODE_FLAG} does not accept additional command-line arguments"
        ));
    }
    Ok(count == 1)
}

/// Supported lifecycle code uses this small policy function before launching
/// the separate UI process.  A non-interactive/session-zero host never starts
/// a desktop window.
#[allow(dead_code)] // Kept as the Rust-testable form of the lifecycle policy.
pub const fn lifecycle_gui_decision(interactive_desktop: bool) -> GuiLaunchDecision {
    if interactive_desktop {
        GuiLaunchDecision::Launch
    } else {
        GuiLaunchDecision::SkipHeadless
    }
}

fn bounded(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_GUI_FIELD_CHARS));
    for ch in text.chars() {
        if out.chars().count() >= MAX_GUI_FIELD_CHARS {
            out.push_str("...");
            break;
        }
        out.push(if ch.is_control() { ' ' } else { ch });
    }
    out
}

/// Bounds display text by Unicode scalar values, never by an arbitrary UTF-8
/// byte index. This preserves the GUI cap without panicking when a multibyte
/// character crosses that cap.
fn truncate_display_characters(text: &str, maximum: usize) -> String {
    text.chars().take(maximum).collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MessagePumpDisposition {
    Dispatch,
    Quit,
    Failure,
}

/// Classifies the Win32 `GetMessageW` result without exposing platform error
/// details through the GUI error surface.
#[allow(dead_code)]
fn classify_message_pump_result(result: i32) -> MessagePumpDisposition {
    if result > 0 {
        MessagePumpDisposition::Dispatch
    } else if result == 0 {
        MessagePumpDisposition::Quit
    } else {
        MessagePumpDisposition::Failure
    }
}

fn compact_duration(millis: u128) -> String {
    let seconds = millis / 1_000;
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m {:02}s", seconds % 60);
    }
    format!("{}h {:02}m", minutes / 60, minutes % 60)
}

fn is_loopback_endpoint(host: &str, port: u16) -> Option<SocketAddr> {
    if port == 0 {
        return None;
    }
    let addresses = (host, port).to_socket_addrs().ok()?;
    addresses
        .into_iter()
        .find(|address| address.ip().is_loopback())
}

/// The local probe is display-only and bounded.  It never authenticates to,
/// configures, or restarts the local MCP service.
fn redacted_transport_status() -> String {
    let Ok(config) = load_app_config() else {
        return "TRANSPORT=CONFIG_UNAVAILABLE; LOCAL_MCP=UNKNOWN".into();
    };
    let local = is_loopback_endpoint(&config.mcp.bind_host, config.mcp.port)
        .map(|address| {
            if TcpStream::connect_timeout(&address, Duration::from_millis(125)).is_ok() {
                "READY"
            } else {
                "UNAVAILABLE"
            }
        })
        .unwrap_or("UNAVAILABLE");
    format!(
        "TRANSPORT={}; LOCAL_MCP={local}",
        config.tunnel.mode.as_str()
    )
}

pub fn render_snapshot(snapshot: &AutonomyObservabilitySnapshotV1) -> String {
    let overall = if snapshot.stale {
        format!("{} (STALE)", snapshot.overall.label())
    } else {
        snapshot.overall.label().to_string()
    };
    let lines = [
        "CatDesk Binagotchy — read-only operator view".to_string(),
        format!(
            "Status: {overall}    Actor: {}    Provider: {}",
            snapshot.actor.label(),
            bounded(&snapshot.provider)
        ),
        format!(
            "Project / ticket / session: {} / {} / {} ({})",
            bounded(&snapshot.project_id),
            bounded(&snapshot.task_id),
            bounded(&snapshot.session_id),
            bounded(&snapshot.session_state)
        ),
        format!(
            "Model / reasoning: {} / {}",
            bounded(&snapshot.model),
            bounded(&snapshot.reasoning)
        ),
        format!(
            "Timing: active {} | wall {} | waiting {} | evidence {} | ChatGPT {}",
            compact_duration(snapshot.timing.known_active_millis),
            compact_duration(snapshot.timing.wall_millis),
            compact_duration(snapshot.timing.known_waiting_millis),
            bounded(&snapshot.timing.evidence_completeness),
            bounded(&snapshot.timing.chatgpt_web_status)
        ),
        format!("Last step: {}", bounded(&snapshot.last_step)),
        format!("Next expected action: {}", bounded(&snapshot.next_action)),
        format!(
            "Wake: {}    Pending attention: {} unread ({})",
            bounded(&snapshot.wake),
            snapshot.unread_review_count,
            bounded(&snapshot.review_attention)
        ),
        format!("Transport / local MCP: {}", bounded(&snapshot.transport)),
        "Close or minimize this window without stopping CatDesk.".to_string(),
    ];
    truncate_display_characters(&lines.join("\r\n"), MAX_GUI_TEXT_CHARS)
}

fn read_gui_text(workspace: &Path) -> String {
    let transport = redacted_transport_status();
    render_snapshot(&read_autonomy_snapshot(workspace, &transport))
}

/// Fixed, redacted result vocabulary for the designated-control-chat editor.
/// It intentionally never embeds a filesystem path, browser/profile detail,
/// target URL, digest, or lower-layer error text in a failure display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DesignatedChatUrlStatusV1 {
    Ready,
    Updated,
    Unchanged,
    InvalidUrl,
    StaleTarget,
    ProtectedStateMismatch,
    SynchronizationFailure,
    AuthorityUnavailable,
}

impl DesignatedChatUrlStatusV1 {
    const fn label(self) -> &'static str {
        match self {
            Self::Ready => "DESIGNATED_CHAT_URL_READY",
            Self::Updated => "DESIGNATED_CHAT_URL_UPDATED",
            Self::Unchanged => "DESIGNATED_CHAT_URL_UNCHANGED",
            Self::InvalidUrl => "DESIGNATED_CHAT_URL_INVALID",
            Self::StaleTarget => "DESIGNATED_CHAT_URL_STALE",
            Self::ProtectedStateMismatch => "DESIGNATED_CHAT_URL_PROTECTED_STATE_MISMATCH",
            Self::SynchronizationFailure => "DESIGNATED_CHAT_URL_SYNCHRONIZATION_FAILED",
            Self::AuthorityUnavailable => "DESIGNATED_CHAT_URL_AUTHORITY_UNAVAILABLE",
        }
    }

    const fn from_authority(error: DesignatedChatTargetErrorV1) -> Self {
        match error {
            DesignatedChatTargetErrorV1::InvalidUrl => Self::InvalidUrl,
            DesignatedChatTargetErrorV1::Stale => Self::StaleTarget,
            DesignatedChatTargetErrorV1::ProtectedStateMismatch => Self::ProtectedStateMismatch,
            DesignatedChatTargetErrorV1::SynchronizationFailure => Self::SynchronizationFailure,
            DesignatedChatTargetErrorV1::Unavailable => Self::AuthorityUnavailable,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DesignatedChatUrlStateV1 {
    draft_url: String,
    authoritative_url: Option<String>,
    authoritative_sha256: Option<String>,
    status: DesignatedChatUrlStatusV1,
}

trait DesignatedChatTargetAuthorityV1 {
    fn readback(&self) -> Result<DesignatedChatTargetV1, DesignatedChatTargetErrorV1>;
    fn update(
        &self,
        draft_url: &str,
        expected_current_target_sha256: &str,
    ) -> Result<DesignatedChatTargetV1, DesignatedChatTargetErrorV1>;
}

#[derive(Clone, Debug)]
struct ProductionDesignatedChatTargetAuthorityV1 {
    workspace: PathBuf,
}

impl ProductionDesignatedChatTargetAuthorityV1 {
    fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }
}

impl DesignatedChatTargetAuthorityV1 for ProductionDesignatedChatTargetAuthorityV1 {
    fn readback(&self) -> Result<DesignatedChatTargetV1, DesignatedChatTargetErrorV1> {
        crate::mcp::operator_read_designated_chat_target(&self.workspace)
    }

    fn update(
        &self,
        draft_url: &str,
        expected_current_target_sha256: &str,
    ) -> Result<DesignatedChatTargetV1, DesignatedChatTargetErrorV1> {
        crate::mcp::operator_update_designated_chat_target(
            &self.workspace,
            draft_url,
            expected_current_target_sha256,
        )
    }
}

fn independent_wake_store() -> Result<catdesk_wake::store::Store, String> {
    let store = catdesk_wake::store::Store::open(&catdesk_wake::runtime::default_root()?)?;
    store.initialize()?;
    Ok(store)
}

fn independent_wake_status() -> String {
    match independent_wake_store().and_then(|s| catdesk_wake::runtime::status(&s)) {
        Ok(s) => format!(
            "CatDesk Wake {}\r\nHost: {}\r\nTarget generation: {}\r\nQueue: {} / Stale: {}\r\nBrowser: {}\r\nLogin: {}\r\nSubmission: {}\r\nLast event: {}\r\nLast success UTC: {}\r\nAttention: {}",
            s.version,
            s.host,
            s.targets.get("catdesk").map_or(0, |t| t.generation),
            s.queue_depth,
            s.stale_count,
            s.browser,
            s.login,
            s.submission,
            s.last_event_id.unwrap_or_default(),
            s.last_success_utc
                .map_or_else(|| "none".into(), |t| t.to_string()),
            s.attention.unwrap_or_else(|| "none".into())
        ),
        Err(e) => format!("Wake status: {e}"),
    }
}

/// Separates a freely editable draft from the authoritative URL/digest
/// readback. Editing never touches authority; only `apply` submits the
/// currently displayed digest through the reviewed CAS transaction.
struct DesignatedChatUrlControllerV1<A> {
    authority: A,
    state: DesignatedChatUrlStateV1,
}

impl<A: DesignatedChatTargetAuthorityV1> DesignatedChatUrlControllerV1<A> {
    fn new(authority: A) -> Self {
        let mut controller = Self {
            authority,
            state: DesignatedChatUrlStateV1 {
                draft_url: String::new(),
                authoritative_url: None,
                authoritative_sha256: None,
                status: DesignatedChatUrlStatusV1::AuthorityUnavailable,
            },
        };
        controller.refresh();
        controller
    }

    fn state(&self) -> &DesignatedChatUrlStateV1 {
        &self.state
    }

    fn edit_draft(&mut self, draft_url: String) {
        self.state.draft_url = truncate_display_characters(&draft_url, MAX_GUI_URL_CHARS);
    }

    fn refresh(&mut self) {
        match self.authority.readback() {
            Ok(target) => {
                self.state.draft_url = target.url.clone();
                self.state.authoritative_url = Some(target.url);
                self.state.authoritative_sha256 = Some(target.sha256);
                self.state.status = DesignatedChatUrlStatusV1::Ready;
            }
            Err(error) => {
                self.state.status = DesignatedChatUrlStatusV1::from_authority(error);
            }
        }
    }

    fn apply(&mut self) {
        let Ok(canonical_draft) =
            crate::delegated::autonomy_projects::canonical_project_chat_target(
                &self.state.draft_url,
            )
        else {
            self.state.status = DesignatedChatUrlStatusV1::InvalidUrl;
            return;
        };
        let Some(expected_digest) = self.state.authoritative_sha256.as_deref() else {
            self.state.status = DesignatedChatUrlStatusV1::AuthorityUnavailable;
            return;
        };
        if self.state.authoritative_url.as_deref() == Some(canonical_draft.as_str()) {
            self.refresh();
            if self.state.status == DesignatedChatUrlStatusV1::Ready {
                self.state.status = DesignatedChatUrlStatusV1::Unchanged;
            }
            return;
        }
        match self.authority.update(&canonical_draft, expected_digest) {
            Ok(_) => {
                self.refresh();
                if self.state.status == DesignatedChatUrlStatusV1::Ready {
                    self.state.status = DesignatedChatUrlStatusV1::Updated;
                }
            }
            Err(error) => self.state.status = DesignatedChatUrlStatusV1::from_authority(error),
        }
    }
}

/// The GUI receives only the presentation exported by the canonical T-0293
/// evaluator. It never receives its evidence inputs and therefore cannot
/// manufacture acceptance from a display refresh or source fixture.
trait CoreAcceptanceReadAuthorityV1 {
    fn read_preflight(&self) -> CoreHostAcceptancePresentationV1;
}

#[derive(Clone, Debug)]
struct ProductionCoreAcceptanceReadAuthorityV1 {
    workspace: PathBuf,
}

impl ProductionCoreAcceptanceReadAuthorityV1 {
    fn new(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
        }
    }
}

impl CoreAcceptanceReadAuthorityV1 for ProductionCoreAcceptanceReadAuthorityV1 {
    fn read_preflight(&self) -> CoreHostAcceptancePresentationV1 {
        read_fixed_core_host_acceptance_preflight(&self.workspace).presentation()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CoreAcceptanceReadinessStateV1 {
    presentation: CoreHostAcceptancePresentationV1,
}

struct CoreAcceptanceReadinessControllerV1<A> {
    authority: A,
    state: CoreAcceptanceReadinessStateV1,
}

impl<A: CoreAcceptanceReadAuthorityV1> CoreAcceptanceReadinessControllerV1<A> {
    fn new(authority: A) -> Self {
        let presentation = authority.read_preflight();
        Self {
            authority,
            state: CoreAcceptanceReadinessStateV1 { presentation },
        }
    }

    fn state(&self) -> &CoreAcceptanceReadinessStateV1 {
        &self.state
    }

    /// Only replaces in-memory display data using the existing read-only
    /// evaluator. It has no mutation method and no browser/host authority.
    fn refresh(&mut self) {
        self.state.presentation = self.authority.read_preflight();
    }
}

fn render_core_acceptance_readiness(presentation: &CoreHostAcceptancePresentationV1) -> String {
    let next = presentation.next_live_gate.unwrap_or("NONE");
    let mut lines = vec![
        "Core Acceptance Readiness (read-only)".to_string(),
        format!(
            "Overall: {} ({}) | next: {next}",
            presentation.classification, presentation.reason
        ),
    ];
    for gate in &presentation.gates {
        let evidence = if gate.accepted {
            "EXACT_DURABLE_EVIDENCE"
        } else {
            "NOT_ACCEPTED"
        };
        lines.push(format!("{}: {evidence} ({})", gate.gate, gate.reason));
    }
    lines.push(
        "READY means ready for the next ordered live step; display is not live acceptance.".into(),
    );
    truncate_display_characters(&lines.join("\r\n"), MAX_CORE_ACCEPTANCE_TEXT_CHARS)
}

#[cfg(not(windows))]
pub fn run_gui(_workspace: &Path) -> Result<(), String> {
    Err("CATDESK_GUI_UNAVAILABLE_ON_THIS_PLATFORM".into())
}

#[cfg(windows)]
mod native {
    use super::{
        CoreAcceptanceReadinessControllerV1, DesignatedChatUrlControllerV1, MAX_GUI_URL_CHARS,
        ProductionCoreAcceptanceReadAuthorityV1, ProductionDesignatedChatTargetAuthorityV1,
        read_gui_text, render_core_acceptance_readiness,
    };
    use std::ffi::c_void;
    use std::iter;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    type Handle = *mut c_void;
    type Hwnd = *mut c_void;
    type Hdc = *mut c_void;
    type Hinstance = *mut c_void;

    const ERROR_ALREADY_EXISTS: u32 = 183;
    const WM_DESTROY: u32 = 0x0002;
    const WM_PAINT: u32 = 0x000F;
    const WM_CLOSE: u32 = 0x0010;
    const WM_COMMAND: u32 = 0x0111;
    const WM_TIMER: u32 = 0x0113;
    const WS_OVERLAPPEDWINDOW: u32 = 0x00CF0000;
    const WS_VISIBLE: u32 = 0x10000000;
    const WS_CHILD: u32 = 0x40000000;
    const WS_BORDER: u32 = 0x00800000;
    const ES_AUTOHSCROLL: u32 = 0x0080;
    const CW_USEDEFAULT: i32 = i32::MIN;
    const SW_RESTORE: i32 = 9;
    const IDC_ARROW: usize = 32512;
    const WHITE_BRUSH: i32 = 0;
    const DT_WORDBREAK: u32 = 0x0010;
    const DT_NOPREFIX: u32 = 0x0800;
    const GUI_TIMER_ID: usize = 1;
    const DESIGNATED_CHAT_APPLY_ID: usize = 1001;
    const DESIGNATED_CHAT_REFRESH_ID: usize = 1002;
    const CORE_ACCEPTANCE_REFRESH_ID: usize = 1003;
    const WAKE_START_ID: usize = 1010;
    const WAKE_PAUSE_ID: usize = 1011;
    const WAKE_RESUME_ID: usize = 1012;
    const WAKE_STOP_ID: usize = 1013;

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[derive(Clone, Copy)]
    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct PaintStruct {
        hdc: Hdc,
        erase: i32,
        paint: Rect,
        restore: i32,
        inc_update: i32,
        reserved: [u8; 32],
    }

    #[repr(C)]
    struct Msg {
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        point: Point,
    }

    #[repr(C)]
    struct WndClassW {
        style: u32,
        wnd_proc: Option<unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize>,
        cls_extra: i32,
        wnd_extra: i32,
        instance: Hinstance,
        icon: Handle,
        cursor: Handle,
        background: Handle,
        menu_name: *const u16,
        class_name: *const u16,
    }

    #[link(name = "User32")]
    unsafe extern "system" {
        fn RegisterClassW(class: *const WndClassW) -> u16;
        fn CreateWindowExW(
            ex_style: u32,
            class_name: *const u16,
            window_name: *const u16,
            style: u32,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: Hwnd,
            menu: Handle,
            instance: Hinstance,
            parameter: *mut c_void,
        ) -> Hwnd;
        fn DefWindowProcW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
        fn DestroyWindow(hwnd: Hwnd) -> i32;
        fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
        fn SetForegroundWindow(hwnd: Hwnd) -> i32;
        fn FindWindowW(class_name: *const u16, window_name: *const u16) -> Hwnd;
        fn GetMessageW(message: *mut Msg, hwnd: Hwnd, minimum: u32, maximum: u32) -> i32;
        fn TranslateMessage(message: *const Msg) -> i32;
        fn DispatchMessageW(message: *const Msg) -> isize;
        fn PostQuitMessage(exit_code: i32);
        fn BeginPaint(hwnd: Hwnd, paint: *mut PaintStruct) -> Hdc;
        fn EndPaint(hwnd: Hwnd, paint: *const PaintStruct) -> i32;
        fn DrawTextW(hdc: Hdc, text: *const u16, count: i32, rect: *mut Rect, format: u32) -> i32;
        fn InvalidateRect(hwnd: Hwnd, rect: *const Rect, erase: i32) -> i32;
        fn SetTimer(hwnd: Hwnd, id: usize, elapsed: u32, callback: *const c_void) -> usize;
        fn KillTimer(hwnd: Hwnd, id: usize) -> i32;
        fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
        fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, maximum: i32) -> i32;
        fn LoadCursorW(instance: Hinstance, cursor: *const u16) -> Handle;
        fn GetStockObject(index: i32) -> Handle;
        fn FillRect(hdc: Hdc, rect: *const Rect, brush: Handle) -> i32;
    }

    #[link(name = "Gdi32")]
    unsafe extern "system" {
        fn CreateSolidBrush(color: u32) -> Handle;
        fn DeleteObject(object: Handle) -> i32;
    }

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(attributes: *mut c_void, initial_owner: i32, name: *const u16) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn GetLastError() -> u32;
        fn GetModuleHandleW(module_name: *const u16) -> Hinstance;
        fn Sleep(milliseconds: u32);
    }

    struct NativeGuiState {
        controller: DesignatedChatUrlControllerV1<ProductionDesignatedChatTargetAuthorityV1>,
        core_acceptance:
            CoreAcceptanceReadinessControllerV1<ProductionCoreAcceptanceReadAuthorityV1>,
        // HWND values are used only on this GUI thread. Store their opaque
        // integer representation so the enclosing static has no accidental
        // cross-thread pointer authority.
        draft_control: usize,
        status_control: usize,
        wake_status_control: usize,
        core_acceptance_control: usize,
    }

    static GUI_WORKSPACE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    static GUI_EDITOR: OnceLock<Mutex<Option<NativeGuiState>>> = OnceLock::new();

    fn workspace_slot() -> &'static Mutex<Option<PathBuf>> {
        GUI_WORKSPACE.get_or_init(|| Mutex::new(None))
    }

    fn editor_slot() -> &'static Mutex<Option<NativeGuiState>> {
        GUI_EDITOR.get_or_init(|| Mutex::new(None))
    }

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(iter::once(0))
            .collect()
    }

    fn paint_window(hwnd: Hwnd) {
        let workspace = workspace_slot().lock().ok().and_then(|slot| slot.clone());
        let text = workspace
            .as_deref()
            .map(read_gui_text)
            .unwrap_or_else(|| "CatDesk Binagotchy\r\nStatus unavailable".into());
        let wide_text = wide(&text);
        let mut paint = PaintStruct {
            hdc: std::ptr::null_mut(),
            erase: 0,
            paint: Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            restore: 0,
            inc_update: 0,
            reserved: [0; 32],
        };
        // SAFETY: hwnd is supplied by the Windows message queue and the text
        // buffer remains alive through DrawTextW.
        unsafe {
            let hdc = BeginPaint(hwnd, &mut paint);
            if !hdc.is_null() {
                let mut rect = paint.paint;
                rect.bottom = rect.bottom.min(340);
                rect.right = rect.right.min(740);
                let _ = DrawTextW(
                    hdc,
                    wide_text.as_ptr(),
                    wide_text.len().saturating_sub(1) as i32,
                    &mut rect,
                    DT_WORDBREAK | DT_NOPREFIX,
                );
            }
            if !hdc.is_null() {
                paint_mascot(hdc);
            }
            let _ = EndPaint(hwnd, &paint);
        }
    }

    fn paint_mascot(hdc: Hdc) {
        static MASCOT: OnceLock<crate::mascot::MascotPack> = OnceLock::new();
        let mascot = MASCOT.get_or_init(|| {
            let seed = crate::state::load_app_config()
                .ok()
                .and_then(|c| c.partner_binagotchy_seed)
                .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                .unwrap_or(0x4341544445534b);
            crate::mascot::build_workspace_mascot(seed)
        });
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        for (y, row) in mascot.current_tui_frame(millis).rows.iter().enumerate() {
            for (x, cell) in row.iter().enumerate() {
                for (half, color) in [cell.fg, cell.bg].into_iter().enumerate() {
                    let (r, g, b) = color.unwrap_or((255, 255, 255));
                    let rect = Rect {
                        left: 765 + x as i32 * 8,
                        top: 24 + y as i32 * 16 + half as i32 * 8,
                        right: 773 + x as i32 * 8,
                        bottom: 32 + y as i32 * 16 + half as i32 * 8,
                    };
                    unsafe {
                        let brush = CreateSolidBrush(
                            u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16),
                        );
                        if !brush.is_null() {
                            FillRect(hdc, &rect, brush);
                            DeleteObject(brush);
                        }
                    }
                }
            }
        }
    }

    fn refresh_wake_status() {
        if let Ok(slot) = editor_slot().lock()
            && let Some(editor) = slot.as_ref()
        {
            set_control_text(
                editor.wake_status_control as Hwnd,
                &super::independent_wake_status(),
            );
        }
    }

    fn wake_control(id: usize) {
        std::thread::spawn(move || {
            if let Ok(store) = super::independent_wake_store() {
                match id {
                    WAKE_START_ID => {
                        let _ = catdesk_wake::runtime::start_installed(&store);
                    }
                    WAKE_PAUSE_ID => {
                        let _ = catdesk_wake::runtime::control(&store, "PAUSED");
                    }
                    WAKE_RESUME_ID => {
                        if store.config().is_ok_and(|c| !c.targets.is_empty()) {
                            let _ = catdesk_wake::runtime::control(&store, "RUNNING");
                        }
                    }
                    WAKE_STOP_ID => {
                        let _ = catdesk_wake::runtime::control(&store, "STOPPED");
                    }
                    _ => {}
                }
            }
        });
    }

    fn set_control_text(control: Hwnd, text: &str) {
        let text = wide(text);
        // SAFETY: the child HWND belongs to this process and the UTF-16
        // buffer remains live for the synchronous call.
        unsafe {
            let _ = SetWindowTextW(control, text.as_ptr());
        }
    }

    fn update_editor_controls() {
        let Ok(slot) = editor_slot().lock() else {
            return;
        };
        let Some(editor) = slot.as_ref() else {
            return;
        };
        set_control_text(
            editor.draft_control as Hwnd,
            &editor.controller.state().draft_url,
        );
        set_control_text(
            editor.status_control as Hwnd,
            editor.controller.state().status.label(),
        );
        set_control_text(
            editor.core_acceptance_control as Hwnd,
            &render_core_acceptance_readiness(&editor.core_acceptance.state().presentation),
        );
    }

    fn read_draft_control(control: Hwnd) -> String {
        let mut buffer = vec![0u16; MAX_GUI_URL_CHARS + 1];
        // SAFETY: the buffer has the advertised capacity and belongs to this
        // process; GetWindowTextW is synchronous.
        let copied = unsafe { GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32) }
            .max(0) as usize;
        String::from_utf16_lossy(&buffer[..copied])
    }

    fn apply_editor_draft() {
        let Ok(mut slot) = editor_slot().lock() else {
            return;
        };
        let Some(editor) = slot.as_mut() else {
            return;
        };
        editor
            .controller
            .edit_draft(read_draft_control(editor.draft_control as Hwnd));
        editor.controller.apply();
        drop(slot);
        update_editor_controls();
    }

    fn refresh_editor_readback() {
        let Ok(mut slot) = editor_slot().lock() else {
            return;
        };
        let Some(editor) = slot.as_mut() else {
            return;
        };
        editor.controller.refresh();
        drop(slot);
        update_editor_controls();
    }

    fn refresh_core_acceptance_readback() {
        let Ok(mut slot) = editor_slot().lock() else {
            return;
        };
        let Some(editor) = slot.as_mut() else {
            return;
        };
        editor.core_acceptance.refresh();
        drop(slot);
        update_editor_controls();
    }

    fn create_editor_controls(
        hwnd: Hwnd,
        instance: Hinstance,
        workspace: &Path,
    ) -> Result<(), String> {
        let label = wide("Designated Chat URL");
        let edit_class = wide("EDIT");
        let button_class = wide("BUTTON");
        let static_class = wide("STATIC");
        let apply = wide("Apply/Update");
        let refresh = wide("Refresh readback");
        let core_label = wide("Core Acceptance Readiness");
        let core_refresh = wide("Refresh readiness");
        // SAFETY: these are fixed child controls with fixed styles and IDs;
        // no caller-provided path, command, or browser authority is used.
        let label_control = unsafe {
            CreateWindowExW(
                0,
                static_class.as_ptr(),
                label.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                16,
                350,
                180,
                24,
                hwnd,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        let draft_control = unsafe {
            CreateWindowExW(
                0,
                edit_class.as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_BORDER | ES_AUTOHSCROLL,
                16,
                374,
                710,
                26,
                hwnd,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        let apply_control = unsafe {
            CreateWindowExW(
                0,
                button_class.as_ptr(),
                apply.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                16,
                410,
                140,
                28,
                hwnd,
                DESIGNATED_CHAT_APPLY_ID as isize as Handle,
                instance,
                std::ptr::null_mut(),
            )
        };
        let refresh_control = unsafe {
            CreateWindowExW(
                0,
                button_class.as_ptr(),
                refresh.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                164,
                410,
                150,
                28,
                hwnd,
                DESIGNATED_CHAT_REFRESH_ID as isize as Handle,
                instance,
                std::ptr::null_mut(),
            )
        };
        let status_control = unsafe {
            CreateWindowExW(
                0,
                static_class.as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE,
                324,
                416,
                402,
                20,
                hwnd,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        let core_label_control = unsafe {
            CreateWindowExW(
                0,
                static_class.as_ptr(),
                core_label.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                16,
                452,
                230,
                24,
                hwnd,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        let core_refresh_control = unsafe {
            CreateWindowExW(
                0,
                button_class.as_ptr(),
                core_refresh.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                250,
                448,
                160,
                28,
                hwnd,
                CORE_ACCEPTANCE_REFRESH_ID as isize as Handle,
                instance,
                std::ptr::null_mut(),
            )
        };
        let core_acceptance_control = unsafe {
            CreateWindowExW(
                0,
                static_class.as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | WS_VISIBLE,
                16,
                480,
                710,
                128,
                hwnd,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        let wake_status_control = unsafe {
            CreateWindowExW(
                0,
                static_class.as_ptr(),
                wide(&super::independent_wake_status()).as_ptr(),
                WS_CHILD | WS_VISIBLE,
                760,
                325,
                340,
                280,
                hwnd,
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        for (index, (id, name)) in [
            (WAKE_START_ID, "Start Wake"),
            (WAKE_PAUSE_ID, "Pause Wake"),
            (WAKE_RESUME_ID, "Resume Wake"),
            (WAKE_STOP_ID, "Stop Wake"),
        ]
        .into_iter()
        .enumerate()
        {
            let control = unsafe {
                CreateWindowExW(
                    0,
                    button_class.as_ptr(),
                    wide(name).as_ptr(),
                    WS_CHILD | WS_VISIBLE,
                    16 + index as i32 * 175,
                    625,
                    165,
                    30,
                    hwnd,
                    id as isize as Handle,
                    instance,
                    std::ptr::null_mut(),
                )
            };
            if control.is_null() {
                return Err("CATDESK_GUI_WAKE_CONTROLS_UNAVAILABLE".into());
            }
        }
        if label_control.is_null()
            || draft_control.is_null()
            || apply_control.is_null()
            || refresh_control.is_null()
            || status_control.is_null()
            || core_label_control.is_null()
            || core_refresh_control.is_null()
            || core_acceptance_control.is_null()
            || wake_status_control.is_null()
        {
            return Err("CATDESK_GUI_EDITOR_UNAVAILABLE".into());
        }
        let controller = DesignatedChatUrlControllerV1::new(
            ProductionDesignatedChatTargetAuthorityV1::new(workspace),
        );
        let core_acceptance = CoreAcceptanceReadinessControllerV1::new(
            ProductionCoreAcceptanceReadAuthorityV1::new(workspace),
        );
        *editor_slot()
            .lock()
            .map_err(|_| "CATDESK_GUI_STATE_UNAVAILABLE")? = Some(NativeGuiState {
            controller,
            core_acceptance,
            draft_control: draft_control as usize,
            status_control: status_control as usize,
            wake_status_control: wake_status_control as usize,
            core_acceptance_control: core_acceptance_control as usize,
        });
        update_editor_controls();
        Ok(())
    }

    unsafe extern "system" fn window_proc(
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        match message {
            WM_PAINT => {
                paint_window(hwnd);
                0
            }
            WM_TIMER => {
                refresh_wake_status();
                // SAFETY: hwnd belongs to this window procedure.
                unsafe { InvalidateRect(hwnd, std::ptr::null(), 1) };
                0
            }
            WM_COMMAND => {
                match wparam & 0xffff {
                    DESIGNATED_CHAT_APPLY_ID => apply_editor_draft(),
                    DESIGNATED_CHAT_REFRESH_ID => refresh_editor_readback(),
                    CORE_ACCEPTANCE_REFRESH_ID => refresh_core_acceptance_readback(),
                    WAKE_START_ID | WAKE_PAUSE_ID | WAKE_RESUME_ID | WAKE_STOP_ID => {
                        wake_control(wparam & 0xffff)
                    }
                    _ => {}
                }
                0
            }
            WM_CLOSE => {
                // Closing the view only ends this process; no daemon shutdown
                // or transport operation exists in this module.
                unsafe { DestroyWindow(hwnd) };
                0
            }
            WM_DESTROY => {
                unsafe { KillTimer(hwnd, GUI_TIMER_ID) };
                if let Ok(mut slot) = editor_slot().lock() {
                    *slot = None;
                }
                unsafe { PostQuitMessage(0) };
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    pub(super) fn run_gui(workspace: &Path) -> Result<(), String> {
        let mutex_name = wide("Local\\CatDesk.BinagotchyGui.V1");
        // SAFETY: fixed local mutex name; no security-sensitive authority is
        // transferred through it.
        let mutex = unsafe { CreateMutexW(std::ptr::null_mut(), 0, mutex_name.as_ptr()) };
        if mutex.is_null() {
            return Err("CATDESK_GUI_SINGLE_INSTANCE_UNAVAILABLE".into());
        }
        // SAFETY: immediately follows CreateMutexW on this thread.
        let already_exists = unsafe { GetLastError() == ERROR_ALREADY_EXISTS };
        let class_name = wide("CatDeskBinagotchyGuiV1");
        let title = wide("CatDesk Binagotchy");
        if already_exists {
            // A just-starting existing instance may not have created the
            // window yet. Wait only a bounded 500ms before refusing a second.
            for _ in 0..10 {
                // SAFETY: fixed class/title buffers remain valid in this scope.
                let existing = unsafe { FindWindowW(class_name.as_ptr(), title.as_ptr()) };
                if !existing.is_null() {
                    unsafe {
                        ShowWindow(existing, SW_RESTORE);
                        SetForegroundWindow(existing);
                        CloseHandle(mutex);
                    }
                    return Ok(());
                }
                unsafe { Sleep(50) };
            }
            unsafe { CloseHandle(mutex) };
            return Err("CATDESK_GUI_EXISTING_INSTANCE_UNAVAILABLE".into());
        }

        *workspace_slot()
            .lock()
            .map_err(|_| "CATDESK_GUI_STATE_UNAVAILABLE")? = Some(workspace.to_path_buf());
        // SAFETY: process module handle is borrowed for class/window creation.
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let class = WndClassW {
            style: 0,
            wnd_proc: Some(window_proc),
            cls_extra: 0,
            wnd_extra: 0,
            instance,
            icon: std::ptr::null_mut(),
            cursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW as *const u16) },
            background: unsafe { GetStockObject(WHITE_BRUSH) },
            menu_name: std::ptr::null(),
            class_name: class_name.as_ptr(),
        };
        // SAFETY: the descriptor and UTF-16 class storage remain live for the
        // lifetime of the message loop below.
        if unsafe { RegisterClassW(&class) } == 0 {
            unsafe { CloseHandle(mutex) };
            return Err("CATDESK_GUI_WINDOW_CLASS_UNAVAILABLE".into());
        }
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1130,
                720,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            unsafe { CloseHandle(mutex) };
            return Err("CATDESK_GUI_WINDOW_UNAVAILABLE".into());
        }
        if let Err(error) = create_editor_controls(hwnd, instance, workspace) {
            unsafe {
                DestroyWindow(hwnd);
                CloseHandle(mutex);
            }
            return Err(error);
        }
        unsafe {
            ShowWindow(hwnd, SW_RESTORE);
            let _ = SetTimer(hwnd, GUI_TIMER_ID, 1_000, std::ptr::null());
        }
        wake_control(WAKE_START_ID);
        let mut message = Msg {
            hwnd: std::ptr::null_mut(),
            message: 0,
            wparam: 0,
            lparam: 0,
            time: 0,
            point: Point { x: 0, y: 0 },
        };
        let message_result = loop {
            let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
            match super::classify_message_pump_result(result) {
                super::MessagePumpDisposition::Dispatch => unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                },
                super::MessagePumpDisposition::Quit => break Ok(()),
                super::MessagePumpDisposition::Failure => {
                    break Err("CATDESK_GUI_MESSAGE_PUMP_UNAVAILABLE".into());
                }
            }
        };
        if let Ok(mut slot) = workspace_slot().lock() {
            *slot = None;
        }
        unsafe { CloseHandle(mutex) };
        message_result
    }
}

#[cfg(windows)]
pub fn run_gui(workspace: &Path) -> Result<(), String> {
    native::run_gui(workspace)
}

#[cfg(test)]
mod tests {
    use super::{
        BINAGOTCHY_GUI_MODE_FLAG, CoreAcceptanceReadAuthorityV1,
        CoreAcceptanceReadinessControllerV1, DesignatedChatTargetAuthorityV1,
        DesignatedChatUrlControllerV1, DesignatedChatUrlStatusV1, GuiLaunchDecision,
        MAX_CORE_ACCEPTANCE_TEXT_CHARS, MAX_GUI_TEXT_CHARS, MessagePumpDisposition, bounded,
        classify_message_pump_result, lifecycle_gui_decision, parse_binagotchy_gui_mode,
        render_core_acceptance_readiness, render_snapshot, truncate_display_characters,
    };
    use crate::core_host_acceptance_preflight::{
        CoreHostAcceptanceGatePresentationV1, CoreHostAcceptancePresentationV1,
    };
    use crate::delegated::autonomy_observability::AutonomyObservabilitySnapshotV1;
    use crate::mcp::{DesignatedChatTargetErrorV1, DesignatedChatTargetV1};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct FixtureChatTargetAuthority {
        state: Arc<Mutex<FixtureChatTargetState>>,
    }

    struct FixtureChatTargetState {
        target: DesignatedChatTargetV1,
        read_error: Option<DesignatedChatTargetErrorV1>,
        update_error: Option<DesignatedChatTargetErrorV1>,
        update_calls: usize,
    }

    impl FixtureChatTargetAuthority {
        fn new(url: &str, sha256: &str) -> Self {
            Self {
                state: Arc::new(Mutex::new(FixtureChatTargetState {
                    target: DesignatedChatTargetV1 {
                        url: url.into(),
                        sha256: sha256.into(),
                    },
                    read_error: None,
                    update_error: None,
                    update_calls: 0,
                })),
            }
        }
    }

    impl DesignatedChatTargetAuthorityV1 for FixtureChatTargetAuthority {
        fn readback(&self) -> Result<DesignatedChatTargetV1, DesignatedChatTargetErrorV1> {
            let state = self.state.lock().expect("fixture lock");
            state
                .read_error
                .map_or_else(|| Ok(state.target.clone()), Err)
        }

        fn update(
            &self,
            draft_url: &str,
            expected_current_target_sha256: &str,
        ) -> Result<DesignatedChatTargetV1, DesignatedChatTargetErrorV1> {
            let mut state = self.state.lock().expect("fixture lock");
            state.update_calls += 1;
            if state.target.sha256 != expected_current_target_sha256 {
                return Err(DesignatedChatTargetErrorV1::Stale);
            }
            if let Some(error) = state.update_error {
                return Err(error);
            }
            state.target = DesignatedChatTargetV1 {
                url: draft_url.into(),
                sha256: format!("{}-updated", state.target.sha256),
            };
            Ok(state.target.clone())
        }
    }

    #[derive(Clone)]
    struct FixtureCoreAcceptanceAuthority {
        state: Arc<Mutex<FixtureCoreAcceptanceState>>,
    }

    struct FixtureCoreAcceptanceState {
        presentation: CoreHostAcceptancePresentationV1,
        read_calls: usize,
    }

    impl FixtureCoreAcceptanceAuthority {
        fn new(presentation: CoreHostAcceptancePresentationV1) -> Self {
            Self {
                state: Arc::new(Mutex::new(FixtureCoreAcceptanceState {
                    presentation,
                    read_calls: 0,
                })),
            }
        }

        fn replace(&self, presentation: CoreHostAcceptancePresentationV1) {
            self.state.lock().expect("fixture lock").presentation = presentation;
        }
    }

    impl CoreAcceptanceReadAuthorityV1 for FixtureCoreAcceptanceAuthority {
        fn read_preflight(&self) -> CoreHostAcceptancePresentationV1 {
            let mut state = self.state.lock().expect("fixture lock");
            state.read_calls += 1;
            state.presentation.clone()
        }
    }

    fn core_presentation(
        classification: &'static str,
        reason: &'static str,
        next_live_gate: Option<&'static str>,
        accepted: [bool; 4],
    ) -> CoreHostAcceptancePresentationV1 {
        let gates = [
            ("T-0224", accepted[0]),
            ("T-0223", accepted[1]),
            ("T-0222/T-0139", accepted[2]),
            ("T-0152", accepted[3]),
        ]
        .map(|(gate, accepted)| CoreHostAcceptanceGatePresentationV1 {
            gate,
            accepted,
            reason: if accepted {
                "EXACT_DURABLE_LIVE_EVIDENCE"
            } else if next_live_gate == Some(gate) {
                reason
            } else {
                "ORDERED_LIVE_GATE_PENDING"
            },
        });
        CoreHostAcceptancePresentationV1 {
            classification,
            reason,
            next_live_gate,
            gates,
        }
    }

    #[test]
    fn gui_mode_is_zero_parameter_and_closed() {
        assert_eq!(
            parse_binagotchy_gui_mode(&[BINAGOTCHY_GUI_MODE_FLAG.to_string()]),
            Ok(true)
        );
        assert_eq!(parse_binagotchy_gui_mode(&[]), Ok(false));
        assert!(
            parse_binagotchy_gui_mode(&[
                BINAGOTCHY_GUI_MODE_FLAG.to_string(),
                "C:\\attacker.exe".to_string()
            ])
            .is_err()
        );
        assert!(
            parse_binagotchy_gui_mode(&[
                BINAGOTCHY_GUI_MODE_FLAG.to_string(),
                BINAGOTCHY_GUI_MODE_FLAG.to_string()
            ])
            .is_err()
        );
    }

    #[test]
    fn lifecycle_launch_decision_keeps_headless_contexts_safe() {
        assert_eq!(lifecycle_gui_decision(true), GuiLaunchDecision::Launch);
        assert_eq!(
            lifecycle_gui_decision(false),
            GuiLaunchDecision::SkipHeadless
        );
    }

    #[test]
    fn rendered_view_uses_existing_redacted_observability_fields() {
        let mut snapshot = AutonomyObservabilitySnapshotV1::unknown(
            "TRANSPORT=OPENAI_SECURE; LOCAL_MCP=READY",
            "test local read",
        );
        snapshot.project_id = "catdesk".into();
        snapshot.task_id = "T-0139".into();
        snapshot.session_id = "session-1".into();
        snapshot.provider = "QWEN".into();
        snapshot.wake = "WAITING_FOR_CHATGPT".into();
        let rendered = render_snapshot(&snapshot);
        for expected in [
            "CatDesk Binagotchy",
            "catdesk / T-0139 / session-1",
            "Provider: QWEN",
            "WAITING_FOR_CHATGPT",
            "TRANSPORT=OPENAI_SECURE; LOCAL_MCP=READY",
            "Close or minimize",
        ] {
            assert!(rendered.contains(expected), "missing {expected}");
        }
    }

    #[test]
    fn display_truncation_is_character_safe_at_and_beyond_ascii_cap() {
        let exact = "a".repeat(MAX_GUI_TEXT_CHARS);
        assert_eq!(
            truncate_display_characters(&exact, MAX_GUI_TEXT_CHARS),
            exact
        );

        let beyond = format!("{}z", "a".repeat(MAX_GUI_TEXT_CHARS));
        let rendered = truncate_display_characters(&beyond, MAX_GUI_TEXT_CHARS);
        assert_eq!(rendered.chars().count(), MAX_GUI_TEXT_CHARS);
        assert!(rendered.chars().all(|character| character == 'a'));
    }

    #[test]
    fn display_truncation_keeps_unicode_boundaries_valid() {
        let emoji_boundary = format!("{}😀x", "a".repeat(MAX_GUI_TEXT_CHARS - 1));
        let emoji_rendered = truncate_display_characters(&emoji_boundary, MAX_GUI_TEXT_CHARS);
        assert_eq!(emoji_rendered.chars().count(), MAX_GUI_TEXT_CHARS);
        assert!(emoji_rendered.ends_with('😀'));

        let cjk_rendered =
            truncate_display_characters(&"界".repeat(MAX_GUI_TEXT_CHARS + 1), MAX_GUI_TEXT_CHARS);
        assert_eq!(cjk_rendered.chars().count(), MAX_GUI_TEXT_CHARS);
        assert!(cjk_rendered.chars().all(|character| character == '界'));

        let short_unicode = "naïve 🐈";
        assert_eq!(
            truncate_display_characters(short_unicode, MAX_GUI_TEXT_CHARS),
            short_unicode
        );
    }

    #[test]
    fn field_rendering_sanitizes_control_characters() {
        let rendered = bounded("A\nB\t\u{7f}C");
        assert_eq!(rendered, "A B  C");
        assert!(rendered.chars().all(|character| !character.is_control()));
    }

    #[test]
    fn message_pump_classifier_distinguishes_dispatch_quit_and_failure() {
        assert_eq!(
            classify_message_pump_result(-1),
            MessagePumpDisposition::Failure
        );
        assert_eq!(
            classify_message_pump_result(0),
            MessagePumpDisposition::Quit
        );
        assert_eq!(
            classify_message_pump_result(1),
            MessagePumpDisposition::Dispatch
        );
        assert_eq!(
            classify_message_pump_result(7),
            MessagePumpDisposition::Dispatch
        );
    }

    #[test]
    fn designated_chat_url_initial_and_recreated_controllers_use_authoritative_readback() {
        let authority = FixtureChatTargetAuthority::new(
            "https://chatgpt.com/c/initial-thread",
            "digest-initial",
        );
        let first = DesignatedChatUrlControllerV1::new(authority.clone());
        assert_eq!(
            first.state().authoritative_url.as_deref(),
            Some("https://chatgpt.com/c/initial-thread")
        );
        assert_eq!(
            first.state().draft_url,
            "https://chatgpt.com/c/initial-thread"
        );
        assert_eq!(first.state().status, DesignatedChatUrlStatusV1::Ready);

        authority.state.lock().expect("fixture lock").target = DesignatedChatTargetV1 {
            url: "https://chatgpt.com/c/persisted-thread".into(),
            sha256: "digest-persisted".into(),
        };
        let recreated = DesignatedChatUrlControllerV1::new(authority);
        assert_eq!(
            recreated.state().authoritative_url.as_deref(),
            Some("https://chatgpt.com/c/persisted-thread")
        );
        assert_eq!(
            recreated.state().authoritative_sha256.as_deref(),
            Some("digest-persisted")
        );
    }

    #[test]
    fn designated_chat_url_edit_without_apply_is_a_noop() {
        let authority = FixtureChatTargetAuthority::new(
            "https://chatgpt.com/c/initial-thread",
            "digest-initial",
        );
        let mut controller = DesignatedChatUrlControllerV1::new(authority.clone());
        controller.edit_draft("https://chatgpt.com/c/draft-thread".into());
        assert_eq!(
            controller.state().draft_url,
            "https://chatgpt.com/c/draft-thread"
        );
        let state = authority.state.lock().expect("fixture lock");
        assert_eq!(state.update_calls, 0);
        assert_eq!(state.target.url, "https://chatgpt.com/c/initial-thread");
    }

    #[test]
    fn designated_chat_url_apply_updates_through_guarded_authority_then_refreshes() {
        let authority = FixtureChatTargetAuthority::new(
            "https://chatgpt.com/c/initial-thread",
            "digest-initial",
        );
        let mut controller = DesignatedChatUrlControllerV1::new(authority.clone());
        controller.edit_draft("https://chatgpt.com/c/updated-thread".into());
        controller.apply();
        assert_eq!(
            controller.state().status,
            DesignatedChatUrlStatusV1::Updated
        );
        assert_eq!(
            controller.state().authoritative_url.as_deref(),
            Some("https://chatgpt.com/c/updated-thread")
        );
        assert_eq!(
            authority.state.lock().expect("fixture lock").update_calls,
            1
        );
    }

    #[test]
    fn designated_chat_url_same_value_apply_is_idempotent() {
        let authority = FixtureChatTargetAuthority::new(
            "https://chatgpt.com/c/initial-thread",
            "digest-initial",
        );
        let mut controller = DesignatedChatUrlControllerV1::new(authority.clone());
        controller.apply();
        assert_eq!(
            controller.state().status,
            DesignatedChatUrlStatusV1::Unchanged
        );
        assert_eq!(
            authority.state.lock().expect("fixture lock").update_calls,
            0
        );
    }

    #[test]
    fn designated_chat_url_rejects_malformed_draft_without_authority_mutation() {
        let authority = FixtureChatTargetAuthority::new(
            "https://chatgpt.com/c/initial-thread",
            "digest-initial",
        );
        let mut controller = DesignatedChatUrlControllerV1::new(authority.clone());
        controller.edit_draft("https://example.test/c/not-chatgpt".into());
        controller.apply();
        assert_eq!(
            controller.state().status,
            DesignatedChatUrlStatusV1::InvalidUrl
        );
        assert_eq!(
            authority.state.lock().expect("fixture lock").update_calls,
            0
        );
    }

    #[test]
    fn designated_chat_url_stale_and_pre_mutation_failures_are_redacted_and_non_partial() {
        let authority = FixtureChatTargetAuthority::new(
            "https://chatgpt.com/c/initial-thread",
            "digest-initial",
        );
        let mut controller = DesignatedChatUrlControllerV1::new(authority.clone());
        authority.state.lock().expect("fixture lock").target = DesignatedChatTargetV1 {
            url: "https://chatgpt.com/c/concurrent-thread".into(),
            sha256: "digest-concurrent".into(),
        };
        controller.edit_draft("https://chatgpt.com/c/updated-thread".into());
        controller.apply();
        assert_eq!(
            controller.state().status,
            DesignatedChatUrlStatusV1::StaleTarget
        );
        assert_eq!(
            authority.state.lock().expect("fixture lock").target.url,
            "https://chatgpt.com/c/concurrent-thread"
        );

        let authority = FixtureChatTargetAuthority::new(
            "https://chatgpt.com/c/initial-thread",
            "digest-initial",
        );
        authority.state.lock().expect("fixture lock").update_error =
            Some(DesignatedChatTargetErrorV1::ProtectedStateMismatch);
        let mut controller = DesignatedChatUrlControllerV1::new(authority.clone());
        controller.edit_draft("https://chatgpt.com/c/updated-thread".into());
        controller.apply();
        assert_eq!(
            controller.state().status,
            DesignatedChatUrlStatusV1::ProtectedStateMismatch
        );
        let state = authority.state.lock().expect("fixture lock");
        assert_eq!(state.target.url, "https://chatgpt.com/c/initial-thread");
        assert_eq!(state.update_calls, 1);
        assert!(
            !controller.state().status.label().contains("initial-thread"),
            "bounded GUI error must not disclose the target"
        );
    }

    #[test]
    fn core_acceptance_readiness_initial_render_is_a_read_only_t0293_presentation() {
        let authority = FixtureCoreAcceptanceAuthority::new(core_presentation(
            "READY",
            "T0224_NATURAL_DELIVERY_REQUIRED",
            Some("T-0224"),
            [false, false, false, false],
        ));
        let controller = CoreAcceptanceReadinessControllerV1::new(authority.clone());
        let rendered = render_core_acceptance_readiness(&controller.state().presentation);

        for expected in [
            "Core Acceptance Readiness (read-only)",
            "Overall: READY (T0224_NATURAL_DELIVERY_REQUIRED) | next: T-0224",
            "T-0224: NOT_ACCEPTED (T0224_NATURAL_DELIVERY_REQUIRED)",
            "T-0223: NOT_ACCEPTED (ORDERED_LIVE_GATE_PENDING)",
            "T-0222/T-0139: NOT_ACCEPTED (ORDERED_LIVE_GATE_PENDING)",
            "T-0152: NOT_ACCEPTED (ORDERED_LIVE_GATE_PENDING)",
            "READY means ready for the next ordered live step; display is not live acceptance.",
        ] {
            assert!(rendered.contains(expected), "missing {expected}");
        }
        assert_eq!(authority.state.lock().expect("fixture lock").read_calls, 1);
        assert!(
            controller
                .state()
                .presentation
                .gates
                .iter()
                .all(|gate| !gate.accepted)
        );
    }

    #[test]
    fn core_acceptance_readiness_preserves_failure_and_live_evidence_states_without_upgrading_them()
    {
        let cases = [
            (
                core_presentation(
                    "FAILED_DETERMINISTIC_PREREQUISITE",
                    "SUPERVISOR_READINESS_UNAVAILABLE",
                    None,
                    [false, false, false, false],
                ),
                "FAILED_DETERMINISTIC_PREREQUISITE",
            ),
            (
                core_presentation(
                    "BLOCKED_BY_LIVE_ACCEPTANCE",
                    "T0224_LIVE_EVIDENCE_INVALID",
                    Some("T-0224"),
                    [false, false, false, false],
                ),
                "T0224_LIVE_EVIDENCE_INVALID",
            ),
            (
                core_presentation(
                    "BLOCKED_BY_LIVE_ACCEPTANCE",
                    "T0223_LIVE_EVIDENCE_INVALID",
                    Some("T-0223"),
                    [true, false, false, false],
                ),
                "T0223_LIVE_EVIDENCE_INVALID",
            ),
        ];

        for (presentation, expected) in cases {
            let original = presentation.clone();
            let rendered = render_core_acceptance_readiness(&presentation);
            assert!(rendered.contains(expected));
            assert!(rendered.contains("NOT_ACCEPTED"));
            assert_eq!(presentation, original, "rendering must be pure");
        }
    }

    #[test]
    fn core_acceptance_readiness_renders_authoritative_fixture_without_claiming_gui_acceptance() {
        let presentation = core_presentation(
            "READY",
            "ALL_CORE_LIVE_GATES_AUTHORITATIVELY_ACCEPTED",
            None,
            [true, true, true, true],
        );
        let rendered = render_core_acceptance_readiness(&presentation);
        assert_eq!(rendered.matches("EXACT_DURABLE_EVIDENCE").count(), 4);
        assert!(rendered.contains("display is not live acceptance"));
        assert_eq!(
            presentation
                .gates
                .iter()
                .filter(|gate| gate.accepted)
                .count(),
            4
        );
    }

    #[test]
    fn core_acceptance_readiness_refresh_replaces_only_in_memory_readback() {
        let authority = FixtureCoreAcceptanceAuthority::new(core_presentation(
            "READY",
            "T0224_NATURAL_DELIVERY_REQUIRED",
            Some("T-0224"),
            [false, false, false, false],
        ));
        let mut controller = CoreAcceptanceReadinessControllerV1::new(authority.clone());
        authority.replace(core_presentation(
            "BLOCKED_BY_LIVE_ACCEPTANCE",
            "T0224_LIVE_EVIDENCE_INVALID",
            Some("T-0224"),
            [false, false, false, false],
        ));
        controller.refresh();

        assert_eq!(
            controller.state().presentation.reason,
            "T0224_LIVE_EVIDENCE_INVALID"
        );
        assert_eq!(authority.state.lock().expect("fixture lock").read_calls, 2);
    }

    #[test]
    fn core_acceptance_readiness_output_is_bounded_and_redacted() {
        let rendered = render_core_acceptance_readiness(&core_presentation(
            "BLOCKED_BY_LIVE_ACCEPTANCE",
            "T0224_LIVE_EVIDENCE_INVALID",
            Some("T-0224"),
            [false, false, false, false],
        ));
        assert!(rendered.chars().count() <= MAX_CORE_ACCEPTANCE_TEXT_CHARS);
        for forbidden in ["https://", "C:\\", "browser profile", "token="] {
            assert!(
                !rendered.contains(forbidden),
                "unexpected detail: {forbidden}"
            );
        }
    }

    #[test]
    fn gui_module_is_a_read_only_view_not_a_daemon_or_tunnel_owner() {
        let source = include_str!("windows_gui.rs");
        let forbidden = [
            ["AppState", "::new("].concat(),
            ["start", "_services("].concat(),
            ["run_native", "_daemon("].concat(),
            ["run_reviewed", "_build_worker("].concat(),
        ];
        for forbidden in forbidden {
            assert!(
                !source.contains(&forbidden),
                "unexpected GUI ownership: {forbidden}"
            );
        }
    }
}
