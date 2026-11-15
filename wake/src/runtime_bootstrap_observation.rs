//! Bounded bootstrap readback hints, never delivery or binding authority.
use super::*;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BootstrapDiagnostic {
    ClickUncertain,
    UrlUnissued,
    UrlInvalid,
    TargetDrift,
    AcceptanceUnproven,
    FreshDocumentUnproven,
    ReceiptMismatch,
    ResponseLoadError,
    ResponseIncomplete,
    ResponseTimeout,
    ResponseNetworkError,
    ResponseSequenceUnproven,
    ResponseDraftOrEditorUnproven,
    AdapterExited,
    AdapterProtocolInvalid,
    UnclassifiedPostSubmitFailure,
}

pub(super) fn diagnostic(frame: &serde_json::Value) -> BootstrapDiagnostic {
    use BootstrapDiagnostic::*;
    match frame.get("reason").and_then(|v| v.as_str()) {
        Some("SUBMIT_CLICK_UNKNOWN" | "SUBMIT_ENTER_UNKNOWN") => ClickUncertain,
        Some("BOOTSTRAP_URL_UNISSUED") => UrlUnissued,
        Some("BOOTSTRAP_URL_INVALID") => UrlInvalid,
        Some(
            "BOOTSTRAP_SOURCE_REVISITED" | "BOOTSTRAP_DESTINATION_DRIFT" | "RESPONSE_TARGET_DRIFT",
        ) => TargetDrift,
        Some("SUBMIT_ACCEPTANCE_UNPROVEN") => AcceptanceUnproven,
        Some("SUBMIT_RECEIPT_ROUND_TRIP_FAILED" | "SUBMIT_RECEIPT_UNPROVEN") => {
            FreshDocumentUnproven
        }
        Some("BOOTSTRAP_RECEIPT_SEQUENCE_DRIFT" | "BOOTSTRAP_RECEIPT_MISMATCH") => ReceiptMismatch,
        Some("RESPONSE_LOAD_ERROR" | "BOOTSTRAP_ASSISTANT_LOAD_ERROR_PLACEHOLDER") => {
            ResponseLoadError
        }
        Some(
            "RESPONSE_COMPLETION_UNPROVEN"
            | "RESPONSE_PERSISTENCE_UNPROVEN"
            | "RESPONSE_GENERATION_RESUMED",
        ) => ResponseIncomplete,
        Some("RESPONSE_TOTAL_TIMEOUT" | "RESPONSE_GENERATION_TIMEOUT") => ResponseTimeout,
        Some("RESPONSE_NETWORK_ERROR") => ResponseNetworkError,
        Some("RESPONSE_SEQUENCE_DRIFT" | "RESPONSE_SEQUENCE_QUERY_FAILED") => {
            ResponseSequenceUnproven
        }
        Some("RESPONSE_REFRESH_DRAFT_OR_EDITOR_UNPROVEN") => ResponseDraftOrEditorUnproven,
        _ => UnclassifiedPostSubmitFailure,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BootstrapDestinationHint {
    pub project_id: String,
    pub intent_id: String,
    pub source_generation: u64,
    pub source_digest: String,
    pub message_digest: String,
    pub destination_url: String,
    pub destination_digest: String,
}

impl BootstrapDestinationHint {
    pub fn validate_scope(
        &self,
        project: &str,
        intent: &str,
        generation: u64,
        source_digest: &str,
        message_digest: &str,
    ) -> Result<()> {
        if self.project_id != project
            || self.intent_id != intent
            || self.source_generation != generation
            || self.source_digest != source_digest
            || self.message_digest != message_digest
            || !crate::protocol::valid_id(project)
            || !crate::protocol::valid_id(intent)
        {
            return Err("BOOTSTRAP_DESTINATION_HINT_SCOPE_INVALID".into());
        }
        let destination = Target::new(1, self.destination_url.clone())
            .map_err(|_| "BOOTSTRAP_DESTINATION_HINT_INVALID")?;
        if destination.digest != self.destination_digest {
            return Err("BOOTSTRAP_DESTINATION_HINT_INVALID".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BootstrapReadback {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unverified_destination: Option<BootstrapDestinationHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_submit_diagnostic: Option<BootstrapDiagnostic>,
}

/// Capture only observations emitted after SEND_ATTEMPTED. Neither this helper
/// nor either field can advance a receipt, target generation or binding.
pub fn capture_bootstrap_readback(
    slot: &mut Option<BootstrapReadback>,
    checkpoint: &BootstrapCheckpoint,
) -> bool {
    match checkpoint {
        BootstrapCheckpoint::DestinationObserved(value) => {
            slot.get_or_insert_with(Default::default)
                .unverified_destination = Some(value.clone());
            true
        }
        BootstrapCheckpoint::PostSubmitDiagnostic(value) => {
            slot.get_or_insert_with(Default::default)
                .post_submit_diagnostic = Some(*value);
            true
        }
        _ => false,
    }
}

pub(super) fn destination_frame(
    frame: &serde_json::Value,
    project: &str,
    intent: &str,
    origin: BootstrapOrigin<'_>,
    message: &str,
) -> Result<BootstrapDestinationHint> {
    let mut value = frame.clone();
    if value.as_object().map(|v| v.len()) != Some(8)
        || value["stage"] != "BOOTSTRAP_DESTINATION_OBSERVED"
    {
        return Err("BOOTSTRAP_DESTINATION_HINT_INVALID".into());
    }
    value.as_object_mut().unwrap().remove("stage");
    let hint: BootstrapDestinationHint =
        serde_json::from_value(value).map_err(|_| "BOOTSTRAP_DESTINATION_HINT_INVALID")?;
    let (generation, digest, predecessor) = match origin {
        BootstrapOrigin::Rollover(target) => (
            target.generation,
            target.digest.as_str(),
            Some(target.url.as_str()),
        ),
        BootstrapOrigin::InitialRegistration(digest) => (0, digest, None),
    };
    hint.validate_scope(project, intent, generation, digest, message)?;
    // Both routes have already passed the existing exact route validator.
    if predecessor
        .is_some_and(|url| url.rsplit('/').next() == hint.destination_url.rsplit('/').next())
    {
        return Err("BOOTSTRAP_DESTINATION_HINT_INVALID".into());
    }
    Ok(hint)
}

pub(super) fn outcome_unknown(
    checkpoint: &mut dyn FnMut(BootstrapCheckpoint) -> Result<()>,
    diagnostic: BootstrapDiagnostic,
) -> String {
    // Failure to persist diagnostics must never turn an uncertain submission
    // into a pre-submit error or permit retry. SEND_ATTEMPTED is already durable.
    let _ = checkpoint(BootstrapCheckpoint::PostSubmitDiagnostic(diagnostic));
    "BOOTSTRAP_OUTCOME_UNKNOWN".into()
}

/// A native-history candidate is only a navigation hint. This path sends no
/// bootstrap or submit ACK; the existing browser must independently prove the
/// original full USER and completed acknowledgment in a fresh document.
pub struct InitializationReadbackRequest<'a> {
    pub project: &'a str,
    pub intent: &'a str,
    pub registration: &'a str,
    pub handoff: &'a str,
    pub candidate: &'a str,
    pub native_turn_started_utc: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitializationReadbackObservation {
    pub phase: String,
    pub driver_route: String,
    pub document_route: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<serde_json::Value>,
}

impl InitializationReadbackObservation {
    pub fn validate(&self) -> Result<()> {
        let phases = [
            "NAVIGATION",
            "INITIAL_OBSERVATION",
            "HOME_RECOVERY",
            "TAB_SELECTION",
            "ORIGINAL_USER_CHECK",
            "COMPLETION",
            "REFRESH",
            "PERSISTENCE",
            "ACKNOWLEDGMENT",
            "FINAL_USER_CHECK",
        ];
        let routes = [
            "CHROME_ERROR",
            "OTHER_HOST",
            "AUTH",
            "HOME",
            "SAME_CONVERSATION",
            "DIFFERENT_CONVERSATION",
            "PROJECT_OTHER",
            "OTHER_ROUTE",
            "INVALID_ROUTE",
        ];
        if !phases.contains(&self.phase.as_str())
            || !routes.contains(&self.driver_route.as_str())
            || !routes.contains(&self.document_route.as_str())
        {
            return Err("INITIALIZATION_READBACK_DIAGNOSTIC_INVALID".into());
        }
        if self.failure_code.as_deref().is_some_and(|code| {
            !matches!(
                code,
                "INITIALIZATION_READBACK_USER_UNPROVEN"
                    | "INITIALIZATION_READBACK_FIXED_NAVIGATION_UNAVAILABLE"
                    | "RESPONSE_TARGET_DRIFT"
                    | "RESPONSE_USER_DOCUMENT_NOT_HYDRATED"
                    | "RESPONSE_STATE_QUERY_FAILED"
                    | "BOOTSTRAP_INITIALIZATION_ACK_UNVERIFIED"
                    | "LOGIN_OR_PROFILE_REQUIRED"
                    | "CAPTCHA_OR_SECURITY_CHECK"
                    | "EXISTING_DRAFT"
                    | "BOOTSTRAP_HOME_NOT_READY"
                    | "BOOTSTRAP_HOME_HAS_TURNS"
                    | "BOOTSTRAP_HOME_DRIFT"
                    | "BOOTSTRAP_DRAFT_SERIALIZATION_UNVERIFIED"
                    | "BOOTSTRAP_RICH_DRAFT_UNVERIFIED"
                    | "CHATGPT_NOT_IDLE"
                    | "RESPONSE_LOAD_ERROR"
                    | "RESPONSE_REFRESH_DRAFT_OR_EDITOR_UNPROVEN"
                    | "RESPONSE_PERSISTENCE_UNPROVEN"
                    | "OBSERVATION_UNPROVEN"
                    | "INITIALIZATION_READBACK_TARGET_TAB_UNPROVEN"
            )
        }) {
            return Err("INITIALIZATION_READBACK_DIAGNOSTIC_INVALID".into());
        }
        if self.surface.as_ref().is_some_and(|value| {
            crate::readiness_probe::surface_observation(&format!("PROBE:SURFACE:{value}")).is_none()
        }) {
            return Err("INITIALIZATION_READBACK_DIAGNOSTIC_INVALID".into());
        }
        if self.tab.as_ref().is_some_and(|value| {
            crate::readiness_probe::tab_observation(&format!("PROBE:TAB:{value}")).is_none()
        }) {
            return Err("INITIALIZATION_READBACK_DIAGNOSTIC_INVALID".into());
        }
        Ok(())
    }
}

fn readback_observation(
    frame: &serde_json::Value,
    request: &InitializationReadbackRequest<'_>,
) -> Result<InitializationReadbackObservation> {
    if frame.as_object().map(|v| v.len())
        != Some(
            10 + usize::from(frame.get("failureCode").is_some())
                + usize::from(frame.get("surface").is_some())
                + usize::from(frame.get("tab").is_some()),
        )
        || !matches!(
            frame["stage"].as_str(),
            Some(
                "INITIALIZATION_READBACK_DIAGNOSTIC"
                    | "INITIALIZATION_READBACK_HOME_RECOVERED"
                    | "INITIALIZATION_READBACK_TAB_SELECTED"
            )
        )
        || frame["schemaVersion"] != 1
        || frame["projectId"] != request.project
        || frame["intentId"] != request.intent
        || frame["registrationDigest"] != request.registration
        || frame["messageDigest"]
            != BootstrapOrigin::InitialRegistration(request.registration).message_digest(
                request.project,
                request.intent,
                request.handoff,
            )?
        || frame["candidateDigest"] != crate::protocol::digest(request.candidate)
    {
        return Err("INITIALIZATION_READBACK_DIAGNOSTIC_INVALID".into());
    }
    let observation: InitializationReadbackObservation = serde_json::from_value(serde_json::json!({
        "phase":frame["phase"],"driverRoute":frame["driverRoute"],"documentRoute":frame["documentRoute"],"failureCode":frame["failureCode"],"surface":frame["surface"],"tab":frame["tab"]}))
        .map_err(|_|"INITIALIZATION_READBACK_DIAGNOSTIC_INVALID")?;
    observation.validate()?;
    if frame["stage"] == "INITIALIZATION_READBACK_HOME_RECOVERED"
        && (observation.phase != "HOME_RECOVERY"
            || observation.driver_route != "SAME_CONVERSATION"
            || observation.document_route != "SAME_CONVERSATION"
            || observation.failure_code.is_some())
    {
        return Err("INITIALIZATION_READBACK_DIAGNOSTIC_INVALID".into());
    }
    if frame["stage"] == "INITIALIZATION_READBACK_TAB_SELECTED"
        && (observation.phase != "TAB_SELECTION"
            || observation.driver_route != "SAME_CONVERSATION"
            || observation.document_route != "SAME_CONVERSATION"
            || observation.failure_code.is_some()
            || observation.tab.as_ref().is_none_or(|tab| {
                tab["state"] != "TARGET_TAB_EXACT"
                    || tab["stage"] != "VERIFIED"
                    || tab["metadataMatches"] != 1
                    || tab["unreadableMetadata"] != 0
                    || tab["candidateRoute"] != "EXACT"
                    || tab["controlledRoute"] != "EXACT"
                    || tab["candidateShape"] != "STRING"
                    || tab["controlledShape"] != "STRING"
            }))
    {
        return Err("INITIALIZATION_READBACK_DIAGNOSTIC_INVALID".into());
    }
    Ok(observation)
}

pub fn read_initialization_candidate_guarded(
    store: &Store,
    request: InitializationReadbackRequest<'_>,
    keep_running: &dyn Fn() -> Result<()>,
    observe: &mut dyn FnMut(InitializationReadbackObservation) -> Result<()>,
) -> Result<BootstrapReceipt> {
    let InitializationReadbackRequest {
        project,
        intent,
        registration,
        handoff,
        candidate,
        native_turn_started_utc,
    } = request;
    keep_running()?;
    let destination = Target::new(1, candidate.to_owned())?;
    if native_turn_started_utc == 0 || native_turn_started_utc > crate::store::now() {
        return Err("INITIALIZATION_READBACK_TIME_INVALID".into());
    }
    let origin = BootstrapOrigin::InitialRegistration(registration);
    let message = origin.message_digest(project, intent, handoff)?;
    let _owner = browser_owner_guard(store)?;
    origin.ensure_available(store, project)?;
    crate::development_runtime::validate_target_isolation_for_store(store.root(), candidate)?;
    let installed = installed_bootstrap_directory(store)?;
    let bytes =
        std::fs::read(installed.join("adapter.py")).map_err(|_| "BROWSER_ADAPTER_UNAVAILABLE")?;
    if !bytes
        .windows(b"def initialization_readback(".len())
        .any(|part| part == b"def initialization_readback(")
    {
        return Err("INITIALIZATION_READBACK_PACKAGE_REQUIRED".into());
    }
    if !bytes
        .windows(b"INITIALIZATION_READBACK_DIAGNOSTICS_V1 = True".len())
        .any(|part| part == b"INITIALIZATION_READBACK_DIAGNOSTICS_V1 = True")
    {
        return Err("INITIALIZATION_READBACK_DIAGNOSTICS_PACKAGE_REQUIRED".into());
    }
    if !bytes
        .windows(b"INITIALIZATION_READBACK_SURFACE_V1 = True".len())
        .any(|part| part == b"INITIALIZATION_READBACK_SURFACE_V1 = True")
    {
        return Err("INITIALIZATION_READBACK_DIAGNOSTICS_PACKAGE_REQUIRED".into());
    }
    if !bytes
        .windows(b"INITIALIZATION_READBACK_FAILURE_CODES_V1 = True".len())
        .any(|part| part == b"INITIALIZATION_READBACK_FAILURE_CODES_V1 = True")
    {
        return Err("INITIALIZATION_READBACK_DIAGNOSTICS_PACKAGE_REQUIRED".into());
    }
    if !bytes
        .windows(b"INITIALIZATION_READBACK_HOME_ONLY_V1 = True".len())
        .any(|part| part == b"INITIALIZATION_READBACK_HOME_ONLY_V1 = True")
    {
        return Err("INITIALIZATION_READBACK_HOME_RECOVERY_PACKAGE_REQUIRED".into());
    }
    if !bytes
        .windows(b"INITIALIZATION_READBACK_TAB_SELECTION_V1 = True".len())
        .any(|part| part == b"INITIALIZATION_READBACK_TAB_SELECTION_V1 = True")
    {
        return Err("INITIALIZATION_READBACK_DIAGNOSTICS_PACKAGE_REQUIRED".into());
    }
    let mut adapter = Adapter::spawn_from_installed_guarded(
        store.root(),
        &installed,
        Duration::from_secs(30),
        keep_running,
    )?;
    adapter.send(
        serde_json::json!({"initializationReadback":true,"projectId":project,
        "intentId":intent,"registrationDigest":registration,"handoff":handoff,
        "candidateUrl":candidate,"nativeTurnStartedUtc":native_turn_started_utc}),
    )?;
    let deadline = Instant::now() + Duration::from_secs(TURN_TARGET_SECONDS);
    let mut diagnostic_seen = false;
    let mut home_recovered = false;
    let mut tab_selected = false;
    while Instant::now() < deadline {
        let value = match guarded_browser_receive(&adapter.output, keep_running)? {
            Ok(Ok(value)) => value,
            Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("INITIALIZATION_READBACK_ADAPTER_EXITED".into());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        };
        if diagnostic_seen && value["stage"] != "ATTENTION" {
            return Err("INITIALIZATION_READBACK_PROTOCOL_INVALID".into());
        }
        match value["stage"].as_str() {
            Some("INITIALIZATION_READBACK_TAB_SELECTED") if !tab_selected => {
                let request = InitializationReadbackRequest {
                    project,
                    intent,
                    registration,
                    handoff,
                    candidate,
                    native_turn_started_utc,
                };
                let observation = readback_observation(&value, &request)?;
                keep_running()?;
                observe(observation)?;
                tab_selected = true;
            }
            Some("INITIALIZATION_READBACK_HOME_RECOVERED") if !home_recovered => {
                let request = InitializationReadbackRequest {
                    project,
                    intent,
                    registration,
                    handoff,
                    candidate,
                    native_turn_started_utc,
                };
                let observation = readback_observation(&value, &request)?;
                keep_running()?;
                observe(observation)?;
                home_recovered = true;
            }
            Some("INITIALIZATION_READBACK_DIAGNOSTIC") => {
                let request = InitializationReadbackRequest {
                    project,
                    intent,
                    registration,
                    handoff,
                    candidate,
                    native_turn_started_utc,
                };
                let diagnostic = readback_observation(&value, &request)?;
                keep_running()?;
                observe(diagnostic)?;
                diagnostic_seen = true;
            }
            Some("INITIALIZATION_READBACK_COMPLETE") => {
                let receipt: BootstrapReceipt = serde_json::from_value(value["receipt"].clone())
                    .map_err(|_| "INITIALIZATION_READBACK_RECEIPT_INVALID")?;
                origin.validate_receipt(
                    &receipt,
                    project,
                    intent,
                    &message,
                    "RESPONSE_COMPLETED",
                )?;
                if receipt.destination_url != destination.url
                    || receipt.destination_digest != destination.digest
                    || receipt.sent_utc != native_turn_started_utc
                {
                    return Err("INITIALIZATION_READBACK_RECEIPT_INVALID".into());
                }
                origin.ensure_available(store, project)?;
                crate::development_runtime::validate_target_isolation_for_store(
                    store.root(),
                    candidate,
                )?;
                keep_running()?;
                let _ = adapter.allow_graceful_exit(Duration::from_secs(5));
                return Ok(receipt);
            }
            Some("ATTENTION") => {
                let reason = match value["reason"].as_str() {
                    Some("INITIALIZATION_READBACK_USER_UNPROVEN") => {
                        "INITIALIZATION_READBACK_USER_UNPROVEN"
                    }
                    Some("BOOTSTRAP_INITIALIZATION_ACK_UNVERIFIED") => {
                        "BOOTSTRAP_INITIALIZATION_ACK_UNVERIFIED"
                    }
                    Some("LOGIN_OR_PROFILE_REQUIRED") => "LOGIN_OR_PROFILE_REQUIRED",
                    Some("CAPTCHA_OR_SECURITY_CHECK") => "CAPTCHA_OR_SECURITY_CHECK",
                    Some("RESPONSE_LOAD_ERROR" | "BOOTSTRAP_ASSISTANT_LOAD_ERROR_PLACEHOLDER") => {
                        "RESPONSE_LOAD_ERROR"
                    }
                    Some("RESPONSE_REFRESH_DRAFT_OR_EDITOR_UNPROVEN") => {
                        "RESPONSE_REFRESH_DRAFT_OR_EDITOR_UNPROVEN"
                    }
                    Some("RESPONSE_TARGET_DRIFT") => "RESPONSE_TARGET_DRIFT",
                    Some("RESPONSE_PERSISTENCE_UNPROVEN") => "RESPONSE_PERSISTENCE_UNPROVEN",
                    Some("RESPONSE_USER_DOCUMENT_NOT_HYDRATED") => {
                        "RESPONSE_USER_DOCUMENT_NOT_HYDRATED"
                    }
                    Some("RESPONSE_NETWORK_ERROR") => "RESPONSE_NETWORK_ERROR",
                    Some("EXISTING_DRAFT" | "BOOTSTRAP_RICH_DRAFT_UNVERIFIED") => {
                        "INITIALIZATION_READBACK_HOME_DRAFT_PRESERVED"
                    }
                    Some("CHATGPT_NOT_IDLE") => "INITIALIZATION_READBACK_HOME_GENERATION_ACTIVE",
                    Some("BOOTSTRAP_HOME_NOT_READY" | "BOOTSTRAP_HOME_HAS_TURNS") => {
                        "INITIALIZATION_READBACK_HOME_STATE_UNPROVEN"
                    }
                    Some("INITIALIZATION_READBACK_FIXED_NAVIGATION_UNAVAILABLE") => {
                        "INITIALIZATION_READBACK_FIXED_NAVIGATION_UNAVAILABLE"
                    }
                    Some("INITIALIZATION_READBACK_TARGET_TAB_UNPROVEN") => {
                        "INITIALIZATION_READBACK_TARGET_TAB_UNPROVEN"
                    }
                    _ => "INITIALIZATION_READBACK_OBSERVATION_UNPROVEN",
                };
                return Err(reason.into());
            }
            Some(
                "CDP_REUSE_BEGIN"
                | "CDP_REUSE_DONE"
                | "CDP_ACTIVATE_BEGIN"
                | "CDP_ACTIVATE_DONE"
                | "GENERATION_WAIT"
                | "GENERATION_ACTIVE"
                | "RESPONSE_COMPLETED"
                | "RESPONSE_COMPLETED_AFTER_REOPEN",
            ) => {}
            _ => return Err("INITIALIZATION_READBACK_PROTOCOL_INVALID".into()),
        }
    }
    Err("INITIALIZATION_READBACK_TIMEOUT".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_readback_diagnostic_is_closed_and_exactly_scoped() {
        let request = InitializationReadbackRequest {
            project: "alpha",
            intent: "initial-one",
            registration: &"a".repeat(64),
            handoff: "original handoff",
            candidate: "https://chatgpt.com/c/original",
            native_turn_started_utc: 1,
        };
        let frame = serde_json::json!({"stage":"INITIALIZATION_READBACK_DIAGNOSTIC","schemaVersion":1,
            "projectId":request.project,"intentId":request.intent,"registrationDigest":request.registration,
            "messageDigest":BootstrapOrigin::InitialRegistration(request.registration).message_digest(request.project,request.intent,request.handoff).unwrap(),
            "candidateDigest":crate::protocol::digest(request.candidate),"phase":"INITIAL_OBSERVATION",
            "driverRoute":"SAME_CONVERSATION","documentRoute":"HOME"});
        let observed = readback_observation(&frame, &request).unwrap();
        assert_eq!(observed.document_route, "HOME");
        let mut selected = frame.clone();
        selected["stage"] = serde_json::json!("INITIALIZATION_READBACK_TAB_SELECTED");
        selected["phase"] = serde_json::json!("TAB_SELECTION");
        selected["documentRoute"] = serde_json::json!("SAME_CONVERSATION");
        selected["tab"] = serde_json::json!({"schemaVersion":1,"tabCount":2,"metadataMatches":1,"unreadableMetadata":0,
            "candidateShape":"STRING","candidateRoute":"EXACT","candidateFrame":"TOP",
            "controlledShape":"STRING","controlledRoute":"EXACT","alternateRoute":false,
            "stage":"VERIFIED","state":"TARGET_TAB_EXACT"});
        assert!(readback_observation(&selected, &request).is_ok());
        for (key, value) in [
            ("metadataMatches", serde_json::json!(2)),
            ("controlledRoute", serde_json::json!("HOME")),
            ("candidateRoute", serde_json::json!("OTHER")),
            ("state", serde_json::json!("TARGET_TAB_DRIFT")),
        ] {
            let mut invalid = selected.clone();
            invalid["tab"][key] = value;
            assert!(readback_observation(&invalid, &request).is_err());
        }
        let mut surfaced = frame.clone();
        surfaced["surface"] = serde_json::json!({"route":"HOME","document":"COMPLETE","history":"ABSENT",
            "editor":"MISSING","composer":"UNKNOWN","generation":"IDLE","assistant":"ABSENT",
            "exchange":"UNATTRIBUTED","recovery":"OBSERVATION_ONLY"});
        assert!(readback_observation(&surfaced, &request).is_ok());
        surfaced["surface"]["body"] = serde_json::json!("private context");
        assert!(readback_observation(&surfaced, &request).is_err());
        let mut reasoned = frame.clone();
        reasoned["failureCode"] = serde_json::json!("BOOTSTRAP_HOME_NOT_READY");
        assert_eq!(
            readback_observation(&reasoned, &request)
                .unwrap()
                .failure_code
                .as_deref(),
            Some("BOOTSTRAP_HOME_NOT_READY")
        );
        reasoned["failureCode"] = serde_json::json!("private context execution key");
        assert!(readback_observation(&reasoned, &request).is_err());
        for (key, value) in [
            ("projectId", serde_json::json!("other")),
            ("intentId", serde_json::json!("other")),
            ("registrationDigest", serde_json::json!("b".repeat(64))),
            ("messageDigest", serde_json::json!("b".repeat(64))),
            ("candidateDigest", serde_json::json!("b".repeat(64))),
            ("phase", serde_json::json!("private draft")),
            ("driverRoute", serde_json::json!(request.candidate)),
            ("documentRoute", serde_json::json!("private context")),
            ("schemaVersion", serde_json::json!(true)),
        ] {
            let mut changed = frame.clone();
            changed[key] = value;
            assert!(readback_observation(&changed, &request).is_err());
        }
        let mut recovered = frame.clone();
        recovered["stage"] = serde_json::json!("INITIALIZATION_READBACK_HOME_RECOVERED");
        recovered["phase"] = serde_json::json!("HOME_RECOVERY");
        recovered["documentRoute"] = serde_json::json!("SAME_CONVERSATION");
        assert!(readback_observation(&recovered, &request).is_ok());
        for field in ["driverRoute", "documentRoute"] {
            let mut invalid = recovered.clone();
            invalid[field] = serde_json::json!("HOME");
            assert!(readback_observation(&invalid, &request).is_err());
        }
        recovered["phase"] = serde_json::json!("INITIAL_OBSERVATION");
        assert!(readback_observation(&recovered, &request).is_err());
        let mut changed = frame;
        changed["body"] = serde_json::json!("private context");
        assert!(readback_observation(&changed, &request).is_err());
        assert!(
            !serde_json::to_string(&observed)
                .unwrap()
                .contains(request.candidate)
        );
    }

    fn frame() -> serde_json::Value {
        let url = "https://chatgpt.com/c/readback-candidate";
        serde_json::json!({"stage":"BOOTSTRAP_DESTINATION_OBSERVED","projectId":"alpha",
            "intentId":"initial-one","sourceGeneration":0,"sourceDigest":"a".repeat(64),
            "messageDigest":"b".repeat(64),"destinationUrl":url,
            "destinationDigest":crate::protocol::digest(url)})
    }

    #[test]
    fn bootstrap_hint_is_scope_bound_and_never_a_receipt() {
        let origin = BootstrapOrigin::InitialRegistration(&"a".repeat(64));
        let hint =
            destination_frame(&frame(), "alpha", "initial-one", origin, &"b".repeat(64)).unwrap();
        for (field, replacement) in [
            ("projectId", serde_json::json!("beta")),
            ("intentId", serde_json::json!("other")),
            ("sourceGeneration", serde_json::json!(1)),
            ("sourceDigest", serde_json::json!("c".repeat(64))),
            ("messageDigest", serde_json::json!("c".repeat(64))),
            ("destinationDigest", serde_json::json!("c".repeat(64))),
            (
                "destinationUrl",
                serde_json::json!("https://user:secret@chatgpt.com/c/other"),
            ),
        ] {
            let mut value = frame();
            value[field] = replacement;
            assert!(
                destination_frame(&value, "alpha", "initial-one", origin, &"b".repeat(64)).is_err()
            );
        }
        let mut extra = frame();
        extra["text"] = serde_json::json!("private context");
        assert!(
            destination_frame(&extra, "alpha", "initial-one", origin, &"b".repeat(64)).is_err()
        );
        let mut slot = None;
        capture_bootstrap_readback(
            &mut slot,
            &BootstrapCheckpoint::DestinationObserved(hint.clone()),
        );
        capture_bootstrap_readback(
            &mut slot,
            &BootstrapCheckpoint::PostSubmitDiagnostic(BootstrapDiagnostic::ResponseLoadError),
        );
        let saved = slot.unwrap();
        assert_eq!(saved.unverified_destination, Some(hint));
        assert_eq!(
            saved.post_submit_diagnostic,
            Some(BootstrapDiagnostic::ResponseLoadError)
        );
        assert!(
            !serde_json::to_string(&saved)
                .unwrap()
                .contains("EXACT_USER_MESSAGE_APPENDED")
        );
    }

    #[test]
    fn bootstrap_diagnostic_never_exports_unknown_reason_or_clears_uncertainty() {
        let unknown =
            diagnostic(&serde_json::json!({"reason":"PRIVATE_CONTEXT_AND_EXECUTION_KEY"}));
        assert_eq!(unknown, BootstrapDiagnostic::UnclassifiedPostSubmitFailure);
        assert!(
            !serde_json::to_string(&unknown)
                .unwrap()
                .contains("PRIVATE_CONTEXT")
        );
        assert_eq!(
            diagnostic(&serde_json::json!({"reason":"BOOTSTRAP_URL_UNISSUED"})),
            BootstrapDiagnostic::UrlUnissued
        );
        let mut called = 0;
        let result = outcome_unknown(
            &mut |_| {
                called += 1;
                Err("SAVE_FAILED".into())
            },
            unknown,
        );
        assert_eq!(called, 1);
        assert_eq!(result, "BOOTSTRAP_OUTCOME_UNKNOWN");
    }
}
