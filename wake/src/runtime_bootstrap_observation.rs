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

pub fn read_initialization_candidate_guarded(
    store: &Store,
    request: InitializationReadbackRequest<'_>,
    keep_running: &dyn Fn() -> Result<()>,
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
    while Instant::now() < deadline {
        let value = match guarded_browser_receive(&adapter.output, keep_running)? {
            Ok(Ok(value)) => value,
            Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("INITIALIZATION_READBACK_ADAPTER_EXITED".into());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        };
        match value["stage"].as_str() {
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
