//! Closed native Windows startup authority for the stable control-plane
//! supervisor.  This module has no caller-selected task, executable, user,
//! session, argument, or scheduler input.  It uses Task Scheduler COM directly
//! rather than a shell, the Task Scheduler command-line executable, a Run
//! key, or service persistence.

use std::path::Path;

use crate::control_plane_supervisor::FIXED_STABLE_SUPERVISOR_RUNTIME_FLAG;

/// One stable task identity, intentionally kept in the Task Scheduler root so
/// no folder create/delete authority is needed.  The current numeric session
/// is *not* placed in this definition: it only proves that the process that
/// constructed the authority is interactive right now.
pub(crate) const FIXED_SUPERVISOR_STARTUP_TASK_NAME: &str = "CatDeskStableSupervisorV1";
const FIXED_SUPERVISOR_STARTUP_TASK_URI: &str = "\\CatDeskStableSupervisorV1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SupervisorStartupDefinitionStateV1 {
    Absent,
    ExactOwned,
    /// The fixed name exists but is not the exact CatDesk definition.  It is
    /// never overwritten or deleted by this authority.
    ForeignOrAmbiguous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SupervisorStartupErrorV1 {
    AuthorityUnavailable,
    ElevationRequired,
    ForeignOrAmbiguous,
    NativeApiUnavailable,
    RegistrationFailed,
    PostRegistrationMismatch,
}

/// Opaque evidence that the current process is the interactive product user.
/// Its SID is obtained from the current OS token through the same accepted
/// TokenUser/TokenSessionId derivation used by the fixed pipe.  No public or
/// caller-controlled constructor exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SupervisorStartupAuthorityV1 {
    user_sid: String,
}

impl SupervisorStartupAuthorityV1 {
    #[cfg(test)]
    pub(crate) fn test_only_interactive_user(
        user_sid: &str,
    ) -> Result<Self, SupervisorStartupErrorV1> {
        validate_interactive_user_sid(user_sid).map(|user_sid| Self { user_sid })
    }

    fn user_sid(&self) -> &str {
        &self.user_sid
    }
}

/// The exact, non-secret scheduler definition that is admissible at the fixed
/// task name.  The command path is supplied only by the protected installer
/// composition after it has revalidated the current receipt/image; it is not
/// an operator argument or a path trust anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FixedSupervisorStartupDefinitionV1 {
    user_sid: String,
    action_path: String,
    enabled: bool,
}

impl FixedSupervisorStartupDefinitionV1 {
    pub(crate) fn from_protected_installer_action(
        authority: &SupervisorStartupAuthorityV1,
        action_path: &Path,
    ) -> Result<Self, SupervisorStartupErrorV1> {
        let action_path = action_path
            .to_str()
            .filter(|value| !value.is_empty() && value.len() <= 1024)
            .ok_or(SupervisorStartupErrorV1::NativeApiUnavailable)?;
        // The caller cannot select this path: the lifecycle obtains it only
        // from the fixed protected installer receipt.  This is defense in
        // depth against malformed internal plumbing, not a trust decision.
        if !action_path.starts_with(r"C:\ProgramData\CatDesk\ControlPlaneSupervisor\")
            || !action_path.ends_with(r"\catdesk-control-plane-supervisor.exe")
        {
            return Err(SupervisorStartupErrorV1::NativeApiUnavailable);
        }
        Ok(Self {
            user_sid: authority.user_sid().to_owned(),
            action_path: action_path.to_owned(),
            enabled: true,
        })
    }

    #[cfg(test)]
    pub(crate) fn test_only_for_action(
        authority: &SupervisorStartupAuthorityV1,
        action_path: &str,
    ) -> Result<Self, SupervisorStartupErrorV1> {
        if action_path.is_empty() || action_path.len() > 1024 {
            return Err(SupervisorStartupErrorV1::NativeApiUnavailable);
        }
        Ok(Self {
            user_sid: authority.user_sid().to_owned(),
            action_path: action_path.to_owned(),
            enabled: true,
        })
    }

    pub(crate) fn staged_disabled(&self) -> Self {
        Self {
            user_sid: self.user_sid.clone(),
            action_path: self.action_path.clone(),
            enabled: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn task_xml_for_test(&self) -> String {
        self.task_xml()
    }

    fn task_xml(&self) -> String {
        // TASK_LOGON_INTERACTIVE_TOKEN and LeastPrivilege deliberately avoid
        // stored passwords, service/LocalSystem identity, elevation, and any
        // persisted numeric session id.  The action is the single fixed
        // stable-supervisor role in the protected installer image.
        format!(
            r#"<?xml version="1.0" encoding="UTF-16"?><Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><RegistrationInfo><URI>{uri}</URI></RegistrationInfo><Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{sid}</UserId></LogonTrigger></Triggers><Principals><Principal id="CatDeskInteractiveUser"><UserId>{sid}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals><Settings><Enabled>{enabled}</Enabled><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><StartWhenAvailable>false</StartWhenAvailable><ExecutionTimeLimit>PT0S</ExecutionTimeLimit></Settings><Actions Context="CatDeskInteractiveUser"><Exec><Command>{path}</Command><Arguments>{argument}</Arguments></Exec></Actions></Task>"#,
            uri = FIXED_SUPERVISOR_STARTUP_TASK_URI,
            sid = xml_escape(&self.user_sid),
            path = xml_escape(&self.action_path),
            argument = FIXED_STABLE_SUPERVISOR_RUNTIME_FLAG,
            enabled = if self.enabled { "true" } else { "false" },
        )
    }
}

pub(crate) fn fixed_supervisor_startup_authority()
-> Result<SupervisorStartupAuthorityV1, SupervisorStartupErrorV1> {
    #[cfg(windows)]
    {
        let sid =
            crate::windows_supervisor_control_pipe::fixed_interactive_supervisor_startup_user_sid()
                .map_err(|_| SupervisorStartupErrorV1::AuthorityUnavailable)?;
        validate_interactive_user_sid(&sid)
            .map(|user_sid| SupervisorStartupAuthorityV1 { user_sid })
    }
    #[cfg(not(windows))]
    {
        Err(SupervisorStartupErrorV1::AuthorityUnavailable)
    }
}

/// Classifies the one fixed task name using only native Task Scheduler COM.
/// It is read-only and does not create, update, delete, or launch anything.
pub(crate) fn classify_fixed_supervisor_startup_definition(
    authority: &SupervisorStartupAuthorityV1,
    definition: &FixedSupervisorStartupDefinitionV1,
) -> Result<SupervisorStartupDefinitionStateV1, SupervisorStartupErrorV1> {
    if authority.user_sid() != definition.user_sid {
        return Err(SupervisorStartupErrorV1::AuthorityUnavailable);
    }
    #[cfg(windows)]
    {
        native::classify(definition)
    }
    #[cfg(not(windows))]
    {
        let _ = definition;
        Err(SupervisorStartupErrorV1::NativeApiUnavailable)
    }
}

/// Creates only an absent fixed definition, or accepts an already exact one.
/// Foreign/malformed state is never overwritten.  Registration is immediately
/// read back through the same native classifier before this function succeeds.
pub(crate) fn register_exact_fixed_supervisor_startup_definition(
    authority: &SupervisorStartupAuthorityV1,
    definition: &FixedSupervisorStartupDefinitionV1,
) -> Result<(), SupervisorStartupErrorV1> {
    match classify_fixed_supervisor_startup_definition(authority, definition)? {
        SupervisorStartupDefinitionStateV1::ExactOwned => Ok(()),
        SupervisorStartupDefinitionStateV1::ForeignOrAmbiguous => {
            Err(SupervisorStartupErrorV1::ForeignOrAmbiguous)
        }
        SupervisorStartupDefinitionStateV1::Absent => {
            #[cfg(windows)]
            native::register_absent(definition)?;
            #[cfg(not(windows))]
            return Err(SupervisorStartupErrorV1::NativeApiUnavailable);
            match classify_fixed_supervisor_startup_definition(authority, definition)? {
                SupervisorStartupDefinitionStateV1::ExactOwned => Ok(()),
                _ => Err(SupervisorStartupErrorV1::PostRegistrationMismatch),
            }
        }
    }
}

/// Completes the post-install half of the bounded lifecycle transaction.
/// `previous` was classified as exact before the protected installer changed
/// the current receipt.  A changed action can be updated only after a second
/// native read proves the live task still equals that exact prior definition;
/// this is a narrow owned repair, never a blind overwrite of foreign state.
pub(crate) fn register_or_update_fixed_supervisor_startup_definition(
    authority: &SupervisorStartupAuthorityV1,
    previous: &FixedSupervisorStartupDefinitionV1,
    previous_state: SupervisorStartupDefinitionStateV1,
    next: &FixedSupervisorStartupDefinitionV1,
) -> Result<(), SupervisorStartupErrorV1> {
    if authority.user_sid() != previous.user_sid || authority.user_sid() != next.user_sid {
        return Err(SupervisorStartupErrorV1::AuthorityUnavailable);
    }
    match previous_state {
        SupervisorStartupDefinitionStateV1::ForeignOrAmbiguous => {
            Err(SupervisorStartupErrorV1::ForeignOrAmbiguous)
        }
        SupervisorStartupDefinitionStateV1::Absent => {
            register_exact_fixed_supervisor_startup_definition(authority, next)
        }
        SupervisorStartupDefinitionStateV1::ExactOwned if previous == next => {
            register_exact_fixed_supervisor_startup_definition(authority, next)
        }
        SupervisorStartupDefinitionStateV1::ExactOwned => {
            #[cfg(windows)]
            native::update_only_if_still_exact(previous, next)?;
            #[cfg(not(windows))]
            return Err(SupervisorStartupErrorV1::NativeApiUnavailable);
            match classify_fixed_supervisor_startup_definition(authority, next)? {
                SupervisorStartupDefinitionStateV1::ExactOwned => Ok(()),
                _ => Err(SupervisorStartupErrorV1::PostRegistrationMismatch),
            }
        }
    }
}

/// Narrow compensation used only when protected current receipt commit fails
/// after a disabled staged task was written. It first re-reads the task: an
/// Absent predecessor permits deletion only if the live task is still the
/// transaction's exact disabled definition; an Exact predecessor permits an
/// update only from that exact disabled definition back to the exact snapshot.
pub(crate) fn compensate_staged_fixed_supervisor_startup_definition(
    authority: &SupervisorStartupAuthorityV1,
    previous: &FixedSupervisorStartupDefinitionV1,
    previous_state: SupervisorStartupDefinitionStateV1,
    staged: &FixedSupervisorStartupDefinitionV1,
) -> Result<(), SupervisorStartupErrorV1> {
    if authority.user_sid() != staged.user_sid || authority.user_sid() != previous.user_sid {
        return Err(SupervisorStartupErrorV1::AuthorityUnavailable);
    }
    #[cfg(windows)]
    match previous_state {
        SupervisorStartupDefinitionStateV1::Absent => native::delete_only_if_still_exact(staged),
        SupervisorStartupDefinitionStateV1::ExactOwned => {
            native::update_only_if_still_exact(staged, previous)
        }
        SupervisorStartupDefinitionStateV1::ForeignOrAmbiguous => {
            Err(SupervisorStartupErrorV1::ForeignOrAmbiguous)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = previous_state;
        Err(SupervisorStartupErrorV1::NativeApiUnavailable)
    }
}

fn validate_interactive_user_sid(value: &str) -> Result<String, SupervisorStartupErrorV1> {
    if value.is_empty()
        || value.len() > 256
        || value == "S-1-5-18"
        || !value.starts_with("S-1-")
        || !value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(*byte, b'S' | b'-'))
    {
        return Err(SupervisorStartupErrorV1::AuthorityUnavailable);
    }
    Ok(value.to_owned())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_is_exact_owned(xml: &str, definition: &FixedSupervisorStartupDefinitionV1) -> bool {
    let Some(root) = parse_scheduler_task_xml(xml) else {
        return false;
    };
    if root.name != "Task" || root.children.len() != 5 {
        return false;
    }
    let Some(registration) = root.only_child("RegistrationInfo") else {
        return false;
    };
    let Some(triggers) = root.only_child("Triggers") else {
        return false;
    };
    let Some(principals) = root.only_child("Principals") else {
        return false;
    };
    let Some(settings) = root.only_child("Settings") else {
        return false;
    };
    let Some(actions) = root.only_child("Actions") else {
        return false;
    };

    // The generated Task Scheduler XML deliberately has a narrow semantic
    // whitelist.  Namespace prefix/attribute ordering is normalized by the
    // parser, but an extra node, trigger, principal, action, setting, working
    // directory, or unknown execution behavior is foreign/ambiguous.
    registration.exact_children(&[("URI", FIXED_SUPERVISOR_STARTUP_TASK_URI)])
        && triggers.children.len() == 1
        && triggers.children[0].name == "LogonTrigger"
        && triggers.children[0]
            .exact_children(&[("Enabled", "true"), ("UserId", &definition.user_sid)])
        && principals.children.len() == 1
        && principals.children[0].name == "Principal"
        && principals.children[0].exact_children(&[
            ("UserId", &definition.user_sid),
            ("LogonType", "InteractiveToken"),
            ("RunLevel", "LeastPrivilege"),
        ])
        && actions.attr("Context") == Some("CatDeskInteractiveUser")
        && actions.children.len() == 1
        && actions.children[0].name == "Exec"
        && actions.children[0].exact_children(&[
            ("Command", &definition.action_path),
            ("Arguments", FIXED_STABLE_SUPERVISOR_RUNTIME_FLAG),
        ])
        && settings.exact_children(&[
            ("Enabled", if definition.enabled { "true" } else { "false" }),
            ("MultipleInstancesPolicy", "IgnoreNew"),
            ("StartWhenAvailable", "false"),
            ("ExecutionTimeLimit", "PT0S"),
        ])
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SchedulerXmlNodeV1 {
    name: String,
    attributes: Vec<(String, String)>,
    text: String,
    children: Vec<Self>,
}

impl SchedulerXmlNodeV1 {
    fn attr(&self, wanted: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find_map(|(name, value)| (name == wanted).then_some(value.as_str()))
    }

    fn only_child(&self, wanted: &str) -> Option<&Self> {
        let mut found = self.children.iter().filter(|child| child.name == wanted);
        let child = found.next()?;
        found.next().is_none().then_some(child)
    }

    fn exact_children(&self, expected: &[(&str, &str)]) -> bool {
        self.children.len() == expected.len()
            && self
                .children
                .iter()
                .zip(expected)
                .all(|(child, (name, value))| {
                    child.name == *name
                        && child.children.is_empty()
                        && child.attributes.is_empty()
                        && child.text == *value
                })
    }
}

/// A deliberately small XML parser for the Task Scheduler document shape.
/// It supports namespace prefixes and benign whitespace/attribute ordering,
/// but rejects declarations other than the XML prolog, comments, CDATA, DTDs,
/// processing instructions, malformed nesting, duplicated nodes, and unknown
/// entities.  The ownership decision then uses the parsed tree above rather
/// than substring/count heuristics.
fn parse_scheduler_task_xml(input: &str) -> Option<SchedulerXmlNodeV1> {
    // The fixed definition uses the Scheduler default namespace.  A prefixed
    // document would require namespace-scope tracking to prove that each
    // local-name retains this exact namespace; accepting a prefix and then
    // erasing it would permit rebinding ambiguity.  Fail closed instead.
    if input.len() > 64 * 1024
        || input.contains("<!")
        || input.contains("<!--")
        || input.contains("xmlns:")
    {
        return None;
    }
    let mut cursor = 0usize;
    let mut stack: Vec<SchedulerXmlNodeV1> = Vec::new();
    let mut root = None;
    while cursor < input.len() {
        let rest = &input[cursor..];
        if let Some(text_end) = rest.find('<') {
            let text = &rest[..text_end];
            if !text.trim().is_empty() {
                let node = stack.last_mut()?;
                node.text.push_str(&xml_unescape(text.trim())?);
            }
            cursor += text_end;
        } else {
            return None;
        }
        let tail = &input[cursor..];
        let close = find_xml_tag_end(tail)?;
        let token = &tail[1..close];
        cursor += close + 1;
        if token.starts_with('?') {
            if token != "?xml version=\"1.0\" encoding=\"UTF-16\"?" {
                return None;
            }
            continue;
        }
        if let Some(raw) = token.strip_prefix('/') {
            let name = xml_local_name(raw.trim())?;
            let node = stack.pop()?;
            if node.name != name {
                return None;
            }
            if let Some(parent) = stack.last_mut() {
                parent.children.push(node);
            } else if root.replace(node).is_some() {
                return None;
            }
            continue;
        }
        let self_closing = token.trim_end().ends_with('/');
        let token = token.trim_end().trim_end_matches('/').trim();
        let (name, attributes) = parse_xml_open_tag(token)?;
        let node = SchedulerXmlNodeV1 {
            name,
            attributes,
            text: String::new(),
            children: Vec::new(),
        };
        if self_closing {
            if let Some(parent) = stack.last_mut() {
                parent.children.push(node);
            } else if root.replace(node).is_some() {
                return None;
            }
        } else {
            stack.push(node);
        }
    }
    if !stack.is_empty() {
        return None;
    }
    root
}

fn find_xml_tag_end(input: &str) -> Option<usize> {
    let mut quoted = None;
    for (index, character) in input.char_indices().skip(1) {
        match (quoted, character) {
            (None, '\'' | '\"') => quoted = Some(character),
            (Some(quote), character) if quote == character => quoted = None,
            (None, '>') => return Some(index),
            _ => {}
        }
    }
    None
}

fn parse_xml_open_tag(input: &str) -> Option<(String, Vec<(String, String)>)> {
    let mut pieces = input.split_whitespace();
    let name = xml_local_name(pieces.next()?)?;
    let mut attributes = Vec::new();
    let first_space = input
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index))
        .unwrap_or(input.len());
    let mut remaining = input.get(first_space..)?.trim();
    while !remaining.is_empty() {
        if remaining.is_empty() {
            break;
        }
        let equal = remaining.find('=')?;
        let raw_name = remaining[..equal].trim();
        let after_equal = remaining[equal + 1..].trim_start();
        let quote = after_equal.chars().next()?;
        if !matches!(quote, '\'' | '\"') {
            return None;
        }
        let end = after_equal[1..].find(quote)? + 1;
        let raw_value = &after_equal[1..end];
        // Attribute local-name normalization must never collapse duplicates
        // (`x:Enabled` and `Enabled`, or repeated `Enabled`) into one benign
        // looking setting.  Namespace-prefixed attributes are not part of the
        // closed task grammar, including namespace rebinding declarations.
        if raw_name.contains(':') {
            return None;
        }
        let name = xml_local_name(raw_name)?;
        if attributes.iter().any(|(existing, _)| existing == &name) {
            return None;
        }
        attributes.push((name, xml_unescape(raw_value)?));
        remaining = after_equal.get(end + 1..)?.trim_start();
    }
    attributes.sort();
    Some((name, attributes))
}

fn xml_local_name(value: &str) -> Option<String> {
    let value = value.trim();
    // Prefixes are rejected by the closed parser instead of being normalized
    // away.  This makes namespace rebinding and prefix collisions fail closed.
    if value.contains(':') {
        return None;
    }
    let local = value;
    (!local.is_empty()
        && local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then(|| local.to_owned())
}

fn xml_unescape(value: &str) -> Option<String> {
    let mut value = value.replace("&amp;", "&");
    value = value.replace("&lt;", "<");
    value = value.replace("&gt;", ">");
    value = value.replace("&quot;", "\"");
    value = value.replace("&apos;", "'");
    (!value.contains('&')).then_some(value)
}

/// The native API exposes access denial as an HRESULT.  The lifecycle keeps
/// this operational condition distinct from malformed/foreign task evidence:
/// an elevation-required fixed operation is surfaced to the operator boundary
/// without trying a service, shell, or alternate persistence mechanism.
fn native_hresult_reason(
    status: i32,
    fallback: SupervisorStartupErrorV1,
) -> SupervisorStartupErrorV1 {
    const E_ACCESSDENIED: i32 = 0x8007_0005_u32 as i32;
    if status == E_ACCESSDENIED {
        SupervisorStartupErrorV1::ElevationRequired
    } else {
        fallback
    }
}

#[cfg(windows)]
mod native {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    use super::{
        FIXED_SUPERVISOR_STARTUP_TASK_NAME, FixedSupervisorStartupDefinitionV1,
        SupervisorStartupDefinitionStateV1, SupervisorStartupErrorV1, xml_is_exact_owned,
    };

    const SCHED_E_TASK_NOT_FOUND: i32 = 0x8004_130F_u32 as i32;
    const TASK_CREATE: i32 = 0x2;
    const TASK_UPDATE: i32 = 0x4;
    const TASK_LOGON_INTERACTIVE_TOKEN: i32 = 3;
    const CLSCTX_INPROC_SERVER: u32 = 0x1;
    const COINIT_MULTITHREADED: u32 = 0;

    // COM slots include IUnknown (0..=2) and IDispatch (3..=6). These are
    // named rather than scattered literals so SDK inheritance cannot silently
    // shift an authority-bearing call. Taskschd.h: ITaskService::GetFolder=7,
    // GetRunningTasks=8, NewTask=9, Connect=10; ITaskFolder::GetTask=13 and
    // RegisterTask=16; IRegisteredTask::get_Xml=20.
    const I_TASK_SERVICE_GET_FOLDER_SLOT: usize = 7;
    const I_TASK_SERVICE_CONNECT_SLOT: usize = 10;
    const I_TASK_FOLDER_GET_TASK_SLOT: usize = 13;
    const I_TASK_FOLDER_DELETE_TASK_SLOT: usize = 15;
    const I_TASK_FOLDER_REGISTER_TASK_SLOT: usize = 16;
    const I_REGISTERED_TASK_GET_XML_SLOT: usize = 20;
    const I_UNKNOWN_RELEASE_SLOT: usize = 2;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    const CLSID_TASK_SCHEDULER: Guid = Guid {
        data1: 0x0f87_369f,
        data2: 0xa4e5,
        data3: 0x4cfc,
        data4: [0xbd, 0x3e, 0x73, 0xe6, 0x15, 0x45, 0x72, 0xdd],
    };
    const IID_ITASK_SERVICE: Guid = Guid {
        data1: 0x2fab_a4c7,
        data2: 0x4da9,
        data3: 0x4013,
        data4: [0x96, 0x97, 0x20, 0xcc, 0x3f, 0xd4, 0x0f, 0x85],
    };

    // `VARIANT` is passed **by value** by ITaskService::Connect and the
    // userId/password/sddl arguments of ITaskFolder::RegisterTask.  The
    // Windows SDK layout is an 8-byte VARTYPE/reserved header followed by an
    // 8-byte discriminated union on both supported x86 and x64 Windows ABIs.
    // Keeping the largest scalar member prevents the old, incorrect
    // pointer-only 12-byte x86 representation.
    #[repr(C)]
    #[derive(Clone, Copy)]
    union VariantValue {
        bstr: *mut u16,
        pointer: *mut c_void,
        long_long: i64,
        unsigned_long_long: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Variant {
        vt: u16,
        reserved1: u16,
        reserved2: u16,
        reserved3: u16,
        value: VariantValue,
    }

    impl Variant {
        const fn empty() -> Self {
            Self {
                vt: 0,
                reserved1: 0,
                reserved2: 0,
                reserved3: 0,
                value: VariantValue {
                    pointer: std::ptr::null_mut(),
                },
            }
        }
        fn bstr(value: &Bstr) -> Self {
            Self {
                vt: 8,
                reserved1: 0,
                reserved2: 0,
                reserved3: 0,
                value: VariantValue { bstr: value.0 },
            }
        }
    }

    struct Bstr(*mut u16);
    impl Bstr {
        fn new(value: &str) -> Result<Self, SupervisorStartupErrorV1> {
            let wide: Vec<u16> = std::ffi::OsStr::new(value).encode_wide().collect();
            let raw = unsafe { SysAllocStringLen(wide.as_ptr(), wide.len() as u32) };
            if raw.is_null() {
                Err(SupervisorStartupErrorV1::NativeApiUnavailable)
            } else {
                Ok(Self(raw))
            }
        }
        fn as_ptr(&self) -> *mut u16 {
            self.0
        }
        unsafe fn into_string(raw: *mut u16) -> String {
            if raw.is_null() {
                return String::new();
            }
            let len = unsafe { SysStringLen(raw) } as usize;
            let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(raw, len) });
            unsafe { SysFreeString(raw) };
            text
        }
    }
    impl Drop for Bstr {
        fn drop(&mut self) {
            unsafe { SysFreeString(self.0) };
        }
    }

    struct ComApartment(bool);
    impl ComApartment {
        fn initialize() -> Result<Self, SupervisorStartupErrorV1> {
            let hr = unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED) };
            if hr >= 0 {
                Ok(Self(true))
            } else {
                Err(SupervisorStartupErrorV1::NativeApiUnavailable)
            }
        }
    }
    impl Drop for ComApartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() }
            }
        }
    }

    struct ComPtr(*mut c_void);
    impl Drop for ComPtr {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { release(self.0) };
            }
        }
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
        fn CoUninitialize();
        fn CoCreateInstance(
            clsid: *const Guid,
            outer: *mut c_void,
            context: u32,
            iid: *const Guid,
            out: *mut *mut c_void,
        ) -> i32;
    }
    #[link(name = "oleaut32")]
    unsafe extern "system" {
        fn SysAllocStringLen(source: *const u16, length: u32) -> *mut u16;
        fn SysFreeString(value: *mut u16);
        fn SysStringLen(value: *mut u16) -> u32;
    }

    unsafe fn method<T: Copy>(object: *mut c_void, index: usize) -> T {
        let vtable = unsafe { *(object as *mut *mut *const c_void) };
        let entry = unsafe { *vtable.add(index) };
        unsafe { std::mem::transmute_copy(&entry) }
    }
    type IUnknownRelease = unsafe extern "system" fn(*mut c_void) -> u32;
    type ITaskServiceConnect =
        unsafe extern "system" fn(*mut c_void, Variant, Variant, Variant, Variant) -> i32;
    type ITaskServiceGetFolder =
        unsafe extern "system" fn(*mut c_void, *mut u16, *mut *mut c_void) -> i32;
    type ITaskFolderGetTask =
        unsafe extern "system" fn(*mut c_void, *mut u16, *mut *mut c_void) -> i32;
    type ITaskFolderRegisterTask = unsafe extern "system" fn(
        *mut c_void,
        *mut u16,
        *mut u16,
        i32,
        Variant,
        Variant,
        i32,
        Variant,
        *mut *mut c_void,
    ) -> i32;
    type ITaskFolderDeleteTask = unsafe extern "system" fn(*mut c_void, *mut u16, i32) -> i32;
    type IRegisteredTaskGetXml = unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32;

    unsafe fn release(object: *mut c_void) {
        let f: IUnknownRelease = unsafe { method(object, I_UNKNOWN_RELEASE_SLOT) };
        unsafe { f(object) };
    }

    fn connect_service() -> Result<(ComApartment, ComPtr), SupervisorStartupErrorV1> {
        let apartment = ComApartment::initialize()?;
        let mut raw = std::ptr::null_mut();
        let hr = unsafe {
            CoCreateInstance(
                &CLSID_TASK_SCHEDULER,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_ITASK_SERVICE,
                &mut raw,
            )
        };
        if hr < 0 || raw.is_null() {
            return Err(SupervisorStartupErrorV1::NativeApiUnavailable);
        }
        let service = ComPtr(raw);
        let empty = Variant::empty();
        let connect: ITaskServiceConnect =
            unsafe { method(service.0, I_TASK_SERVICE_CONNECT_SLOT) };
        if unsafe { connect(service.0, empty, empty, empty, empty) } < 0 {
            return Err(SupervisorStartupErrorV1::NativeApiUnavailable);
        }
        Ok((apartment, service))
    }

    fn root_folder(service: &ComPtr) -> Result<ComPtr, SupervisorStartupErrorV1> {
        let root = Bstr::new("\\")?;
        let mut raw = std::ptr::null_mut();
        let get_folder: ITaskServiceGetFolder =
            unsafe { method(service.0, I_TASK_SERVICE_GET_FOLDER_SLOT) };
        if unsafe { get_folder(service.0, root.as_ptr(), &mut raw) } < 0 || raw.is_null() {
            return Err(SupervisorStartupErrorV1::NativeApiUnavailable);
        }
        Ok(ComPtr(raw))
    }

    fn existing_task_xml(folder: &ComPtr) -> Result<Option<String>, SupervisorStartupErrorV1> {
        let name = Bstr::new(FIXED_SUPERVISOR_STARTUP_TASK_NAME)?;
        let mut task_raw = std::ptr::null_mut();
        let get_task: ITaskFolderGetTask = unsafe { method(folder.0, I_TASK_FOLDER_GET_TASK_SLOT) };
        let hr = unsafe { get_task(folder.0, name.as_ptr(), &mut task_raw) };
        if hr == SCHED_E_TASK_NOT_FOUND {
            return Ok(None);
        }
        if hr < 0 || task_raw.is_null() {
            return Err(SupervisorStartupErrorV1::NativeApiUnavailable);
        }
        let task = ComPtr(task_raw);
        let mut xml = std::ptr::null_mut();
        let get_xml: IRegisteredTaskGetXml =
            unsafe { method(task.0, I_REGISTERED_TASK_GET_XML_SLOT) };
        if unsafe { get_xml(task.0, &mut xml) } < 0 || xml.is_null() {
            return Err(SupervisorStartupErrorV1::NativeApiUnavailable);
        }
        Ok(Some(unsafe { Bstr::into_string(xml) }))
    }

    pub(super) fn classify(
        definition: &FixedSupervisorStartupDefinitionV1,
    ) -> Result<SupervisorStartupDefinitionStateV1, SupervisorStartupErrorV1> {
        let (_apartment, service) = connect_service()?;
        let folder = root_folder(&service)?;
        match existing_task_xml(&folder)? {
            None => Ok(SupervisorStartupDefinitionStateV1::Absent),
            Some(xml) if xml_is_exact_owned(&xml, definition) => {
                Ok(SupervisorStartupDefinitionStateV1::ExactOwned)
            }
            Some(_) => Ok(SupervisorStartupDefinitionStateV1::ForeignOrAmbiguous),
        }
    }

    fn register_definition(
        folder: &ComPtr,
        definition: &FixedSupervisorStartupDefinitionV1,
        flags: i32,
    ) -> Result<(), SupervisorStartupErrorV1> {
        let name = Bstr::new(FIXED_SUPERVISOR_STARTUP_TASK_NAME)?;
        let xml = Bstr::new(&definition.task_xml())?;
        let user = Bstr::new(&definition.user_sid)?;
        let user_variant = Variant::bstr(&user);
        let empty = Variant::empty();
        let mut registered = std::ptr::null_mut();
        let register: ITaskFolderRegisterTask =
            unsafe { method(folder.0, I_TASK_FOLDER_REGISTER_TASK_SLOT) };
        let hr = unsafe {
            register(
                folder.0,
                name.as_ptr(),
                xml.as_ptr(),
                flags,
                user_variant,
                empty,
                TASK_LOGON_INTERACTIVE_TOKEN,
                empty,
                &mut registered,
            )
        };
        let _registered = ComPtr(registered);
        if hr < 0 || registered.is_null() {
            return Err(super::native_hresult_reason(
                hr,
                SupervisorStartupErrorV1::RegistrationFailed,
            ));
        }
        Ok(())
    }

    pub(super) fn register_absent(
        definition: &FixedSupervisorStartupDefinitionV1,
    ) -> Result<(), SupervisorStartupErrorV1> {
        let (_apartment, service) = connect_service()?;
        let folder = root_folder(&service)?;
        // Recheck immediately before TASK_CREATE. A concurrent creator causes
        // TASK_CREATE to fail; there is no combined create-or-overwrite path.
        if existing_task_xml(&folder)?.is_some() {
            return Err(SupervisorStartupErrorV1::ForeignOrAmbiguous);
        }
        register_definition(&folder, definition, TASK_CREATE)
    }

    pub(super) fn update_only_if_still_exact(
        previous: &FixedSupervisorStartupDefinitionV1,
        next: &FixedSupervisorStartupDefinitionV1,
    ) -> Result<(), SupervisorStartupErrorV1> {
        let (_apartment, service) = connect_service()?;
        let folder = root_folder(&service)?;
        match existing_task_xml(&folder)? {
            Some(xml) if xml_is_exact_owned(&xml, previous) => {
                register_definition(&folder, next, TASK_UPDATE)
            }
            _ => Err(SupervisorStartupErrorV1::ForeignOrAmbiguous),
        }
    }

    pub(super) fn delete_only_if_still_exact(
        staged: &FixedSupervisorStartupDefinitionV1,
    ) -> Result<(), SupervisorStartupErrorV1> {
        let (_apartment, service) = connect_service()?;
        let folder = root_folder(&service)?;
        match existing_task_xml(&folder)? {
            Some(xml) if xml_is_exact_owned(&xml, staged) => {
                let name = Bstr::new(FIXED_SUPERVISOR_STARTUP_TASK_NAME)?;
                let delete: ITaskFolderDeleteTask =
                    unsafe { method(folder.0, I_TASK_FOLDER_DELETE_TASK_SLOT) };
                if unsafe { delete(folder.0, name.as_ptr(), 0) } < 0 {
                    return Err(SupervisorStartupErrorV1::RegistrationFailed);
                }
                match existing_task_xml(&folder)? {
                    None => Ok(()),
                    Some(_) => Err(SupervisorStartupErrorV1::PostRegistrationMismatch),
                }
            }
            _ => Err(SupervisorStartupErrorV1::ForeignOrAmbiguous),
        }
    }

    #[cfg(test)]
    mod abi_tests {
        use super::*;

        #[derive(Default)]
        struct FakeComVtable {
            calls: Vec<usize>,
            next_hresult: i32,
        }

        impl FakeComVtable {
            fn dispatch(&mut self, slot: usize) -> i32 {
                self.calls.push(slot);
                self.next_hresult
            }
        }

        #[test]
        fn sdk_inherited_slots_are_exact_and_fake_dispatch_preserves_hresult() {
            assert_eq!(I_TASK_SERVICE_GET_FOLDER_SLOT, 7);
            assert_eq!(I_TASK_SERVICE_CONNECT_SLOT, 10);
            assert_eq!(I_TASK_FOLDER_GET_TASK_SLOT, 13);
            assert_eq!(I_TASK_FOLDER_DELETE_TASK_SLOT, 15);
            assert_eq!(I_TASK_FOLDER_REGISTER_TASK_SLOT, 16);
            assert_eq!(I_REGISTERED_TASK_GET_XML_SLOT, 20);
            assert_eq!(I_UNKNOWN_RELEASE_SLOT, 2);

            let mut fake = FakeComVtable {
                next_hresult: -7,
                ..Default::default()
            };
            assert_eq!(fake.dispatch(I_TASK_SERVICE_CONNECT_SLOT), -7);
            assert_eq!(fake.dispatch(I_TASK_SERVICE_GET_FOLDER_SLOT), -7);
            assert_eq!(fake.dispatch(I_TASK_FOLDER_GET_TASK_SLOT), -7);
            assert_eq!(fake.dispatch(I_TASK_FOLDER_REGISTER_TASK_SLOT), -7);
            assert_eq!(fake.dispatch(I_REGISTERED_TASK_GET_XML_SLOT), -7);
            assert_eq!(
                fake.calls,
                vec![7 + 3, 7, 13, 16, 20],
                "fake boundary records the SDK inherited dispatch order",
            );
        }

        #[test]
        fn named_com_function_types_are_abi_checked_by_assignment() {
            let _: Option<IUnknownRelease> = None;
            let _: Option<ITaskServiceConnect> = None;
            let _: Option<ITaskServiceGetFolder> = None;
            let _: Option<ITaskFolderGetTask> = None;
            let _: Option<ITaskFolderDeleteTask> = None;
            let _: Option<ITaskFolderRegisterTask> = None;
            let _: Option<IRegisteredTaskGetXml> = None;
        }

        #[test]
        fn variant_layout_and_by_value_scheduler_signatures_are_exact() {
            use std::mem::{align_of, size_of};

            unsafe extern "system" fn connect_by_value(
                _: *mut c_void,
                _: Variant,
                _: Variant,
                _: Variant,
                _: Variant,
            ) -> i32 {
                0
            }
            unsafe extern "system" fn register_by_value(
                _: *mut c_void,
                _: *mut u16,
                _: *mut u16,
                _: i32,
                _: Variant,
                _: Variant,
                _: i32,
                _: Variant,
                _: *mut *mut c_void,
            ) -> i32 {
                0
            }

            // VARIANT is 16 bytes on supported Windows x86/x64 ABIs: four
            // u16 header fields plus an 8-byte union.  The assertion is a
            // native-boundary regression, not a source-string heuristic.
            assert_eq!(size_of::<VariantValue>(), 8);
            assert_eq!(size_of::<Variant>(), 16);
            assert!(align_of::<Variant>() >= 8);

            let _: ITaskServiceConnect = connect_by_value;
            let _: ITaskFolderRegisterTask = register_by_value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_accepts_only_non_system_interactive_sid_shape() {
        assert!(
            SupervisorStartupAuthorityV1::test_only_interactive_user("S-1-5-21-123-456-789-1001")
                .is_ok()
        );
        for value in ["", "S-1-5-18", "S-1-5-21-x", "not-a-sid"] {
            assert!(SupervisorStartupAuthorityV1::test_only_interactive_user(value).is_err());
        }
    }

    #[test]
    fn fixed_definition_has_only_interactive_least_privilege_singleton_action() {
        let authority =
            SupervisorStartupAuthorityV1::test_only_interactive_user("S-1-5-21-1-2-3-1001")
                .unwrap();
        let definition = FixedSupervisorStartupDefinitionV1::test_only_for_action(&authority, r"C:\ProgramData\CatDesk\ControlPlaneSupervisor\versions\abc\catdesk-control-plane-supervisor.exe").unwrap();
        let xml = definition.task_xml_for_test();
        assert!(xml_is_exact_owned(&xml, &definition));
        for required in [
            FIXED_SUPERVISOR_STARTUP_TASK_URI,
            "InteractiveToken",
            "LeastPrivilege",
            FIXED_STABLE_SUPERVISOR_RUNTIME_FLAG,
        ] {
            assert!(xml.contains(required));
        }
        for forbidden in [
            "sessionId",
            "Password",
            "HighestAvailable",
            "LocalSystem",
            "<ComHandler",
        ] {
            assert!(!xml.contains(forbidden));
        }
    }

    #[test]
    fn malformed_or_foreign_xml_is_not_exact_owned() {
        let authority =
            SupervisorStartupAuthorityV1::test_only_interactive_user("S-1-5-21-1-2-3-1001")
                .unwrap();
        let definition =
            FixedSupervisorStartupDefinitionV1::test_only_for_action(&authority, "fixture.exe")
                .unwrap();
        let exact = definition.task_xml();
        assert!(xml_is_exact_owned(&exact, &definition));
        for replacement in [
            exact.replacen("InteractiveToken", "Password", 1),
            exact.replacen("LeastPrivilege", "HighestAvailable", 1),
            exact.replacen(FIXED_STABLE_SUPERVISOR_RUNTIME_FLAG, "--other", 1),
        ] {
            assert!(!xml_is_exact_owned(&replacement, &definition));
        }
    }

    #[test]
    fn semantic_classifier_refuses_every_widened_trigger_action_principal_and_setting() {
        let authority =
            SupervisorStartupAuthorityV1::test_only_interactive_user("S-1-5-21-1-2-3-1001")
                .unwrap();
        let definition = FixedSupervisorStartupDefinitionV1::test_only_for_action(
            &authority,
            r"C:\ProgramData\CatDesk\ControlPlaneSupervisor\versions\0123456789abcdef\catdesk-control-plane-supervisor.exe",
        )
        .unwrap();
        let exact = definition.task_xml();
        let mutations = [
            (
                "<TimeTrigger><Enabled>true</Enabled></TimeTrigger>",
                "<Triggers>",
            ),
            (
                "<BootTrigger><Enabled>true</Enabled></BootTrigger>",
                "<Triggers>",
            ),
            (
                "<EventTrigger><Enabled>true</Enabled></EventTrigger>",
                "<Triggers>",
            ),
            (
                "<LogonTrigger><Enabled>true</Enabled><UserId>S-1-5-21-other</UserId></LogonTrigger>",
                "</Triggers>",
            ),
            (
                "<Principal><UserId>S-1-5-21-other</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal>",
                "</Principals>",
            ),
            (
                "<ComHandler><ClassId>{foreign}</ClassId></ComHandler>",
                "</Actions>",
            ),
            (
                "<Exec><Command>foreign.exe</Command><Arguments>--catdesk-control-plane-supervisor</Arguments></Exec>",
                "</Actions>",
            ),
            (
                "<WorkingDirectory>C:\\foreign</WorkingDirectory>",
                "</Exec>",
            ),
            ("<Enabled>true</Enabled>", "</Settings>"),
        ];
        for (insertion, marker) in mutations {
            let widened = exact.replacen(marker, &format!("{insertion}{marker}"), 1);
            assert!(
                !xml_is_exact_owned(&widened, &definition),
                "widened XML must be foreign: {insertion}"
            );
        }
        assert!(!xml_is_exact_owned("<Task><broken></Task>", &definition));
        let namespace_ambiguous = exact
            .replace("<Task ", "<t:Task ")
            .replace("</Task>", "</t:Task>");
        assert!(
            !xml_is_exact_owned(&namespace_ambiguous, &definition),
            "a prefix cannot be normalized away without proving its namespace binding"
        );
    }

    #[test]
    fn semantic_classifier_refuses_duplicate_attributes_and_namespace_ambiguity() {
        let authority =
            SupervisorStartupAuthorityV1::test_only_interactive_user("S-1-5-21-1-2-3-1001")
                .unwrap();
        let definition =
            FixedSupervisorStartupDefinitionV1::test_only_for_action(&authority, "fixture.exe")
                .unwrap();
        let exact = definition.task_xml();
        for malformed in [
            exact.replacen(
                "<Actions Context=\"CatDeskInteractiveUser\"",
                "<Actions Context=\"CatDeskInteractiveUser\" Context=\"other\"",
                1,
            ),
            exact.replacen("<Settings>", "<Settings xmlns:x=\"urn:foreign\">", 1),
            exact.replacen("<Arguments>", "<Arguments><Arguments>", 1),
            exact.replacen(
                "</Exec>",
                "<WorkingDirectory>C:\\foreign</WorkingDirectory></Exec>",
                1,
            ),
        ] {
            assert!(
                !xml_is_exact_owned(&malformed, &definition),
                "semantic ambiguity must not normalize to exact ownership"
            );
        }
    }

    #[test]
    fn startup_source_has_no_shell_service_or_caller_selected_authority() {
        let source = include_str!("windows_supervisor_startup.rs");
        for forbidden in [
            ["Power", "Shell"].concat(),
            ["scht", "asks"].concat(),
            ["Command", "::new"].concat(),
            ["Create", "ServiceW"].concat(),
            ["current", "_exe"].concat(),
            ["target", "/release"].concat(),
            ["Run", " key"].concat(),
            ["Startup", " folder"].concat(),
        ] {
            assert!(
                !source.contains(&forbidden),
                "forbidden startup authority: {forbidden}"
            );
        }
        assert!(source.contains("TASK_LOGON_INTERACTIVE_TOKEN"));
        assert!(source.contains("TASK_CREATE"));
        assert!(!source.contains(&["TASK_CREATE", "_OR_UPDATE"].concat()));
    }

    #[test]
    fn access_denied_is_a_distinct_operator_elevation_category() {
        assert_eq!(
            native_hresult_reason(
                0x8007_0005_u32 as i32,
                SupervisorStartupErrorV1::RegistrationFailed
            ),
            SupervisorStartupErrorV1::ElevationRequired
        );
        assert_eq!(
            native_hresult_reason(-1, SupervisorStartupErrorV1::RegistrationFailed),
            SupervisorStartupErrorV1::RegistrationFailed
        );
    }
}
