//! Immutable, explicitly authenticated project connections. The local operator
//! selects a registered project; remote requests cannot select a workspace.
//! Mutations require a separate local task assignment and isolated worktree.

use crate::{
    delegated::autonomy_projects::{
        ResolvedProjectScopeV1, resolve_registered_project_from_control_root,
    },
    mcp::{self, JsonRpcRequest, JsonRpcResponse},
    state::{Mode, ToolMode},
    web_conversation_context::{ConversationKey, Observation, VisibleSourceKind, WebContextStore},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

const SCOPE_TOOL: &str = "catdesk_project_scope";
const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_TRAFFIC_BYTES: usize = MAX_REQUEST_BYTES;
// Bound each tokenizer input, including pathological text without separators.
// The full 2MiB prefix plus one unknown marker still fits one 512-event batch.
const TRAFFIC_CHUNK_BYTES: usize = 8 * 1024;

/// One actual HTTP invocation, not an RPC ID, content hash or task ID. A
/// transport retry may be a second provider-visible exchange; it is never
/// silently merged with a legitimate equal call. Re-recording a part of this
/// same invocation is idempotent in the existing context ledger.
pub(crate) struct TrafficObservation {
    control: PathBuf,
    key: Option<ConversationKey>,
    invocation: String,
    arguments_status: &'static str,
}

struct TrafficText {
    bytes: Vec<u8>,
    incomplete: bool,
}

impl Write for TrafficText {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let retained = bytes.len().min(MAX_TRAFFIC_BYTES - self.bytes.len());
        self.bytes.extend_from_slice(&bytes[..retained]);
        self.incomplete |= retained != bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl TrafficText {
    fn from_value(value: &Value, incomplete: bool) -> Self {
        let mut text = Self {
            bytes: Vec::new(),
            incomplete,
        };
        if serde_json::to_writer(&mut text, value).is_err() {
            text.incomplete = true;
        }
        if let Err(error) = std::str::from_utf8(&text.bytes) {
            text.bytes.truncate(error.valid_up_to());
            text.incomplete = true;
        }
        text
    }

    fn record(
        &self,
        store: &WebContextStore,
        key: &ConversationKey,
        invocation: &str,
        part: &str,
        kind: VisibleSourceKind,
    ) -> Result<(), String> {
        let text =
            std::str::from_utf8(&self.bytes).map_err(|_| "WEB_CONTEXT_EVENT_CONTENT_INVALID")?;
        let mut chunks = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            let mut length = rest.len().min(TRAFFIC_CHUNK_BYTES);
            while !rest.is_char_boundary(length) {
                length -= 1;
            }
            chunks.push(&rest[..length]);
            rest = &rest[length..];
        }
        let ids = (0..chunks.len())
            .map(|index| format!("{invocation}:{part}:{index}"))
            .collect::<Vec<_>>();
        let unknown_id = format!("{invocation}:{part}:unknown");
        let observed = catdesk_wake::store::now();
        let mut events = chunks
            .iter()
            .zip(&ids)
            .map(|(chunk, id)| Observation {
                event_id: id,
                revision: 1,
                dedup_key: id,
                source_kind: kind,
                visible_text: Some(chunk),
                observed_at_unix: observed,
            })
            .collect::<Vec<_>>();
        if self.incomplete {
            events.push(Observation {
                event_id: &unknown_id,
                revision: 1,
                dedup_key: &unknown_id,
                source_kind: VisibleSourceKind::UnknownAttachment,
                visible_text: None,
                observed_at_unix: observed,
            });
        }
        if !events.is_empty() {
            store.record_existing_conversation_batch(key, events)?;
        }
        Ok(())
    }
}

fn traffic_status(result: Result<(), String>) -> &'static str {
    match result.as_ref().map_err(String::as_str) {
        Ok(()) => "RECORDED",
        Err("WEB_CONTEXT_BUSY" | "WEB_CONTEXT_REGISTRATION_BUSY") => "BUSY",
        Err("WEB_CONTEXT_OLD_GENERATION" | "WEB_CONTEXT_TARGET_CHANGED") => "TARGET_CHANGED",
        Err(_) => "UNAVAILABLE",
    }
}

fn response_traffic(response: &JsonRpcResponse) -> TrafficText {
    let mut value = serde_json::Map::new();
    let mut incomplete = false;
    if let Some(error) = &response.error {
        value.insert(
            "error".into(),
            json!({"code":error.code,"message":error.message}),
        );
    }
    if let Some(result) = &response.result {
        if let Some(result) = result.as_object() {
            let mut visible = serde_json::Map::new();
            if let Some(content) = result.get("content") {
                let content = content.as_array().map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|block| {
                            if block["type"] == "text" && block["text"].is_string() {
                                Some(json!({"type":"text","text":block["text"]}))
                            } else {
                                // Image/audio/resource payloads are not text tokens.
                                // Retain uncertainty without tokenizing binary data.
                                incomplete = true;
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                });
                match content {
                    Some(content) => {
                        visible.insert("content".into(), json!(content));
                    }
                    None => incomplete = true,
                }
            }
            for field in ["structuredContent", "isError"] {
                if let Some(data) = result.get(field) {
                    visible.insert(field.into(), data.clone());
                }
            }
            value.insert("result".into(), Value::Object(visible));
        } else {
            value.insert("result".into(), result.clone());
        }
    }
    TrafficText::from_value(&Value::Object(value), incomplete)
}

impl TrafficObservation {
    async fn start(context: &ProjectContext, current: &Selection, params: Value) -> Self {
        Self::start_scoped(
            context.registered.control_workspace.clone(),
            current,
            params,
        )
        .await
    }

    /// Called only after exact project/task-key admission. This observes traffic;
    /// it does not bind a worker, grant execution or attest provider identity.
    pub(crate) async fn start_scoped(control: PathBuf, current: &Selection, params: Value) -> Self {
        let mut observation = Self {
            control: control.clone(),
            key: None,
            invocation: format!("mcp-{}", uuid::Uuid::new_v4().simple()),
            arguments_status: "UNAVAILABLE",
        };
        let invocation = observation.invocation.clone();
        let current = current.clone();
        let started = tokio::task::spawn_blocking(move || {
            let text = TrafficText::from_value(&params, false);
            let (key, bound) = mcp::with_current_project_web_target(
                &control,
                &current.project_id,
                current.target_generation,
                &current.target_sha256,
                |control, target| {
                    let key = ConversationKey {
                        project_id: current.project_id.clone(),
                        conversation_url: target.url.clone(),
                        target_generation: target.generation,
                    };
                    let bound = (|| {
                        let store = WebContextStore::open_nonblocking(
                            control.join(".catdesk/web-context"),
                        )?;
                        let previous = store.active_key(&key.project_id)?;
                        store.bind_observed_target(key.clone(), previous.as_ref())
                    })();
                    Ok((key, bound))
                },
            )?;
            // Tokenization and ledger recording retain no registry/Wake lock.
            // A concurrent rollover can only move this exact key into history.
            let status = traffic_status(bound.and_then(|()| {
                WebContextStore::open_nonblocking(control.join(".catdesk/web-context")).and_then(
                    |store| {
                        text.record(
                            &store,
                            &key,
                            &invocation,
                            "arguments",
                            VisibleSourceKind::ToolArguments,
                        )
                    },
                )
            }));
            Ok::<_, String>((key, status))
        })
        .await;
        match started {
            Ok(Ok((key, status))) => {
                observation.key = Some(key);
                observation.arguments_status = status;
            }
            Ok(Err(reason)) => observation.arguments_status = traffic_status(Err(reason)),
            Err(_) => {}
        }
        observation
    }

    pub(crate) async fn finish(self, response: &mut JsonRpcResponse) {
        let mut result_status = "UNAVAILABLE";
        if let Some(key) = self.key {
            let text = response_traffic(response);
            result_status = tokio::task::spawn_blocking(move || {
                traffic_status(
                    WebContextStore::open_nonblocking(self.control.join(".catdesk/web-context"))
                        .and_then(|store| {
                            text.record(
                                &store,
                                &key,
                                &self.invocation,
                                "result",
                                VisibleSourceKind::ToolResult,
                            )
                        }),
                )
            })
            .await
            .unwrap_or("UNAVAILABLE");
        }
        if let Some(result) = response.result.as_mut().and_then(Value::as_object_mut) {
            let meta = result.entry("_meta").or_insert_with(|| json!({}));
            if let Some(meta) = meta.as_object_mut() {
                meta.insert(
                    "catdeskContextObservation".into(),
                    json!({
                    "arguments":self.arguments_status,"result":result_status,
                    "source":"PROJECT_CONNECTION_TRAFFIC","rawTextPersisted":false,
                    "advisory":true,"originAuthenticated":false,"providerReceiptVerified":false,
                    "possibleRetryOrSurfaceOverlap":true}),
                );
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub project_id: String,
    pub target_generation: u64,
    pub target_sha256: String,
}

pub(crate) struct ProjectContext {
    registered: ResolvedProjectScopeV1,
    selection: Selection,
    task: Option<Arc<crate::project_mcp_task::ProjectTask>>,
}

impl ProjectContext {
    #[cfg(test)]
    pub async fn bind(control: &Path, selection: Selection) -> Result<Arc<Self>, String> {
        Self::bind_with_task(control, selection, None, 180).await
    }

    pub async fn bind_with_task(
        control: &Path,
        selection: Selection,
        assignment: Option<&Path>,
        connection_seconds: u64,
    ) -> Result<Arc<Self>, String> {
        if selection.target_generation == 0
            || selection.target_sha256.len() != 64
            || !selection
                .target_sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err("PROJECT_MCP_SELECTION_INVALID".into());
        }
        let control = control.to_path_buf();
        let assignment = assignment.map(Path::to_path_buf);
        tokio::task::spawn_blocking(move || {
            let registered =
                resolve_registered_project_from_control_root(&control, &selection.project_id)
                    .map_err(|_| "PROJECT_MCP_REGISTRATION_UNAVAILABLE")?;
            let mut context = Self {
                registered,
                selection,
                task: None,
            };
            if context.verify_now()? != context.selection {
                return Err("PROJECT_MCP_BINDING_CHANGED".into());
            }
            if let Some(assignment) = assignment {
                context.task = Some(crate::project_mcp_task::ProjectTask::open_with_budget(
                    &context.registered,
                    &assignment,
                    connection_seconds,
                )?);
            }
            Ok(Arc::new(context))
        })
        .await
        .map_err(|_| "PROJECT_MCP_SCOPE_UNAVAILABLE")?
    }

    fn verify_now(&self) -> Result<Selection, String> {
        let observed = resolve_registered_project_from_control_root(
            &self.registered.control_workspace,
            &self.selection.project_id,
        )
        .map_err(|_| "PROJECT_MCP_REGISTRATION_UNAVAILABLE")?;
        if observed.control_workspace != self.registered.control_workspace
            || observed.project.project_id != self.registered.project.project_id
            || observed.project.workspace != self.registered.project.workspace
            || observed.project.git_identity != self.registered.project.git_identity
        {
            return Err("PROJECT_MCP_BINDING_CHANGED".into());
        }
        let target = mcp::operator_read_project_web_target_status(
            &self.registered.control_workspace,
            &self.selection.project_id,
        )
        .map_err(|_| "PROJECT_MCP_TARGET_UNAVAILABLE")?
        .ok_or("PROJECT_MCP_TARGET_UNBOUND")?;
        Ok(Selection {
            project_id: self.selection.project_id.clone(),
            target_generation: target.generation,
            target_sha256: target.sha256,
        })
    }

    async fn verify(self: &Arc<Self>) -> Result<Selection, String> {
        let context = self.clone();
        tokio::task::spawn_blocking(move || context.verify_now())
            .await
            .map_err(|_| "PROJECT_MCP_SCOPE_UNAVAILABLE")?
    }

    pub fn metadata(&self) -> Value {
        self.metadata_for(&self.selection)
    }

    pub async fn close_task(&self) -> Result<(), String> {
        if let Some(task) = &self.task {
            task.close().await?;
        }
        Ok(())
    }

    pub(crate) async fn dispatch_task(
        self: &Arc<Self>,
        id: Option<Value>,
        expected: &Selection,
        arguments: Value,
    ) -> JsonRpcResponse {
        let current = match self.verify_dispatch_target().await {
            Ok(current) if &current == expected => current,
            _ => {
                return JsonRpcResponse::error(
                    id,
                    -32002,
                    "PROJECT_MCP_SCOPE_WITNESS_MISMATCH".into(),
                );
            }
        };
        let traffic = TrafficObservation::start(
            self,
            &current,
            json!({"name":crate::project_mcp_task::TOOL,"arguments":arguments}),
        )
        .await;
        let result = match &self.task {
            Some(task) => task.execute(arguments).await,
            None => Err("PROJECT_TASK_NOT_ASSIGNED".into()),
        };
        let mut response = match result {
            Ok(result) => JsonRpcResponse::success(
                id.clone(),
                json!({"content":[{"type":"text","text":result["boundedText"]}],"structuredContent":result,"isError":false}),
            ),
            Err(reason) => JsonRpcResponse::error(id.clone(), -32002, reason),
        };
        if self.verify_dispatch_target().await.as_ref() != Ok(&current) {
            response = JsonRpcResponse::error(id, -32002, "PROJECT_MCP_BINDING_CHANGED".into());
        }
        traffic.finish(&mut response).await;
        response
    }

    async fn verify_dispatch_target(self: &Arc<Self>) -> Result<Selection, String> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let result = self.verify().await;
            if matches!(
                result.as_ref().map_err(String::as_str),
                Err("PROJECT_MCP_TARGET_UNAVAILABLE")
            ) && tokio::time::Instant::now() < deadline
            {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            } else {
                return result;
            }
        }
    }

    fn metadata_for(&self, current: &Selection) -> Value {
        json!({"projectId":current.project_id,"targetGeneration":current.target_generation,
            "targetSha256":current.target_sha256,"executionWorkspace":self.registered.project.workspace,
            "access":match &self.task {Some(task) if task.commands_enabled() => "READ_ONLY_INSPECTION_WITH_ASSIGNED_TASK_PATCH_AND_COMMAND_TOOLS", Some(_) => "READ_ONLY_INSPECTION_WITH_ASSIGNED_TASK_PATCH_TOOLS", None => "READ_ONLY"},"bindingSource":"OPERATOR_SELECTED_EXISTING_REGISTRY",
            "conversationOriginAuthenticated":false,"nativeWriterClaimed":false,
            "assignedTask":self.task.as_ref().map(|task| task.metadata())})
    }

    fn take_witness(&self, req: &mut JsonRpcRequest, current: &Selection) -> Result<(), String> {
        let arguments = req
            .params
            .get_mut("arguments")
            .and_then(Value::as_object_mut)
            .ok_or("PROJECT_MCP_SCOPE_WITNESS_REQUIRED")?;
        if arguments.get("projectId").and_then(Value::as_str) != Some(current.project_id.as_str())
            || arguments
                .get("expectedTargetGeneration")
                .and_then(Value::as_u64)
                != Some(current.target_generation)
            || arguments
                .get("expectedTargetSha256")
                .and_then(Value::as_str)
                != Some(current.target_sha256.as_str())
        {
            return Err("PROJECT_MCP_SCOPE_WITNESS_MISMATCH".into());
        }
        for name in [
            "projectId",
            "expectedTargetGeneration",
            "expectedTargetSha256",
        ] {
            arguments.remove(name);
        }
        Ok(())
    }

    fn describe_tools(&self, response: &mut JsonRpcResponse) {
        let Some(tools) = response
            .result
            .as_mut()
            .and_then(|value| value.get_mut("tools"))
            .and_then(Value::as_array_mut)
        else {
            return;
        };
        if let Some(task) = &self.task {
            tools.push(task.schema());
        }
        for tool in tools.iter_mut() {
            let Some(schema) = tool.get_mut("inputSchema").and_then(Value::as_object_mut) else {
                continue;
            };
            let properties = schema
                .entry("properties")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .expect("tool schema properties");
            properties.insert(
                "projectId".into(),
                json!({"type":"string","const":self.selection.project_id}),
            );
            // Connector catalogs can be cached across conversation rollover.
            // Keep project identity fixed, but validate fresh generation/digest
            // values against paired authority on every call, never in a cache.
            properties.insert("expectedTargetGeneration".into(), json!({"type":"integer","minimum":1,"description":"Current paired generation from the project handoff or catdesk_project_scope."}));
            properties.insert("expectedTargetSha256".into(), json!({"type":"string","pattern":"^[a-f0-9]{64}$","description":"Current paired conversation digest from the project handoff or catdesk_project_scope."}));
            let required = schema
                .entry("required")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("tool schema required");
            for field in [
                "projectId",
                "expectedTargetGeneration",
                "expectedTargetSha256",
            ] {
                if !required.iter().any(|value| value == field) {
                    required.push(json!(field));
                }
            }
        }
        tools.push(json!({"name":SCOPE_TOOL,"description":"Read this connection's fixed registered project identity and access. Starts no worker and claims no native writer.",
            "inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false}}));
    }
}

#[derive(Clone)]
struct ProjectServer {
    context: Arc<ProjectContext>,
    token: Arc<String>,
}

pub(crate) fn router(
    context: Arc<ProjectContext>,
    path: &str,
    token: String,
) -> Result<Router, String> {
    if token.len() < 24 || token.chars().any(char::is_whitespace) {
        return Err("PROJECT_MCP_AUTH_REQUIRED".into());
    }
    Ok(Router::new()
        .route(path, post(project_post))
        .with_state(ProjectServer {
            context,
            token: Arc::new(token),
        }))
}

fn rpc_error(id: Option<Value>, status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(JsonRpcResponse::error(id, -32002, message.into())),
    )
        .into_response()
}

async fn project_post(
    State(server): State<ProjectServer>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !crate::server::mcp_authorized(&headers, Some(server.token.as_str())) {
        return rpc_error(None, StatusCode::UNAUTHORIZED, "PROJECT_MCP_UNAUTHORIZED");
    }
    if body.len() > MAX_REQUEST_BYTES {
        return rpc_error(
            None,
            StatusCode::PAYLOAD_TOO_LARGE,
            "PROJECT_MCP_REQUEST_TOO_LARGE",
        );
    }
    let Ok(mut req) = serde_json::from_slice::<JsonRpcRequest>(&body) else {
        return rpc_error(None, StatusCode::BAD_REQUEST, "PROJECT_MCP_INVALID_REQUEST");
    };
    let current = match server.context.verify().await {
        Ok(current) => current,
        Err(reason) => return rpc_error(req.id, StatusCode::OK, &reason),
    };
    let scope_call = req.method == "tools/call"
        && req.params.get("name").and_then(Value::as_str) == Some(SCOPE_TOOL);
    let traffic_params = (req.method == "tools/call").then(|| req.params.clone());
    if req.method == "tools/call" && !scope_call {
        if let Err(reason) = server.context.take_witness(&mut req, &current) {
            return rpc_error(req.id, StatusCode::OK, &reason);
        }
    }
    // Only the locally selected authenticated connection supplies attribution.
    // It does not prove which provider conversation received the exchange.
    let traffic = if let Some(params) = traffic_params {
        Some(TrafficObservation::start(&server.context, &current, params).await)
    } else {
        None
    };
    let workspace = match server.context.registered.project.workspace.to_str() {
        Some(workspace) => workspace,
        None => return rpc_error(req.id, StatusCode::OK, "PROJECT_MCP_WORKSPACE_UNAVAILABLE"),
    };
    let response = if scope_call {
        Some(JsonRpcResponse::success(
            req.id.clone(),
            json!({"content":[{"type":"text","text":"Fixed project connection"}],"structuredContent":server.context.metadata_for(&current),"isError":false}),
        ))
    } else if req.method == "tools/call"
        && req.params.get("name").and_then(Value::as_str) == Some(crate::project_mcp_task::TOOL)
    {
        let result = match &server.context.task {
            Some(task) => task.execute(req.params["arguments"].clone()).await,
            None => Err("PROJECT_TASK_NOT_ASSIGNED".into()),
        };
        Some(match result {
            Ok(result) => JsonRpcResponse::success(
                req.id.clone(),
                json!({"content":[{"type":"text","text":result["boundedText"]}],"structuredContent":result,"isError":false}),
            ),
            Err(reason) => JsonRpcResponse::error(req.id.clone(), -32002, reason),
        })
    } else {
        mcp::handle_request(
            &req,
            workspace,
            0,
            None,
            Mode::Computer,
            ToolMode::ReadOnly,
            false,
            &None,
        )
        .await
    };
    let Some(mut response) = response else {
        return StatusCode::ACCEPTED.into_response();
    };
    if req.method == "tools/call" {
        // Do not disclose a result after a concurrent local rebind/revocation.
        match server.context.verify().await {
            Ok(after) if after == current => {}
            Ok(_) => {
                response = JsonRpcResponse::error(
                    req.id.clone(),
                    -32002,
                    "PROJECT_MCP_BINDING_CHANGED".into(),
                )
            }
            Err(reason) => response = JsonRpcResponse::error(req.id.clone(), -32002, reason),
        }
    }
    if let Some(traffic) = traffic {
        // Accounting cannot turn a successful mutation into a failed/replayed
        // operation. The unchanged task journal remains execution authority.
        traffic.finish(&mut response).await;
    }
    if req.method == "tools/list" {
        server.context.describe_tools(&mut response);
    }
    if req.method == "initialize" {
        if let Some(result) = response.result.as_mut() {
            result["instructions"] = json!(format!(
                "This connection is fixed to project {}. Use current generation/digest witnesses from the project handoff or catdesk_project_scope. Generic tools inspect the registered workspace read-only. If assignedTask is present, use catdesk_project_task_tool for that separate worktree, exact taskId and stable callId. Native task ownership is unchanged; recorded calls must never be replayed.",
                current.project_id
            ));
            result["projectScope"] = server.context.metadata_for(&current);
        }
    }
    Json(response).into_response()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::delegated::autonomy_projects::AutonomousProjectRegistryStoreV1;
    use std::{
        fs,
        path::{Path, PathBuf},
        time::Duration,
    };
    use tokio::{sync::oneshot, task::JoinHandle};

    const ALPHA_TOKEN: &str = "fixture-alpha-project-token-000000";
    const BETA_TOKEN: &str = "fixture-beta-project-token-0000000";

    fn git_root(path: &Path, identity: &str) -> PathBuf {
        fs::create_dir_all(path).unwrap();
        for args in [
            &["init", "-q"][..],
            &["remote", "add", "origin", identity][..],
        ] {
            assert!(
                std::process::Command::new("git")
                    .arg("-C")
                    .arg(path)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        path.canonicalize().unwrap()
    }

    async fn fixture() -> (PathBuf, PathBuf, Arc<ProjectContext>, Arc<ProjectContext>) {
        let parent =
            std::env::temp_dir().join(format!("project-mcp-scope-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&parent).unwrap();
        let control = parent.join("control");
        let package = parent.join("package");
        let python = parent.join("python");
        for path in [&control, &package, &python] {
            fs::create_dir(path).unwrap();
        }
        for (name, data) in [
            ("CatDeskWakeHost.exe", b"fixture host".as_slice()),
            ("CatDeskBinagotchy.exe", b"fixture app".as_slice()),
            (
                "adapter.py",
                include_bytes!("../wake/adapter.py").as_slice(),
            ),
            (
                "wake_bridge.py",
                include_bytes!("../scripts/wake_bridge.py").as_slice(),
            ),
        ] {
            fs::write(package.join(name), data).unwrap();
        }
        fs::write(python.join("python.exe"), b"fixture Python").unwrap();
        fs::write(python.join("__init__.py"), b"").unwrap();
        catdesk_wake::development_runtime::prepare(&control, &package, &python).unwrap();
        let control = git_root(&control, "https://example.test/control.git");
        let wake = catdesk_wake::development_runtime::root_for_workspace(&control)
            .unwrap()
            .unwrap();
        let registry =
            AutonomousProjectRegistryStoreV1::open(control.join(".catdesk/projects")).unwrap();
        registry
            .register_web_only_project(
                &control,
                "catdesk",
                &control,
                "https://example.test/control.git",
            )
            .unwrap();
        let mut contexts = vec![];
        for project in ["alpha", "beta"] {
            let identity = format!("https://example.test/{project}.git");
            let root = git_root(&parent.join(project), &identity);
            fs::write(root.join("note.txt"), project).unwrap();
            registry
                .register_web_only_project(&control, project, &root, &identity)
                .unwrap();
            let target = mcp::operator_bind_project_web_target_at_root(
                &control,
                Some(&wake),
                project,
                &format!("https://chatgpt.com/c/{project}-project-scope"),
                None,
                0,
            )
            .unwrap();
            contexts.push(
                ProjectContext::bind(
                    &control,
                    Selection {
                        project_id: project.into(),
                        target_generation: target.generation,
                        target_sha256: target.sha256,
                    },
                )
                .await
                .unwrap(),
            );
        }
        (control, wake, contexts.remove(0), contexts.remove(0))
    }

    struct OwnedServer {
        url: String,
        stop: Option<oneshot::Sender<()>>,
        task: Option<JoinHandle<std::io::Result<()>>>,
    }
    impl OwnedServer {
        async fn start(context: Arc<ProjectContext>, token: &str) -> Self {
            let router = router(context, "/mcp", token.into()).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/mcp", listener.local_addr().unwrap());
            let (stop, stopped) = oneshot::channel();
            let task = tokio::spawn(async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .await
            });
            Self {
                url,
                stop: Some(stop),
                task: Some(task),
            }
        }
        async fn finish(mut self) -> bool {
            let _ = self.stop.take().unwrap().send(());
            let mut task = self.task.take().unwrap();
            match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
                Ok(Ok(Ok(()))) => true,
                _ => {
                    task.abort();
                    let _ = task.await;
                    false
                }
            }
        }
    }
    impl Drop for OwnedServer {
        fn drop(&mut self) {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
            if let Some(task) = self.task.take() {
                task.abort();
            }
        }
    }

    fn call(selection: &Selection, name: &str, mut args: Value) -> Value {
        args["projectId"] = json!(selection.project_id);
        args["expectedTargetGeneration"] = json!(selection.target_generation);
        args["expectedTargetSha256"] = json!(selection.target_sha256);
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}})
    }

    async fn rpc(
        client: &reqwest::Client,
        server: &OwnedServer,
        token: &str,
        body: &Value,
    ) -> Result<Value, reqwest::Error> {
        client
            .post(&server.url)
            .bearer_auth(token)
            .json(body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    fn pressure(
        control: &Path,
        selection: &Selection,
        url: &str,
    ) -> crate::web_conversation_context::PressureStatus {
        WebContextStore::open_read_only(control.join(".catdesk/web-context"))
            .unwrap()
            .pressure(&ConversationKey {
                project_id: selection.project_id.clone(),
                conversation_url: url.into(),
                target_generation: selection.target_generation,
            })
            .unwrap()
    }

    #[test]
    fn traffic_projection_is_bounded_counts_only_and_marks_nontext_uncertainty() {
        let response = JsonRpcResponse::success(
            Some(json!(1)),
            json!({
            "content":[{"type":"text","text":"VISIBLE_TRAFFIC_SENTINEL"},
                {"type":"image","data":"BINARY_SECRET_SENTINEL","mimeType":"image/png"}],
            "structuredContent":{"summary":"STRUCTURED_TRAFFIC_SENTINEL"},
            "_meta":{"private":"AUTH_SECRET_SENTINEL"}}),
        );
        let projected = response_traffic(&response);
        let text = std::str::from_utf8(&projected.bytes).unwrap();
        assert!(text.contains("VISIBLE_TRAFFIC_SENTINEL"));
        assert!(text.contains("STRUCTURED_TRAFFIC_SENTINEL"));
        assert!(!text.contains("SECRET_SENTINEL"));
        assert!(projected.incomplete);

        let root =
            std::env::temp_dir().join(format!("project-traffic-text-{}", uuid::Uuid::new_v4()));
        let store = WebContextStore::open(&root).unwrap();
        let key = ConversationKey {
            project_id: "alpha".into(),
            conversation_url: "https://chatgpt.com/c/traffic-bounded".into(),
            target_generation: 1,
        };
        store.bind_observed_target(key.clone(), None).unwrap();
        let large = TrafficText::from_value(
            &json!({"payload":"🙂".repeat(MAX_TRAFFIC_BYTES / 4 + 4)}),
            false,
        );
        assert!(large.incomplete);
        assert!(large.bytes.len() <= MAX_TRAFFIC_BYTES);
        assert!(std::str::from_utf8(&large.bytes).is_ok());
        let stress_started = std::time::Instant::now();
        large
            .record(
                &store,
                &key,
                "mcp-invocation-a",
                "arguments",
                VisibleSourceKind::ToolArguments,
            )
            .unwrap();
        let first = store.pressure(&key).unwrap();
        large
            .record(
                &store,
                &key,
                "mcp-invocation-a",
                "arguments",
                VisibleSourceKind::ToolArguments,
            )
            .unwrap();
        assert_eq!(store.pressure(&key).unwrap(), first);
        let stress_elapsed = stress_started.elapsed();
        eprintln!(
            "C45_STRESS_2MIB_INITIAL_PLUS_REPLAY_MS={}",
            stress_elapsed.as_millis()
        );
        assert!(
            stress_elapsed < Duration::from_secs(10),
            "advisory token estimation exceeded its stress budget: {stress_elapsed:?}"
        );
        assert!(first.recorded_events <= MAX_TRAFFIC_BYTES / TRAFFIC_CHUNK_BYTES + 2);
        assert_eq!(first.unknown_source_count, 1);
        projected
            .record(
                &store,
                &key,
                "mcp-invocation-a",
                "result",
                VisibleSourceKind::ToolResult,
            )
            .unwrap();
        for path in fs::read_dir(root.join("conversations")).unwrap() {
            let persisted = fs::read_to_string(path.unwrap().path()).unwrap();
            for raw in [
                "VISIBLE_TRAFFIC_SENTINEL",
                "STRUCTURED_TRAFFIC_SENTINEL",
                "AUTH_SECRET_SENTINEL",
                "BINARY_SECRET_SENTINEL",
                "🙂",
            ] {
                assert!(!persisted.contains(raw));
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn project_mcp_counts_same_rpc_id_invocations_separately_in_their_own_conversations() {
        let (control, _, alpha, beta) = fixture().await;
        let a = OwnedServer::start(alpha.clone(), ALPHA_TOKEN).await;
        let b = OwnedServer::start(beta.clone(), BETA_TOKEN).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let request = call(&alpha.selection, "read", json!({"path":"note.txt"}));
        let first = rpc(&client, &a, ALPHA_TOKEN, &request).await;
        let first_pressure = pressure(
            &control,
            &alpha.selection,
            alpha
                .registered
                .project
                .chatgpt_target_url
                .as_deref()
                .unwrap(),
        );
        let second = rpc(&client, &a, ALPHA_TOKEN, &request).await;
        let second_pressure = pressure(
            &control,
            &alpha.selection,
            alpha
                .registered
                .project
                .chatgpt_target_url
                .as_deref()
                .unwrap(),
        );
        let sibling = rpc(
            &client,
            &b,
            BETA_TOKEN,
            &call(&beta.selection, "read", json!({"path":"note.txt"})),
        )
        .await;
        let sibling_pressure = pressure(
            &control,
            &beta.selection,
            beta.registered
                .project
                .chatgpt_target_url
                .as_deref()
                .unwrap(),
        );
        drop(client);
        let (a_stopped, b_stopped) = tokio::join!(a.finish(), b.finish());
        assert!(a_stopped && b_stopped);
        for response in [first.unwrap(), second.unwrap(), sibling.unwrap()] {
            let status = &response["result"]["_meta"]["catdeskContextObservation"];
            assert_eq!(status["arguments"], "RECORDED");
            assert_eq!(status["result"], "RECORDED");
            assert_eq!(status["possibleRetryOrSurfaceOverlap"], true);
            assert_eq!(status["originAuthenticated"], false);
        }
        assert_eq!(first_pressure.recorded_events, 2);
        assert_eq!(second_pressure.recorded_events, 4);
        assert_eq!(
            second_pressure.visible_unique_tokens,
            first_pressure.visible_unique_tokens * 2
        );
        assert_eq!(sibling_pressure.recorded_events, 2);
        assert!(sibling_pressure.observed_tool_argument_tokens > 0);
        assert!(sibling_pressure.observed_tool_result_tokens > 0);
        assert!(!sibling_pressure.coverage_known);
        assert!(!sibling_pressure.automatic_rollover_enabled);
        assert_eq!(
            pressure(
                &control,
                &alpha.selection,
                alpha
                    .registered
                    .project
                    .chatgpt_target_url
                    .as_deref()
                    .unwrap()
            ),
            second_pressure
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn traffic_completion_after_rollover_updates_only_original_known_conversation() {
        let (control, wake, alpha, _) = fixture().await;
        let old = TrafficObservation::start(
            &alpha,
            &alpha.selection,
            json!({"name":"read","arguments":{"path":"note.txt"}}),
        )
        .await;
        assert_eq!(old.arguments_status, "RECORDED");
        let old_key = old.key.clone().unwrap();
        let next = mcp::operator_bind_project_web_target_at_root(
            &control,
            Some(&wake),
            "alpha",
            "https://chatgpt.com/c/traffic-next",
            Some(&alpha.selection.target_sha256),
            alpha.selection.target_generation,
        )
        .unwrap();
        let fresh = Selection {
            project_id: "alpha".into(),
            target_generation: next.generation,
            target_sha256: next.sha256,
        };
        let new = TrafficObservation::start(
            &alpha,
            &fresh,
            json!({"name":"read","arguments":{"path":"note.txt"}}),
        )
        .await;
        let new_key = new.key.clone().unwrap();
        let mut new_result = JsonRpcResponse::success(
            Some(json!(1)),
            json!({"content":[{"type":"text","text":"new result"}]}),
        );
        new.finish(&mut new_result).await;
        let meter = WebContextStore::open_read_only(control.join(".catdesk/web-context")).unwrap();
        let new_before = meter.pressure(&new_key).unwrap();
        let history_before = meter
            .project_history("alpha")
            .unwrap()
            .into_iter()
            .map(|entry| entry.key)
            .collect::<Vec<_>>();
        let mut old_result = JsonRpcResponse::success(
            Some(json!(1)),
            json!({"content":[{"type":"text","text":"old final result"}]}),
        );
        old.finish(&mut old_result).await;
        assert_eq!(
            old_result.result.as_ref().unwrap()["_meta"]["catdeskContextObservation"]["result"],
            "RECORDED"
        );
        assert_eq!(meter.pressure(&new_key).unwrap(), new_before);
        assert_eq!(
            meter
                .project_history("alpha")
                .unwrap()
                .into_iter()
                .map(|entry| entry.key)
                .collect::<Vec<_>>(),
            history_before
        );
        assert_eq!(meter.active_key("alpha").unwrap(), Some(new_key));
        let old_pressure = meter.pressure(&old_key).unwrap();
        assert!(!old_pressure.is_current);
        assert!(old_pressure.observed_tool_result_tokens > 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn project_mcp_meter_busy_does_not_fail_or_replay_an_applied_patch() {
        let (control, _, alpha, _) = fixture().await;
        let source = &alpha.registered.project.workspace;
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(source)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        };
        git(&["add", "note.txt"]);
        git(&[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ]);
        let isolated = crate::command::normalize_windows_verbatim_path(
            control.parent().unwrap().join("isolated-alpha"),
        );
        git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "codex/context-meter-test",
            isolated.to_str().unwrap(),
        ]);
        let isolated = isolated.canonicalize().unwrap();
        let mut contract: crate::delegated::contracts::ExecutionContractV1 = serde_json::from_str(
            include_str!("../tests/fixtures/delegated/execution_contract_v1.json"),
        )
        .unwrap();
        contract.task_id = "meter-task".into();
        contract.workspace = isolated.to_str().unwrap().into();
        contract.feature_branch = "codex/context-meter-test".into();
        contract.allowed_paths = vec!["note.txt".into()];
        contract.max_elapsed_seconds = 180;
        contract.approval_requirements.clear();
        let assignment = control.parent().unwrap().join("meter-assignment.json");
        fs::write(&assignment, serde_json::to_vec(&json!({"schemaVersion":1,"projectId":"alpha","baseCommit":git(&["rev-parse", "HEAD"]),"contract":contract})).unwrap()).unwrap();
        let context = ProjectContext::bind_with_task(
            &control,
            alpha.selection.clone(),
            Some(&assignment),
            180,
        )
        .await
        .unwrap();
        let server = OwnedServer::start(context.clone(), ALPHA_TOKEN).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap();
        let preview = rpc(&client, &server, ALPHA_TOKEN, &call(&alpha.selection, crate::project_mcp_task::TOOL,
            json!({"taskId":"meter-task","callId":"preview","tool":"patch.preview","arguments":{"patchId":"meter-patch","operations":[{"path":"note.txt","old":"alpha","new":"alpha changed once"}]}}))).await;
        let held = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(control.join(".catdesk/web-context/ledger.lock"))
            .unwrap();
        held.try_lock().unwrap();
        let apply_request = call(
            &alpha.selection,
            crate::project_mcp_task::TOOL,
            json!({"taskId":"meter-task","callId":"apply","tool":"patch.apply","arguments":{"patchId":"meter-patch"}}),
        );
        let applied = rpc(&client, &server, ALPHA_TOKEN, &apply_request).await;
        drop(held);
        let replay = rpc(&client, &server, ALPHA_TOKEN, &apply_request).await;
        drop(client);
        let stopped = server.finish().await;
        let closed = context.close_task().await;
        assert!(stopped);
        closed.unwrap();
        assert!(preview.unwrap()["error"].is_null());
        let applied = applied.unwrap();
        assert!(applied["error"].is_null(), "{applied}");
        assert_eq!(
            applied["result"]["_meta"]["catdeskContextObservation"]["arguments"],
            "BUSY"
        );
        assert_eq!(
            applied["result"]["_meta"]["catdeskContextObservation"]["result"],
            "BUSY"
        );
        assert_eq!(
            fs::read_to_string(isolated.join("note.txt")).unwrap(),
            "alpha changed once"
        );
        assert_eq!(
            fs::read_to_string(source.join("note.txt")).unwrap(),
            "alpha"
        );
        assert_eq!(
            replay.unwrap()["error"]["message"],
            "PROJECT_TASK_CALL_ALREADY_COMPLETED"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn slow_project_identity_preparation_does_not_block_sibling_target_readback() {
        let (control, _wake, _alpha, _beta) = fixture().await;
        let (started, observed) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let alpha_root = control.clone();
        let alpha = std::thread::spawn(move || {
            mcp::project_target_status_with_preparation(&alpha_root, "alpha", || {
                started.send(()).unwrap();
                // A bounded stand-in for an in-flight metadata probe. No
                // lock cleanup or operator action can release this early.
                released.recv_timeout(Duration::from_secs(10)).unwrap();
            })
        });
        observed.recv_timeout(Duration::from_secs(5)).unwrap();
        let beta_root = control.clone();
        let beta = tokio::task::spawn_blocking(move || {
            mcp::operator_read_project_web_target_status(&beta_root, "beta")
        })
        .await
        .unwrap();
        release.send(()).unwrap();
        let alpha = alpha.join().unwrap();
        // Always join/release the delayed reader before checking outcomes.
        assert_eq!(beta.unwrap().unwrap().project_id, "beta");
        assert_eq!(alpha.unwrap().unwrap().project_id, "alpha");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn project_mcp_connections_route_independently_and_follow_only_paired_rollover() {
        let (control, wake, alpha, beta) = fixture().await;
        let a = OwnedServer::start(alpha.clone(), ALPHA_TOKEN).await;
        let b = OwnedServer::start(beta.clone(), BETA_TOKEN).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let observations: Result<_, reqwest::Error> = async {
            let ar = call(
                &alpha.selection,
                "read",
                json!({"path":"note.txt","workspace":beta.registered.project.workspace}),
            );
            let br = call(&beta.selection, "read", json!({"path":"note.txt"}));
            let (first_a, first_b) = tokio::join!(
                rpc(&client, &a, ALPHA_TOKEN, &ar),
                rpc(&client, &b, BETA_TOKEN, &br)
            );
            let first_a = first_a?;
            let first_b = first_b?;
            let wrong_auth = client
                .post(&a.url)
                .bearer_auth(BETA_TOKEN)
                .json(&ar)
                .send()
                .await?
                .status();
            let wrong_scope = rpc(&client, &a, ALPHA_TOKEN, &br).await?;
            let escape = rpc(
                &client,
                &a,
                ALPHA_TOKEN,
                &call(
                    &alpha.selection,
                    "read",
                    json!({"path":beta.registered.project.workspace.join("note.txt")}),
                ),
            )
            .await?;
            let write = rpc(
                &client,
                &a,
                ALPHA_TOKEN,
                &call(
                    &alpha.selection,
                    "write",
                    json!({"path":"note.txt","content":"not authorized"}),
                ),
            )
            .await?;
            let command = rpc(
                &client,
                &a,
                ALPHA_TOKEN,
                &call(
                    &alpha.selection,
                    "run_command",
                    json!({"command":"echo not authorized"}),
                ),
            )
            .await?;
            let catalog = rpc(
                &client,
                &a,
                ALPHA_TOKEN,
                &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            )
            .await?;
            let no_ui = client
                .post(a.url.replace("/mcp", "/agents/path-mode"))
                .json(&json!({"mode":"other"}))
                .send()
                .await?
                .status();
            let target = mcp::operator_bind_project_web_target_at_root(
                &control,
                Some(&wake),
                "alpha",
                "https://chatgpt.com/c/alpha-scope-next",
                Some(&alpha.selection.target_sha256),
                alpha.selection.target_generation,
            )
            .unwrap();
            let stale = rpc(&client, &a, ALPHA_TOKEN, &ar).await?;
            let fresh = Selection {
                project_id: "alpha".into(),
                target_generation: target.generation,
                target_sha256: target.sha256,
            };
            let resumed = rpc(
                &client,
                &a,
                ALPHA_TOKEN,
                &call(&fresh, "read", json!({"path":"note.txt"})),
            )
            .await?;
            // Simulated independent native work; the project endpoint claims no writer.
            fs::write(
                beta.registered.project.workspace.join("note.txt"),
                "beta advanced",
            )
            .unwrap();
            let continuing_beta = rpc(&client, &b, BETA_TOKEN, &br).await?;
            Ok((
                first_a,
                first_b,
                wrong_auth,
                wrong_scope,
                escape,
                write,
                command,
                catalog,
                no_ui,
                stale,
                resumed,
                continuing_beta,
            ))
        }
        .await;
        drop(client);
        let (a_stopped, b_stopped) = tokio::join!(a.finish(), b.finish());
        assert!(
            a_stopped && b_stopped,
            "owned sockets must exit before assertions"
        );
        let (
            ar,
            br,
            auth,
            scope,
            escape,
            write,
            command,
            catalog,
            ui,
            stale,
            resumed,
            beta_progress,
        ) = observations.unwrap();
        assert_eq!(
            ar["result"]["structuredContent"]["text"], "alpha",
            "alpha response: {ar}"
        );
        assert_eq!(
            br["result"]["structuredContent"]["text"], "beta",
            "beta response: {br}"
        );
        assert_eq!(auth, reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(
            scope["error"]["message"],
            "PROJECT_MCP_SCOPE_WITNESS_MISMATCH"
        );
        assert_eq!(escape["result"]["isError"], true);
        assert_eq!(write["result"]["isError"], true);
        assert_eq!(command["result"]["isError"], true);
        assert_eq!(ui, reqwest::StatusCode::NOT_FOUND);
        assert_eq!(
            stale["error"]["message"],
            "PROJECT_MCP_SCOPE_WITNESS_MISMATCH"
        );
        assert_eq!(resumed["result"]["structuredContent"]["text"], "alpha");
        assert_eq!(
            beta_progress["result"]["structuredContent"]["text"],
            "beta advanced"
        );
        assert_eq!(
            fs::read_to_string(alpha.registered.project.workspace.join("note.txt")).unwrap(),
            "alpha"
        );
        let read = catalog["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "read")
            .unwrap();
        assert_eq!(
            read["inputSchema"]["properties"]["projectId"]["const"],
            "alpha"
        );
        assert!(
            read["inputSchema"]["properties"]["expectedTargetGeneration"]["const"].is_null(),
            "cached schemas must permit a new conversation generation"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn project_mcp_workspace_drift_refuses_old_connection_without_blocking_sibling() {
        let (control, _, alpha, beta) = fixture().await;
        let a = OwnedServer::start(alpha.clone(), ALPHA_TOKEN).await;
        let b = OwnedServer::start(beta.clone(), BETA_TOKEN).await;
        let gamma = git_root(
            &control.parent().unwrap().join("gamma"),
            "https://example.test/gamma.git",
        );
        fs::write(gamma.join("note.txt"), "must not be disclosed").unwrap();
        let path = control.join(".catdesk/projects/projects.json");
        let mut registry: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let record = registry["projects"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["projectId"] == "alpha")
            .unwrap();
        record["workspace"] = json!(gamma);
        record["gitIdentity"] = json!("https://example.test/gamma.git");
        fs::write(&path, serde_json::to_vec_pretty(&registry).unwrap()).unwrap();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let ar = call(&alpha.selection, "read", json!({"path":"note.txt"}));
        let br = call(&beta.selection, "read", json!({"path":"note.txt"}));
        let (blocked, sibling) = tokio::join!(
            rpc(&client, &a, ALPHA_TOKEN, &ar),
            rpc(&client, &b, BETA_TOKEN, &br)
        );
        drop(client);
        let (a_stopped, b_stopped) = tokio::join!(a.finish(), b.finish());
        assert!(a_stopped && b_stopped);
        let blocked = blocked.unwrap();
        assert_eq!(blocked["error"]["message"], "PROJECT_MCP_BINDING_CHANGED");
        assert!(!blocked.to_string().contains("must not be disclosed"));
        assert_eq!(
            sibling.unwrap()["result"]["structuredContent"]["text"],
            "beta"
        );
    }
}
