//! Immutable, explicitly authenticated project connections. The local operator
//! selects a registered project; remote requests cannot select a workspace.
//! Direct mutations remain unavailable until native/task ownership is wired.

use crate::{
    delegated::autonomy_projects::{
        ResolvedProjectScopeV1, resolve_registered_project_from_control_root,
    },
    mcp::{self, JsonRpcRequest, JsonRpcResponse},
    state::{Mode, ToolMode},
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
use std::{path::Path, sync::Arc};

const SCOPE_TOOL: &str = "catdesk_project_scope";
const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub project_id: String,
    pub target_generation: u64,
    pub target_sha256: String,
}

pub(crate) struct ProjectContext {
    registered: ResolvedProjectScopeV1,
    selection: Selection,
}

impl ProjectContext {
    pub async fn bind(control: &Path, selection: Selection) -> Result<Arc<Self>, String> {
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
        tokio::task::spawn_blocking(move || {
            let registered =
                resolve_registered_project_from_control_root(&control, &selection.project_id)
                    .map_err(|_| "PROJECT_MCP_REGISTRATION_UNAVAILABLE")?;
            let context = Arc::new(Self {
                registered,
                selection,
            });
            if context.verify_now()? != context.selection {
                return Err("PROJECT_MCP_BINDING_CHANGED".into());
            }
            Ok(context)
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

    fn metadata_for(&self, current: &Selection) -> Value {
        json!({"projectId":current.project_id,"targetGeneration":current.target_generation,
            "targetSha256":current.target_sha256,"executionWorkspace":self.registered.project.workspace,
            "access":"READ_ONLY","bindingSource":"OPERATOR_SELECTED_EXISTING_REGISTRY",
            "conversationOriginAuthenticated":false,"nativeWriterClaimed":false})
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
    if req.method == "tools/call" && !scope_call {
        if let Err(reason) = server.context.take_witness(&mut req, &current) {
            return rpc_error(req.id, StatusCode::OK, &reason);
        }
    }
    let workspace = match server.context.registered.project.workspace.to_str() {
        Some(workspace) => workspace,
        None => return rpc_error(req.id, StatusCode::OK, "PROJECT_MCP_WORKSPACE_UNAVAILABLE"),
    };
    let response = if scope_call {
        Some(JsonRpcResponse::success(
            req.id.clone(),
            json!({"content":[{"type":"text","text":"Fixed project connection"}],"structuredContent":server.context.metadata_for(&current),"isError":false}),
        ))
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
            Ok(_) => return rpc_error(req.id, StatusCode::OK, "PROJECT_MCP_BINDING_CHANGED"),
            Err(reason) => return rpc_error(req.id, StatusCode::OK, &reason),
        }
    }
    if req.method == "tools/list" {
        server.context.describe_tools(&mut response);
    }
    if req.method == "initialize" {
        if let Some(result) = response.result.as_mut() {
            result["instructions"] = json!(format!(
                "This connection is fixed to project {}. Use current generation/digest witnesses from the project handoff or catdesk_project_scope. It exposes read-only project inspection; native task ownership is unchanged.",
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
        assert_eq!(ar["result"]["structuredContent"]["text"], "alpha");
        assert_eq!(br["result"]["structuredContent"]["text"], "beta");
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
