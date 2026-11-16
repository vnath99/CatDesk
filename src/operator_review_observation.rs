//! Read an original approved public-source review without registering a target,
//! granting a send, claiming a new task or manufacturing a delivery receipt.
use super::*;
use crate::delegated::{
    autonomous_contract::{AutonomousDevelopmentContractV1, AutonomousPolicyEngineV1},
    autonomy_state::{AutonomousSessionLockV1, AutonomousSessionStateV1, AutonomousStateStoreV1},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

fn need(value: bool) -> Result<(), String> {
    value
        .then_some(())
        .ok_or_else(|| "REVIEW_OBSERVATION_SCOPE_UNPROVEN".into())
}

fn read_input(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    for component in path.ancestors() {
        let metadata =
            fs::symlink_metadata(component).map_err(|_| "REVIEW_OBSERVATION_INPUT_UNAVAILABLE")?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            need(metadata.file_attributes() & 0x400 == 0)?;
        }
        need(!metadata.file_type().is_symlink())?;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "REVIEW_OBSERVATION_INPUT_UNAVAILABLE")?
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "REVIEW_OBSERVATION_INPUT_UNAVAILABLE")?;
    need(!bytes.is_empty() && bytes.len() as u64 <= maximum)?;
    Ok(bytes)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| "REVIEW_OBSERVATION_SCOPE_UNPROVEN".into())
}
fn json_input(path: &Path, pins: &mut BTreeMap<PathBuf, String>) -> Result<Value, String> {
    let bytes = read_input(path, 256 * 1024)?;
    pins.insert(path.to_path_buf(), digest(&bytes));
    serde_json::from_slice(&bytes).map_err(|_| "REVIEW_OBSERVATION_INPUT_INVALID".into())
}

struct Context {
    pins: BTreeMap<PathBuf, String>,
    store: AutonomousStateStoreV1,
    _lease: AutonomousSessionLockV1,
    policy: AutonomousPolicyEngineV1,
    version: u64,
    session: String,
    project: String,
    task: String,
    url: String,
    request: PathBuf,
    request_sha256: String,
    expires: u64,
}

impl Context {
    fn check(&self) -> Result<(), String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "REVIEW_OBSERVATION_CLOCK_UNAVAILABLE")?
            .as_secs();
        need(now < self.expires && self.policy.contract().autonomy_lease.valid_at(now))
            .map_err(|_| "REVIEW_OBSERVATION_LEASE_EXPIRED")?;
        for (path, expected) in &self.pins {
            need(digest(&read_input(path, 256 * 1024)?) == *expected)
                .map_err(|_| "REVIEW_OBSERVATION_INPUT_CHANGED")?;
        }
        let stored = self
            .store
            .load_contract(&self.session)
            .map_err(|_| "REVIEW_OBSERVATION_NATIVE_CONTRACT_UNAVAILABLE")?;
        need(stored == *self.policy.contract())
            .map_err(|_| "REVIEW_OBSERVATION_NATIVE_CONTRACT_CHANGED")?;
        let state = self
            .store
            .load_session(&self.session)
            .map_err(|_| "REVIEW_OBSERVATION_NATIVE_STATE_UNAVAILABLE")?;
        need(state.approved_contract_hash.as_deref() == Some(self.policy.contract_hash()))
            .map_err(|_| "REVIEW_OBSERVATION_NATIVE_APPROVAL_REQUIRED")?;
        need(
            state.state == AutonomousSessionStateV1::WaitingForChatgpt
                && state.active
                && !state.cancellation_requested
                && state.provider_turn_count > 0,
        )
        .map_err(|_| "REVIEW_OBSERVATION_ORIGINAL_CLAIM_NOT_WAITING")?;
        need(state.last_event_sequence == self.version)
            .map_err(|_| "REVIEW_OBSERVATION_NATIVE_STATE_CHANGED")?;
        need(state.current_task_id.as_deref() == Some(self.task.as_str()))
            .map_err(|_| "REVIEW_OBSERVATION_NATIVE_TASK_CHANGED")?;
        need(
            state.provider_thread_id.is_none()
                && state.provider_handle_id.is_none()
                && state.expected_codex_thread_id.is_none()
                && state.codex_continuity.is_none(),
        )
        .map_err(|_| "REVIEW_OBSERVATION_NATIVE_PROVIDER_OWNER_PRESENT".into())
    }
}

fn prepare(intent: &Path, expected: &str, turn: usize) -> Result<Context, String> {
    let mut pins = BTreeMap::new();
    let workflow = json_input(intent, &mut pins)?;
    need(pins.get(intent).is_some_and(|actual| actual == expected))
        .map_err(|_| "REVIEW_OBSERVATION_WORKFLOW_CHANGED")?;
    let parent = intent.parent().ok_or("REVIEW_OBSERVATION_INPUT_INVALID")?;
    let consent = json_input(&parent.join("operator-consent.private.json"), &mut pins)?;
    let contract_path = parent.join("contract.private.json");
    let contract_value = json_input(&contract_path, &mut pins)?;
    need(
        pins[&contract_path] == field(&workflow, "contractFileSha256")?
            && workflow["contractFileSha256"] == consent["contractFileSha256"]
            && workflow["contractHash"] == consent["contractHash"]
            && workflow["destination"] == consent["destination"]
            && consent["source"] == "DIRECT_HUMAN_REPLY_IN_CURRENT_CODEX_CHAT"
            && consent["answer"] == "Authorize exact contract and frozen review batch"
            && workflow["allPayloadsFrozenBeforeConsent"] == true,
    )?;
    let messages = workflow["frozenMessages"]
        .as_array()
        .ok_or("REVIEW_OBSERVATION_INPUT_INVALID")?;
    let maximum = workflow["maximumMessages"]
        .as_u64()
        .ok_or("REVIEW_OBSERVATION_INPUT_INVALID")?;
    need(
        maximum > 0
            && maximum <= 8
            && maximum as usize == messages.len()
            && workflow["maximumMessages"] == consent["maximumMessages"]
            && workflow["frozenMessages"] == consent["messages"]
            && turn > 0
            && turn <= messages.len(),
    )?;
    let message = &messages[turn - 1];
    need(message["turn"].as_u64() == Some(turn as u64))?;
    let name = field(message, "file")?;
    need(
        !name.is_empty()
            && name.len() <= 128
            && name.ends_with(".frozen.txt")
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')),
    )?;
    let request = parent.join(name);
    let request_sha256 = digest(&read_input(&request, 65536)?);
    need(request_sha256 == field(message, "sha256")?)?;
    pins.insert(request.clone(), request_sha256.clone());
    let contract: AutonomousDevelopmentContractV1 = serde_json::from_value(contract_value)
        .map_err(|_| "REVIEW_OBSERVATION_NATIVE_CONTRACT_INVALID")?;
    let policy = AutonomousPolicyEngineV1::new(contract)
        .map_err(|_| "REVIEW_OBSERVATION_NATIVE_CONTRACT_INVALID")?;
    let session = field(&workflow, "sessionId")?.to_owned();
    need(
        session == policy.contract().contract_id
            && policy.contract_hash() == field(&workflow, "contractHash")?
            && policy.contract().is_direct_chatgpt(),
    )?;
    let url = field(&workflow, "destination")?.to_owned();
    catdesk_wake::protocol::validate_target(&url)?;
    let store = AutonomousStateStoreV1::open_existing(
        Path::new(&policy.contract().workspace).join(".catdesk/autonomy"),
    )
    .map_err(|_| "REVIEW_OBSERVATION_NATIVE_STORE_UNAVAILABLE")?;
    let lease = store
        .acquire_lock(&session, "readonly-review-observer")
        .map_err(|_| "REVIEW_OBSERVATION_NATIVE_OWNER_BUSY")?;
    let state = store
        .load_session(&session)
        .map_err(|_| "REVIEW_OBSERVATION_NATIVE_STATE_UNAVAILABLE")?;
    let context = Context {
        pins,
        store,
        _lease: lease,
        version: state.last_event_sequence,
        project: policy.contract().project_id.clone(),
        task: policy.contract().task_id.clone(),
        policy,
        session,
        url,
        request,
        request_sha256,
        expires: workflow["expiresAtUnix"]
            .as_u64()
            .ok_or("REVIEW_OBSERVATION_INPUT_INVALID")?,
    };
    context.check()?;
    Ok(context)
}

pub(super) fn inspect(
    workspace: &Path,
    intent: &Path,
    expected: &str,
    turn: usize,
    preflight_only: bool,
) -> Result<Value, String> {
    let context = prepare(intent, expected, turn)?;
    if preflight_only {
        return Ok(
            json!({"status":"NATIVE_REVIEW_CONTEXT_VERIFIED_BROWSER_NOT_STARTED",
            "reviewSessionId":context.session,"projectId":context.project,"taskId":context.task,
            "contractHash":context.policy.contract_hash(),"workflowSha256":expected,"reviewTurn":turn,
            "targetSha256":catdesk_wake::protocol::digest(&context.url),"requestSha256":context.request_sha256,
            "stateVersion":context.version,"preflightOnly":true,"nativeApprovalVerified":true,
            "browserOwnerVerified":false,"browserStarted":false,"sendAttempted":false,
            "wakeBindingAuthority":false,"reviewVerdictOrProducerAuthorityCreated":false}),
        );
    }
    let mut result = catdesk_wake::development_runtime::inspect_approved_review_request(
        workspace,
        &context.url,
        &context.request,
        &context.request_sha256,
        &|| context.check(),
    )?;
    context.check()?;
    result["reviewSessionId"] = json!(context.session);
    result["projectId"] = json!(context.project);
    result["taskId"] = json!(context.task);
    result["contractHash"] = json!(context.policy.contract_hash());
    result["workflowSha256"] = json!(expected);
    result["reviewTurn"] = json!(turn);
    result["reviewVerdictOrProducerAuthorityCreated"] = json!(false);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegated::autonomy_state::AutonomousQueueV1;

    fn fixture() -> (PathBuf, PathBuf, String, AutonomousStateStoreV1) {
        let mut contract = crate::delegated::autonomous_contract::tests::contract();
        contract.mode = "chatgpt_web_direct_autonomous".into();
        contract.provider_policy.primary_provider = "chatgpt-web".into();
        contract.provider_policy.primary_model = "conversation-selected".into();
        contract.provider_policy.routine_provider = "none".into();
        contract.provider_policy.routine_model = "none".into();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        contract.autonomy_lease.issued_at_unix = now - 1;
        contract.autonomy_lease.expires_at_unix = now + 89;
        let root = contract.workspace.clone();
        let packet = root.join("review-inputs");
        fs::create_dir(&packet).unwrap();
        let raw = serde_json::to_vec(&contract).unwrap();
        fs::write(packet.join("contract.private.json"), &raw).unwrap();
        let contract_hash = contract.decision_hash().unwrap();
        let messages = json!([{"turn":1,"file":"original.frozen.txt","sha256":digest(b"original approved review")}]);
        fs::write(
            packet.join("original.frozen.txt"),
            b"original approved review",
        )
        .unwrap();
        let workflow = json!({"sessionId":contract.contract_id,"contractHash":contract_hash,
            "contractFileSha256":digest(&raw),"destination":"https://chatgpt.com/c/approved-review-fixture",
            "maximumMessages":1,"frozenMessages":messages,"expiresAtUnix":now+80,"allPayloadsFrozenBeforeConsent":true});
        let consent = json!({"source":"DIRECT_HUMAN_REPLY_IN_CURRENT_CODEX_CHAT",
            "answer":"Authorize exact contract and frozen review batch","contractHash":contract_hash,
            "contractFileSha256":digest(&raw),"destination":workflow["destination"],"maximumMessages":1,"messages":messages});
        fs::write(
            packet.join("operator-consent.private.json"),
            serde_json::to_vec(&consent).unwrap(),
        )
        .unwrap();
        let intent = packet.join("review-workflow-intent.private.json");
        fs::write(&intent, serde_json::to_vec(&workflow).unwrap()).unwrap();
        let store = AutonomousStateStoreV1::open(root.join(".catdesk/autonomy")).unwrap();
        let mut state = store
            .create_session(
                &contract.contract_id,
                AutonomousQueueV1 {
                    schema_version: 1,
                    tasks: vec![],
                },
            )
            .unwrap();
        store
            .save_contract(&contract.contract_id, &contract)
            .unwrap();
        state.state = AutonomousSessionStateV1::WaitingForChatgpt;
        state.provider_turn_count = 1;
        state.current_task_id = Some(contract.task_id.clone());
        state.approved_contract_hash = Some(contract_hash);
        store.save_session(&state).unwrap();
        let hash = digest(&fs::read(&intent).unwrap());
        (root, intent, hash, store)
    }

    #[test]
    fn approved_review_observation_reuses_native_claim_without_mutation_or_send() {
        let (root, intent, hash, store) = fixture();
        let before = store.load_session("adc-1").unwrap();
        let context = prepare(&intent, &hash, 1).unwrap();
        context.check().unwrap();
        assert_eq!(context.session, "adc-1");
        assert_eq!(context.task, "t-28");
        assert_eq!(context.request_sha256, digest(b"original approved review"));
        assert!(prepare(&intent, &hash, 1).is_err()); // the original native session lease is held
        assert_eq!(store.load_session("adc-1").unwrap(), before);
        drop(context);
        assert!(prepare(&intent, &hash, 1).is_ok());
        let preflight = inspect(&root, &intent, &hash, 1, true).unwrap();
        assert_eq!(preflight["nativeApprovalVerified"], true);
        assert_eq!(preflight["browserStarted"], false);
        assert_eq!(preflight["browserOwnerVerified"], false);
        assert_eq!(store.load_session("adc-1").unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn approved_review_observation_refuses_changed_scope_expiry_and_missing_approval() {
        let (root, intent, hash, store) = fixture();
        assert!(prepare(&intent, &"0".repeat(64), 1).is_err());
        assert!(prepare(&intent, &hash, 0).is_err());
        assert!(prepare(&intent, &hash, 2).is_err());
        let original = fs::read(&intent).unwrap();
        for (key, value) in [
            ("destination", json!("https://chatgpt.com/c/other")),
            ("expiresAtUnix", json!(1)),
            ("contractHash", json!("fnv1a64:0000000000000000")),
            ("allPayloadsFrozenBeforeConsent", json!(false)),
        ] {
            let mut changed: Value = serde_json::from_slice(&original).unwrap();
            changed[key] = value;
            let bytes = serde_json::to_vec(&changed).unwrap();
            fs::write(&intent, &bytes).unwrap();
            assert!(prepare(&intent, &digest(&bytes), 1).is_err());
        }
        fs::write(&intent, original).unwrap();
        let mut state = store.load_session("adc-1").unwrap();
        state.approved_contract_hash = None;
        store.save_session(&state).unwrap();
        assert!(prepare(&intent, &hash, 1).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn approved_review_observation_detects_mid_observation_changes_and_never_creates_store() {
        let (root, intent, hash, store) = fixture();
        let context = prepare(&intent, &hash, 1).unwrap();
        fs::write(&context.request, b"changed request").unwrap();
        assert!(context.check().is_err());
        fs::write(&context.request, b"original approved review").unwrap();
        context.check().unwrap();
        let mut state = store.load_session("adc-1").unwrap();
        state.cancellation_requested = true;
        store.save_session(&state).unwrap();
        assert!(context.check().is_err());
        drop(context);
        let absent = root.join("absent-autonomy-store");
        assert!(AutonomousStateStoreV1::open_existing(&absent).is_err());
        assert!(!absent.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
