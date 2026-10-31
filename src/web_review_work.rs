//! Local operator workflow for a frozen batch of routine source reviews.
//! Consent lives in WorkLedger; only the existing Wake sender can submit.
use super::*;
use crate::web_work_state::{ReviewDispatchPermit, ReviewGrantRequest, ReviewMessageIdentity};
use catdesk_wake::store::Store;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GrantInput {
    project_id: String,
    authorization: ReviewGrantRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RevokeInput {
    project_id: String,
    grant_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewInput {
    project_id: String,
    grant_id: String,
    review_key: String,
    request: Value,
}

fn with_project<T>(
    workspace: &Path,
    project: &str,
    target: &protocol::Target,
    action: impl FnOnce(&WorkLedger) -> Result<T, String>,
) -> Result<T, String> {
    let registry =
        AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
            .map_err(|_| "WEB_REVIEW_REGISTRY_UNAVAILABLE")?;
    registry.with_registration_lock(|| {
        let scope = resolve_registered_project_from_control_root(workspace, project)
            .map_err(|_| "WEB_REVIEW_PROJECT_UNAVAILABLE")?;
        if scope.project.chatgpt_target_url.as_deref() != Some(target.url.as_str())
            || scope.project.chatgpt_target_sha256.as_deref() != Some(target.digest.as_str())
        {
            return Err("WEB_REVIEW_TARGET_CHANGED".into());
        }
        action(&WorkLedger::open(workspace, project)?)
    })
}

fn target(workspace: &Path, project: &str) -> Result<protocol::Target, String> {
    let paired = mcp::operator_read_project_web_target(workspace, project)
        .map_err(|_| "WEB_REVIEW_PAIRED_TARGET_REQUIRED")?
        .ok_or("WEB_REVIEW_PAIRED_TARGET_REQUIRED")?;
    protocol::Target::new(paired.generation, paired.url)
}

fn require_stopped(store: &Store) -> Result<(), String> {
    let control: catdesk_wake::runtime::Control =
        catdesk_wake::store::read(&store.root().join("control.json"))?;
    if control.schema_version != 1 || control.desired != "STOPPED" {
        return Err("WEB_REVIEW_HOST_NOT_STOPPED".into());
    }
    Ok(())
}

// Caller retains the registered project lease, registry lock and stopped host
// lease. This prepares only the original reserved event; it cannot send/refund.
fn prepare_retry(
    store: &Store,
    ledger: &WorkLedger,
    project: &str,
    grant_id: &str,
    identity: &ReviewMessageIdentity,
    expected: &protocol::Target,
    message: &str,
) -> Result<Value, String> {
    ledger.resume_review(grant_id, identity, expected, now())?;
    let delivery = store
        .delivery(&identity.event_id)?
        .ok_or("WEB_REVIEW_RETRY_DELIVERY_MISSING")?;
    if delivery.phase != catdesk_wake::store::Phase::Attention
        || delivery.reason.as_deref() != Some("LOGIN_OR_PROFILE_REQUIRED")
        || delivery.receipt.is_some()
        || delivery.target != *expected
        || delivery.event.project_id != project
        || delivery.event.event_id != identity.event_id
        || delivery.event.target_generation != expected.generation
        || delivery.event.event_type != "source_review"
        || delivery.message_digest != identity.message_sha256
        || delivery.event.message != message
        || delivery.event.audit_references != [format!("catdesk-review-v1:{grant_id}")]
    {
        return Err("WEB_REVIEW_RETRY_NOT_PRE_SUBMIT".into());
    }
    if !store
        .events()?
        .iter()
        .any(|queued| queued == &delivery.event)
    {
        return Err("WEB_REVIEW_RETRY_QUEUE_MISSING".into());
    }
    // Wake repeats target, phase, receipt and absence-of-submission-evidence
    // checks under its existing transaction lock before changing this journal.
    store.retry_pre_submit(&identity.event_id)?;
    Ok(
        json!({"action":"web_review_retry_pre_submit","projectId":project,
        "requestId":identity.request_id,"eventId":identity.event_id,
        "grantId":grant_id,"phase":"CLAIMED","reservationPreserved":true,
        "browserStarted":false,"hostStarted":false,"sendAttempted":false,
        "nativeSessionAction":"NONE","productionAction":"NONE"}),
    )
}

pub(crate) fn action(workspace: &Path, operation: &str, args: Value) -> Result<Value, String> {
    let workspace = workspace
        .canonicalize()
        .map_err(|_| "WEB_REVIEW_CONTROL_UNAVAILABLE")?;
    if operation == "authorize" {
        let input: GrantInput =
            serde_json::from_value(args).map_err(|_| "WEB_REVIEW_ARGUMENTS_INVALID")?;
        // Consent cannot rebind a project or resolve a quarantined rollover.
        let lease = crate::operator_facade::acquire_web_work_lease(&workspace, &input.project_id)?;
        let target = target(&workspace, &input.project_id)?;
        lease.check_target(&target)?;
        if target != input.authorization.target {
            return Err("WEB_REVIEW_TARGET_CHANGED".into());
        }
        wake_protocol_client::preflight_registered_invitation(
            &workspace,
            &input.project_id,
            &target,
        )?;
        return with_project(&workspace, &input.project_id, &target, |ledger| {
            ledger.authorize_review(input.authorization, now())
        });
    }
    if operation == "revoke" {
        let input: RevokeInput =
            serde_json::from_value(args).map_err(|_| "WEB_REVIEW_ARGUMENTS_INVALID")?;
        // Revocation must remain available even after target drift or quarantine.
        let registry =
            AutonomousProjectRegistryStoreV1::open_read_only(workspace.join(".catdesk/projects"))
                .map_err(|_| "WEB_REVIEW_REGISTRY_UNAVAILABLE")?;
        return registry.with_registration_lock(|| {
            resolve_registered_project_from_control_root(&workspace, &input.project_id)
                .map_err(|_| "WEB_REVIEW_PROJECT_UNAVAILABLE")?;
            WorkLedger::open(&workspace, &input.project_id)?.revoke_review(&input.grant_id, now())
        });
    }
    if !matches!(
        operation,
        "preflight" | "run" | "resume" | "retry-pre-submit"
    ) {
        return Err("WEB_REVIEW_ACTION_INVALID".into());
    }
    let input: ReviewInput =
        serde_json::from_value(args).map_err(|_| "WEB_REVIEW_ARGUMENTS_INVALID")?;
    let request = WorkRequest::parse(input.request)?;
    if request.project_id != input.project_id {
        return Err("WEB_REVIEW_PROJECT_SCOPE_MISMATCH".into());
    }
    let message = request.message()?;
    let identity = ReviewMessageIdentity {
        request_id: request.request_id.clone(),
        event_id: request.event_id(),
        message_sha256: protocol::digest(&message),
        review_key: input.review_key,
    };
    let lease = crate::operator_facade::acquire_web_work_lease(&workspace, &input.project_id)?;
    let expected = target(&workspace, &input.project_id)?;
    if request.expected_target_generation != expected.generation
        || request.expected_target_sha256 != expected.digest
    {
        return Err("WEB_REVIEW_TARGET_CHANGED".into());
    }
    lease.check_target(&expected)?;
    wake_protocol_client::preflight_registered_invitation(
        &workspace,
        &input.project_id,
        &expected,
    )?;
    let store = Store::open(&wake_protocol_client::wake_root_for_workspace(&workspace)?)?;
    require_stopped(&store)?;
    if store
        .timers_for_project(&input.project_id)?
        .iter()
        .any(|timer| timer.response_state != catdesk_wake::store::ResponseState::Complete)
    {
        return Err("WEB_REVIEW_PROJECT_BUSY".into());
    }
    if operation == "retry-pre-submit" {
        let _host = store.host_lock()?;
        require_stopped(&store)?;
        return with_project(&workspace, &input.project_id, &expected, |ledger| {
            lease.check_target(&expected)?;
            prepare_retry(
                &store,
                ledger,
                &input.project_id,
                &input.grant_id,
                &identity,
                &expected,
                &message,
            )
        });
    }
    // A stable request can never acquire another send by changing its grant.
    let resume = operation == "resume";
    if store.delivery(&identity.event_id)?.is_some_and(|delivery| {
        !resume
            || delivery.phase != catdesk_wake::store::Phase::Claimed
            || delivery.target != expected
            || delivery.event.project_id != input.project_id
            || delivery.event.event_type != "source_review"
            || delivery.message_digest != identity.message_sha256
            || delivery.event.message != message
            || delivery.event.audit_references != [format!("catdesk-review-v1:{}", input.grant_id)]
    }) || store.timer(&identity.event_id)?.is_some()
    {
        return Err("WEB_REVIEW_REQUEST_ALREADY_PUBLISHED".into());
    }
    for event in store.events()? {
        if event.event_id == identity.event_id {
            if !resume
                || event.project_id != input.project_id
                || event.event_type != "source_review"
                || event.message != message
                || event.target_generation != expected.generation
                || event.audit_references != [format!("catdesk-review-v1:{}", input.grant_id)]
            {
                return Err("WEB_REVIEW_REQUEST_ALREADY_PUBLISHED".into());
            }
        } else if event.project_id == input.project_id
            && protocol::digest(&event.message) == identity.message_sha256
        {
            return Err("WEB_REVIEW_REQUEST_ALREADY_PUBLISHED".into());
        }
    }
    let permit: ReviewDispatchPermit =
        with_project(&workspace, &input.project_id, &expected, |ledger| {
            let permit = if resume {
                ledger.resume_review(&input.grant_id, &identity, &expected, now())?
            } else {
                ledger.preflight_review(&input.grant_id, &identity, &expected, now())?
            };
            if operation != "preflight" {
                if !resume {
                    ledger.reserve_review(&input.grant_id, &identity, &expected, now())?;
                }
                ledger.prepare(
                    &identity.request_id,
                    &identity.event_id,
                    &identity.message_sha256,
                    &expected,
                )?;
                wake_protocol_client::publish_registered_scoped_review(
                    &workspace,
                    &input.project_id,
                    &identity.event_id,
                    &message,
                    &expected,
                    &input.grant_id,
                )?;
            }
            Ok(permit)
        })?;
    if operation == "preflight" {
        return Ok(json!({"status":"ELIGIBLE","projectId":input.project_id,
            "permit":permit,"browserStarted":false,"sendAttempted":false,
            "platformApprovalGranted":false}));
    }
    let check_pair = || {
        if target(&workspace, &input.project_id)? != expected {
            return Err("WEB_REVIEW_TARGET_CHANGED".into());
        }
        lease.check_target(&expected)
    };
    let mut before_submit = |event: &protocol::Event, observed: &protocol::Target| {
        if event.project_id != input.project_id
            || event.event_id != identity.event_id
            || protocol::digest(&event.message) != identity.message_sha256
            || observed != &expected
            || event.audit_references != [format!("catdesk-review-v1:{}", input.grant_id)]
        {
            return Err("WEB_REVIEW_DISPATCH_SCOPE_CHANGED".into());
        }
        with_project(&workspace, &input.project_id, &expected, |ledger| {
            lease.check_target(&expected)?;
            if ledger.check_review(&input.grant_id, &identity, &expected, now())? != permit {
                return Err("WEB_REVIEW_GRANT_SCOPE_CHANGED".into());
            }
            ledger.mark_review_dispatch_started(&permit, now())
        })
    };
    let outcome = catdesk_wake::runtime::run_scoped_review(
        &store,
        &identity.event_id,
        permit.max_execution_seconds,
        &mut before_submit,
        &check_pair,
    );
    // Never retry or refund on an uncertain result. The existing Wake receipt
    // and timer remain the evidence for read-only observation/reconciliation.
    check_pair()?;
    outcome.map(|result| {
        json!({"action":"web_review_run","projectId":input.project_id,
        "requestId":identity.request_id,"eventId":identity.event_id,
        "grantId":permit.grant_id,"result":result,"nativeSessionAction":"NONE",
        "productionAction":"NONE","platformApprovalGranted":false})
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use catdesk_wake::store::Phase;
    use std::{collections::BTreeMap, fs, path::PathBuf};

    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    struct Fixture {
        store: Store,
        ledger: WorkLedger,
        input: ReviewInput,
        identity: ReviewMessageIdentity,
        target: protocol::Target,
        message: String,
        // Last field: stores/handles are dropped before fixture cleanup.
        root: Cleanup,
    }

    impl Fixture {
        fn new(expired: bool) -> Self {
            let root = Cleanup(
                std::env::temp_dir().join(format!("review-pre-submit-{}", uuid::Uuid::new_v4())),
            );
            fs::create_dir_all(root.0.join(".catdesk")).unwrap();
            let store = Store::open_scoped_for_test(&root.0.join("wake"), &root.0).unwrap();
            store.initialize().unwrap();
            let target = store
                .set_target("alpha", 0, "https://chatgpt.com/c/review-alpha")
                .unwrap();
            let value = json!({"projectId":"alpha","requestId":"review-one",
                "expectedTargetGeneration":target.generation,"expectedTargetSha256":target.digest,
                "taskText":"Read-only fixture source review"});
            let request = WorkRequest::parse(value.clone()).unwrap();
            let message = request.message().unwrap();
            let identity = ReviewMessageIdentity {
                request_id: request.request_id.clone(),
                event_id: request.event_id(),
                message_sha256: protocol::digest(&message),
                review_key: "a".repeat(64),
            };
            let input = ReviewInput {
                project_id: "alpha".into(),
                grant_id: "review-grant".into(),
                review_key: identity.review_key.clone(),
                request: value,
            };
            let ledger = WorkLedger::open(&root.0, "alpha").unwrap();
            let at = now();
            ledger
                .authorize_review(
                    ReviewGrantRequest {
                        grant_id: input.grant_id.clone(),
                        category: "READ_ONLY_SOURCE_REVIEW".into(),
                        target: target.clone(),
                        expires_utc: if expired { at - 1 } else { at + 600 },
                        max_messages: 1,
                        max_execution_seconds: 160,
                        approval_evidence_sha256: "b".repeat(64),
                        messages: vec![identity.clone()],
                    },
                    at - 5,
                )
                .unwrap();
            ledger
                .reserve_review(&input.grant_id, &identity, &target, at - 4)
                .unwrap();
            ledger
                .prepare(
                    &identity.request_id,
                    &identity.event_id,
                    &identity.message_sha256,
                    &target,
                )
                .unwrap();
            let event = store
                .produce_expected_with_audit(
                    &identity.event_id,
                    "alpha",
                    "source_review",
                    &message,
                    &target,
                    &[format!("catdesk-review-v1:{}", input.grant_id)],
                )
                .unwrap();
            let host = store.host_lock().unwrap();
            store.claim(&event, &host).unwrap();
            store
                .transition(
                    &event.event_id,
                    host.owner(),
                    Phase::Attention,
                    None,
                    Some("LOGIN_OR_PROFILE_REQUIRED".into()),
                )
                .unwrap();
            drop(host);
            Self {
                store,
                ledger,
                input,
                identity,
                target,
                message,
                root,
            }
        }

        fn prepare(&self) -> Result<Value, String> {
            let _host = self.store.host_lock()?;
            prepare_retry(
                &self.store,
                &self.ledger,
                &self.input.project_id,
                &self.input.grant_id,
                &self.identity,
                &self.target,
                &self.message,
            )
        }
    }

    fn bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        let mut files = BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_dir() {
                    pending.push(entry.path());
                } else {
                    files.insert(
                        entry.path().strip_prefix(root).unwrap().to_path_buf(),
                        fs::read(entry.path()).unwrap(),
                    );
                }
            }
        }
        files
    }

    #[test]
    fn retry_preserves_reserved_message_and_independent_project_without_browser() {
        let fixture = Fixture::new(false);
        let beta = fixture
            .store
            .set_target("beta", 0, "https://chatgpt.com/c/review-beta")
            .unwrap();
        let event = fixture
            .store
            .produce_expected(
                "beta-blocked",
                "beta",
                "test",
                "Other project is blocked",
                &beta,
            )
            .unwrap();
        let host = fixture.store.host_lock().unwrap();
        fixture.store.claim(&event, &host).unwrap();
        fixture
            .store
            .transition(
                &event.event_id,
                host.owner(),
                Phase::Attention,
                None,
                Some("CHATGPT_NOT_IDLE".into()),
            )
            .unwrap();
        drop(host);
        let beta_before = fixture.store.delivery(&event.event_id).unwrap();
        let ledger_path = fixture.root.0.join(".catdesk/web-work/alpha.json");
        let ledger_before = fs::read(&ledger_path).unwrap();
        let events_before = fixture.store.events().unwrap();
        let before = fixture
            .store
            .delivery(&fixture.identity.event_id)
            .unwrap()
            .unwrap();
        let result = fixture.prepare().unwrap();
        assert_eq!(result["phase"], "CLAIMED");
        assert_eq!(result["sendAttempted"], false);
        assert_eq!(result["browserStarted"], false);
        assert_eq!(result["hostStarted"], false);
        let after = fixture
            .store
            .delivery(&fixture.identity.event_id)
            .unwrap()
            .unwrap();
        assert_eq!(after.phase, Phase::Claimed);
        assert_eq!(after.reason, None);
        assert_eq!(after.event, before.event);
        assert_eq!(after.target, before.target);
        assert_eq!(after.owner, before.owner);
        assert_eq!(after.message_digest, before.message_digest);
        assert_eq!(fs::read(ledger_path).unwrap(), ledger_before);
        assert_eq!(fixture.store.events().unwrap(), events_before);
        assert_eq!(
            fixture.store.delivery(&event.event_id).unwrap(),
            beta_before
        );
        let prepared = bytes(&fixture.root.0);
        assert!(
            fixture.prepare().is_err(),
            "already prepared retry must be refused"
        );
        assert_eq!(bytes(&fixture.root.0), prepared);
    }

    #[test]
    fn retry_refuses_changed_expired_revoked_unknown_or_started_work_without_writes() {
        for case in [
            "expired",
            "revoked",
            "dispatch",
            "project",
            "target",
            "message",
            "key",
            "grant",
            "reason",
            "queue",
            "submitting",
            "start",
            "timer",
            "archive",
            "audit",
        ] {
            let mut fixture = Fixture::new(case == "expired");
            match case {
                "revoked" => {
                    fixture
                        .ledger
                        .revoke_review(&fixture.input.grant_id, now())
                        .unwrap();
                }
                "dispatch" => {
                    let permit = fixture
                        .ledger
                        .resume_review(
                            &fixture.input.grant_id,
                            &fixture.identity,
                            &fixture.target,
                            now(),
                        )
                        .unwrap();
                    fixture
                        .ledger
                        .mark_review_dispatch_started(&permit, now())
                        .unwrap();
                }
                "project" => fixture.input.project_id = "beta".into(),
                "target" => {
                    fixture.target =
                        protocol::Target::new(2, "https://chatgpt.com/c/other".into()).unwrap()
                }
                "message" => fixture.message.push_str(" material change"),
                "key" => fixture.identity.review_key = "c".repeat(64),
                "grant" => fixture.input.grant_id = "other-grant".into(),
                "queue" => fs::remove_file(
                    fixture
                        .store
                        .root()
                        .join("queue")
                        .join(format!("{}.json", fixture.identity.event_id)),
                )
                .unwrap(),
                "submitting" => {
                    let delivery = fixture
                        .store
                        .delivery(&fixture.identity.event_id)
                        .unwrap()
                        .unwrap();
                    fixture
                        .store
                        .retry_pre_submit(&fixture.identity.event_id)
                        .unwrap();
                    fixture
                        .store
                        .transition(
                            &fixture.identity.event_id,
                            &delivery.owner,
                            Phase::Submitting,
                            None,
                            None,
                        )
                        .unwrap();
                }
                "start" | "timer" | "archive" => {
                    let path = match case {
                        "start" => fixture.store.root().join("logs").join(format!(
                            "submission-start-{}.json",
                            fixture.identity.event_id
                        )),
                        "timer" => fixture
                            .store
                            .root()
                            .join("turns")
                            .join(format!("{}.json", fixture.identity.event_id)),
                        _ => fixture
                            .store
                            .root()
                            .join("archive")
                            .join(format!("{}.json", fixture.identity.event_id)),
                    };
                    fs::write(path, b"{partial").unwrap();
                }
                "reason" | "audit" => {
                    let path = fixture
                        .store
                        .root()
                        .join("deliveries")
                        .join(format!("{}.json", fixture.identity.event_id));
                    let mut value: Value =
                        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                    if case == "reason" {
                        value["reason"] = json!("UNKNOWN_SUBMISSION");
                    } else {
                        value["event"]["auditReferences"] =
                            json!(["catdesk-review-v1:other-grant"]);
                    }
                    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
                }
                _ => {}
            }
            let before = bytes(&fixture.root.0);
            assert!(
                fixture.prepare().is_err(),
                "unsafe recovery admitted: {case}"
            );
            assert_eq!(
                bytes(&fixture.root.0),
                before,
                "refusal mutated state: {case}"
            );
        }
    }
}
