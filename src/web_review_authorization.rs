//! Exact-message, read-only review consent in the existing project work ledger.
//! Callers retain the same registration lock required by every ledger mutation.
//! A grant permits dispatch only: it is never review, build or release evidence.
use super::*;
use std::collections::BTreeSet;

const MAX_GRANTS: usize = 32;
const MAX_MESSAGES: usize = 32;
const MAX_GRANT_SECONDS: u64 = 31 * 24 * 60 * 60;
const CATEGORY: &str = "READ_ONLY_SOURCE_REVIEW";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewMessageIdentity {
    pub(crate) request_id: String,
    pub(crate) event_id: String,
    pub(crate) message_sha256: String,
    pub(crate) review_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReviewGrantRequest {
    pub(crate) grant_id: String,
    pub(crate) category: String,
    pub(crate) target: Target,
    pub(crate) expires_utc: u64,
    pub(crate) max_messages: usize,
    pub(crate) max_execution_seconds: u64,
    pub(crate) approval_evidence_sha256: String,
    pub(crate) messages: Vec<ReviewMessageIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewReservation {
    message: ReviewMessageIdentity,
    reserved_utc: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dispatch_started_utc: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReviewGrant {
    request: ReviewGrantRequest,
    created_utc: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revoked_utc: Option<u64>,
    reservations: Vec<ReviewReservation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewDispatchPermit {
    pub(crate) grant_id: String,
    pub(crate) request_id: String,
    pub(crate) event_id: String,
    pub(crate) message_sha256: String,
    pub(crate) review_key: String,
    pub(crate) target: Target,
    pub(crate) max_execution_seconds: u64,
}

fn valid_message(message: &ReviewMessageIdentity, project: &str) -> bool {
    valid_request(&message.request_id)
        && message.event_id == expected_event(project, &message.request_id)
        && sha256(&message.message_sha256)
        && sha256(&message.review_key)
}

fn valid_request_scope(request: &ReviewGrantRequest, project: &str, created: u64) -> bool {
    valid_request(&request.grant_id)
        && request.category == CATEGORY
        && request.target.validate().is_ok()
        && created > 0
        && request.expires_utc > created
        && request.expires_utc.saturating_sub(created) <= MAX_GRANT_SECONDS
        && (1..=MAX_MESSAGES).contains(&request.max_messages)
        && (1..=180).contains(&request.max_execution_seconds)
        && sha256(&request.approval_evidence_sha256)
        && !request.messages.is_empty()
        && request.messages.len() <= MAX_MESSAGES
        && request
            .messages
            .iter()
            .all(|message| valid_message(message, project))
}

pub(super) fn validate_grants(grants: &[ReviewGrant], project: &str) -> Result<(), String> {
    if grants.len() > MAX_GRANTS {
        return Err("WEB_REVIEW_GRANTS_INVALID".into());
    }
    let mut grant_ids = BTreeSet::new();
    let mut request_ids = BTreeSet::new();
    let mut event_ids = BTreeSet::new();
    let mut message_hashes = BTreeSet::new();
    let mut review_keys = BTreeSet::new();
    for grant in grants {
        if !valid_request_scope(&grant.request, project, grant.created_utc)
            || !grant_ids.insert(&grant.request.grant_id)
            || grant.revoked_utc.is_some_and(|at| at < grant.created_utc)
            || grant.reservations.len() > grant.request.max_messages
        {
            return Err("WEB_REVIEW_GRANTS_INVALID".into());
        }
        for message in &grant.request.messages {
            // Reauthorization never resets an old request, even after revocation.
            if !request_ids.insert(&message.request_id)
                || !event_ids.insert(&message.event_id)
                || !message_hashes.insert(&message.message_sha256)
                || !review_keys.insert(&message.review_key)
            {
                return Err("WEB_REVIEW_GRANTS_INVALID".into());
            }
        }
        let mut consumed = BTreeSet::new();
        for reservation in &grant.reservations {
            if !grant.request.messages.contains(&reservation.message)
                || !consumed.insert(&reservation.message.request_id)
                || reservation.reserved_utc < grant.created_utc
                || reservation.reserved_utc >= grant.request.expires_utc
                || reservation.dispatch_started_utc.is_some_and(|at| {
                    at < reservation.reserved_utc || at >= grant.request.expires_utc
                })
                || grant.revoked_utc.is_some_and(|at| {
                    reservation.reserved_utc > at
                        || reservation
                            .dispatch_started_utc
                            .is_some_and(|started| started > at)
                })
            {
                return Err("WEB_REVIEW_GRANTS_INVALID".into());
            }
        }
    }
    Ok(())
}

impl ReviewGrant {
    fn permit(
        &self,
        message: &ReviewMessageIdentity,
        target: &Target,
        now: u64,
    ) -> Result<ReviewDispatchPermit, String> {
        if now < self.created_utc || now >= self.request.expires_utc {
            return Err("WEB_REVIEW_GRANT_EXPIRED".into());
        }
        if self.revoked_utc.is_some() {
            return Err("WEB_REVIEW_GRANT_REVOKED".into());
        }
        if self.request.target != *target || !self.request.messages.contains(message) {
            return Err("WEB_REVIEW_GRANT_SCOPE_CHANGED".into());
        }
        if self.reservations.iter().any(|reservation| {
            reservation.message == *message && reservation.dispatch_started_utc.is_some()
        }) {
            return Err("WEB_REVIEW_DISPATCH_ALREADY_STARTED".into());
        }
        Ok(ReviewDispatchPermit {
            grant_id: self.request.grant_id.clone(),
            request_id: message.request_id.clone(),
            event_id: message.event_id.clone(),
            message_sha256: message.message_sha256.clone(),
            review_key: message.review_key.clone(),
            target: target.clone(),
            max_execution_seconds: self.request.max_execution_seconds,
        })
    }

    fn summary(&self, idempotent: bool) -> Value {
        json!({"grantId":self.request.grant_id,"category":CATEGORY,
            "target":self.request.target,"messages":self.request.messages,
            "approvalEvidenceSha256":self.request.approval_evidence_sha256,
            "createdUtc":self.created_utc,"expiresUtc":self.request.expires_utc,
            "maxMessages":self.request.max_messages,"reservedMessages":self.reservations.len(),
            "maxExecutionSeconds":self.request.max_execution_seconds,"revokedUtc":self.revoked_utc,
            "idempotent":idempotent,"reviewVerified":false,"releaseAuthorized":false})
    }
}

fn preflight(
    ledger: &LedgerFile,
    grant_id: &str,
    message: &ReviewMessageIdentity,
    target: &Target,
    now: u64,
) -> Result<ReviewDispatchPermit, String> {
    if ledger.records.iter().any(|record| record.phase == "OPEN") {
        return Err("WEB_WORK_ACTIVE_REQUEST".into());
    }
    if ledger.records.iter().any(|record| {
        record.request_id == message.request_id
            || record.event_id == message.event_id
            || record.message_sha256 == message.message_sha256
    }) {
        return Err("WEB_REVIEW_REQUEST_ALREADY_EXISTS".into());
    }
    let grant = ledger
        .review_authorizations
        .iter()
        .find(|grant| grant.request.grant_id == grant_id)
        .ok_or("WEB_REVIEW_GRANT_NOT_FOUND")?;
    let permit = grant.permit(message, target, now)?;
    if grant
        .reservations
        .iter()
        .any(|saved| saved.message == *message)
    {
        return Err("WEB_REVIEW_REQUEST_ALREADY_RESERVED".into());
    }
    if grant.reservations.len() >= grant.request.max_messages {
        return Err("WEB_REVIEW_GRANT_EXHAUSTED".into());
    }
    Ok(permit)
}

impl WorkLedger {
    /// Retrospective dispatch evidence for the caller's source_review event.
    /// The caller still verifies the Wake SENT receipt and response; this is
    /// neither send permission nor proof of native task completion. Revocation
    /// and expiry do not erase a previously recorded dispatch boundary.
    pub(crate) fn review_dispatch_recorded(
        &self,
        record: &WorkRecord,
        audit_references: &[String],
    ) -> Result<bool, String> {
        let ledger = self.load()?;
        if audit_references.len() != 1 || !ledger.records.iter().any(|saved| saved == record) {
            return Ok(false);
        }
        Ok(ledger.review_authorizations.iter().any(|grant| {
            grant.request.target == record.target
                && audit_references[0] == format!("catdesk-review-v1:{}", grant.request.grant_id)
                && grant.reservations.iter().any(|reservation| {
                    reservation.dispatch_started_utc.is_some()
                        && reservation.message.request_id == record.request_id
                        && reservation.message.event_id == record.event_id
                        && reservation.message.message_sha256 == record.message_sha256
                        && grant.request.messages.contains(&reservation.message)
                })
        }))
    }

    /// Generic publication cannot take over an exact request governed by a
    /// review grant, even after that grant expires or is revoked.
    pub(crate) fn require_ordinary_request(&self, request_id: &str) -> Result<(), String> {
        if !valid_request(request_id) {
            return Err("WEB_WORK_LEDGER_ARGUMENTS_INVALID".into());
        }
        if self.load()?.review_authorizations.iter().any(|grant| {
            grant
                .request
                .messages
                .iter()
                .any(|message| message.request_id == request_id)
        }) {
            return Err("WEB_REVIEW_SCOPED_WORKFLOW_REQUIRED".into());
        }
        Ok(())
    }

    pub(crate) fn authorize_review(
        &self,
        request: ReviewGrantRequest,
        now: u64,
    ) -> Result<Value, String> {
        let mut ledger = self.load()?;
        if let Some(existing) = ledger
            .review_authorizations
            .iter()
            .find(|grant| grant.request.grant_id == request.grant_id)
        {
            return if existing.request == request {
                Ok(existing.summary(true))
            } else {
                Err("WEB_REVIEW_GRANT_CONFLICT".into())
            };
        }
        if !valid_request_scope(&request, &self.project_id, now) {
            return Err("WEB_REVIEW_GRANT_ARGUMENTS_INVALID".into());
        }
        if request.messages.iter().any(|message| {
            ledger.records.iter().any(|record| {
                record.request_id == message.request_id || record.event_id == message.event_id
            })
        }) {
            return Err("WEB_REVIEW_REQUEST_ALREADY_EXISTS".into());
        }
        ledger.review_authorizations.push(ReviewGrant {
            request,
            created_utc: now,
            revoked_utc: None,
            reservations: Vec::new(),
        });
        validate_grants(&ledger.review_authorizations, &self.project_id)?;
        let result = ledger
            .review_authorizations
            .last()
            .expect("pushed grant")
            .summary(false);
        self.save(&ledger)?;
        Ok(result)
    }

    pub(crate) fn revoke_review(&self, grant_id: &str, now: u64) -> Result<Value, String> {
        let mut ledger = self.load()?;
        let grant = ledger
            .review_authorizations
            .iter_mut()
            .find(|grant| grant.request.grant_id == grant_id)
            .ok_or("WEB_REVIEW_GRANT_NOT_FOUND")?;
        if now < grant.created_utc {
            return Err("WEB_REVIEW_GRANT_CLOCK_INVALID".into());
        }
        let idempotent = grant.revoked_utc.is_some();
        if !idempotent {
            grant.revoked_utc = Some(now);
        }
        let result = grant.summary(idempotent);
        if !idempotent {
            self.save(&ledger)?;
        }
        Ok(result)
    }

    /// Read-only recovery of an exact RESERVED request is allowed; this is not
    /// a send boundary. DISPATCH_STARTED and every ambiguous outcome stay spent.
    pub(crate) fn check_review(
        &self,
        grant_id: &str,
        message: &ReviewMessageIdentity,
        target: &Target,
        now: u64,
    ) -> Result<ReviewDispatchPermit, String> {
        let ledger = self.load()?;
        let grant = ledger
            .review_authorizations
            .iter()
            .find(|grant| grant.request.grant_id == grant_id)
            .ok_or("WEB_REVIEW_GRANT_NOT_FOUND")?;
        let permit = grant.permit(message, target, now)?;
        if !grant
            .reservations
            .iter()
            .any(|saved| saved.message == *message)
            && grant.reservations.len() >= grant.request.max_messages
        {
            return Err("WEB_REVIEW_GRANT_EXHAUSTED".into());
        }
        Ok(permit)
    }

    /// Qualification before reservation; unlike check_review, an already
    /// consumed reservation is refused even when dispatch has not started.
    pub(crate) fn preflight_review(
        &self,
        grant_id: &str,
        message: &ReviewMessageIdentity,
        target: &Target,
        now: u64,
    ) -> Result<ReviewDispatchPermit, String> {
        preflight(&self.load()?, grant_id, message, target, now)
    }

    /// Recover a reserved pre-submit workflow without spending another quota
    /// unit. The caller must independently prove there is no unknown browser
    /// submission and must use the same guarded before-submit callback.
    /// An exact OPEN record may exist if the previous process stopped between
    /// prepare and queue publication. A closed, altered or unrelated owner may
    /// never be replaced by this path.
    pub(crate) fn resume_review(
        &self,
        grant_id: &str,
        message: &ReviewMessageIdentity,
        target: &Target,
        now: u64,
    ) -> Result<ReviewDispatchPermit, String> {
        let ledger = self.load()?;
        let grant = ledger
            .review_authorizations
            .iter()
            .find(|grant| grant.request.grant_id == grant_id)
            .ok_or("WEB_REVIEW_GRANT_NOT_FOUND")?;
        let permit = grant.permit(message, target, now)?;
        if !grant
            .reservations
            .iter()
            .any(|saved| saved.message == *message)
        {
            return Err("WEB_REVIEW_RESERVATION_REQUIRED".into());
        }
        for record in &ledger.records {
            if record.request_id == message.request_id || record.event_id == message.event_id {
                if record.phase != "OPEN"
                    || record.request_id != message.request_id
                    || record.event_id != message.event_id
                    || record.message_sha256 != message.message_sha256
                    || record.target != *target
                {
                    return Err("WEB_REVIEW_PREPARED_WORK_CHANGED".into());
                }
            } else if record.phase == "OPEN" {
                return Err("WEB_WORK_ACTIVE_REQUEST".into());
            } else if record.message_sha256 == message.message_sha256 {
                return Err("WEB_REVIEW_REQUEST_ALREADY_EXISTS".into());
            }
        }
        Ok(permit)
    }

    pub(crate) fn reserve_review(
        &self,
        grant_id: &str,
        message: &ReviewMessageIdentity,
        target: &Target,
        now: u64,
    ) -> Result<ReviewDispatchPermit, String> {
        let mut ledger = self.load()?;
        let permit = preflight(&ledger, grant_id, message, target, now)?;
        let grant = ledger
            .review_authorizations
            .iter_mut()
            .find(|grant| grant.request.grant_id == grant_id)
            .ok_or("WEB_REVIEW_GRANT_NOT_FOUND")?;
        grant.reservations.push(ReviewReservation {
            message: message.clone(),
            reserved_utc: now,
            dispatch_started_utc: None,
        });
        self.save(&ledger)?;
        Ok(permit)
    }

    pub(crate) fn mark_review_dispatch_started(
        &self,
        permit: &ReviewDispatchPermit,
        now: u64,
    ) -> Result<(), String> {
        let mut ledger = self.load()?;
        let record = ledger
            .records
            .iter()
            .find(|record| record.request_id == permit.request_id)
            .ok_or("WEB_REVIEW_PREPARED_WORK_REQUIRED")?;
        if record.phase != "OPEN"
            || record.event_id != permit.event_id
            || record.message_sha256 != permit.message_sha256
            || record.target != permit.target
        {
            return Err("WEB_REVIEW_PREPARED_WORK_CHANGED".into());
        }
        let grant = ledger
            .review_authorizations
            .iter_mut()
            .find(|grant| grant.request.grant_id == permit.grant_id)
            .ok_or("WEB_REVIEW_GRANT_NOT_FOUND")?;
        let message = ReviewMessageIdentity {
            request_id: permit.request_id.clone(),
            event_id: permit.event_id.clone(),
            message_sha256: permit.message_sha256.clone(),
            review_key: permit.review_key.clone(),
        };
        if grant.permit(&message, &permit.target, now)? != *permit {
            return Err("WEB_REVIEW_GRANT_SCOPE_CHANGED".into());
        }
        let reservation = grant
            .reservations
            .iter_mut()
            .find(|saved| saved.message == message)
            .ok_or("WEB_REVIEW_RESERVATION_REQUIRED")?;
        if now < reservation.reserved_utc {
            return Err("WEB_REVIEW_GRANT_CLOCK_INVALID".into());
        }
        reservation.dispatch_started_utc = Some(now);
        self.save(&ledger)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(project: &str) -> (PathBuf, WorkLedger, ReviewGrantRequest) {
        let root = std::env::temp_dir().join(format!("web-review-grant-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join(".catdesk")).unwrap();
        let ledger = WorkLedger::open(&root, project).unwrap();
        let request = ReviewGrantRequest {
            grant_id: "grant-one".into(),
            category: CATEGORY.into(),
            target: Target::new(1, "https://chatgpt.com/c/grant-fixture".into()).unwrap(),
            expires_utc: 200,
            max_messages: 1,
            max_execution_seconds: 180,
            approval_evidence_sha256: "a".repeat(64),
            messages: vec![ReviewMessageIdentity {
                request_id: "review-one".into(),
                event_id: expected_event(project, "review-one"),
                message_sha256: "b".repeat(64),
                review_key: "e".repeat(64),
            }],
        };
        (root, ledger, request)
    }

    #[test]
    fn reservation_survives_restart_and_dispatch_is_never_replayed() {
        let (root, ledger, request) = fixture("alpha");
        ledger.authorize_review(request.clone(), 100).unwrap();
        let permit = ledger
            .reserve_review(
                &request.grant_id,
                &request.messages[0],
                &request.target,
                101,
            )
            .unwrap();
        let reopened = WorkLedger::open(&root, "alpha").unwrap();
        assert!(
            reopened
                .reserve_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    102
                )
                .is_err()
        );
        assert_eq!(
            reopened
                .check_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    102
                )
                .unwrap(),
            permit
        );
        assert!(reopened.mark_review_dispatch_started(&permit, 102).is_err());
        reopened
            .prepare(
                &permit.request_id,
                &permit.event_id,
                &permit.message_sha256,
                &permit.target,
            )
            .unwrap();
        reopened.mark_review_dispatch_started(&permit, 103).unwrap();
        let spent = fs::read(&reopened.path).unwrap();
        assert!(reopened.mark_review_dispatch_started(&permit, 104).is_err());
        assert!(
            reopened
                .check_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    104
                )
                .is_err()
        );
        assert_eq!(fs::read(&reopened.path).unwrap(), spent);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn revoked_expired_and_changed_scopes_never_reauthorize_consumption() {
        let (root, ledger, request) = fixture("alpha");
        ledger.authorize_review(request.clone(), 100).unwrap();
        let permit = ledger
            .reserve_review(
                &request.grant_id,
                &request.messages[0],
                &request.target,
                101,
            )
            .unwrap();
        ledger
            .prepare(
                &permit.request_id,
                &permit.event_id,
                &permit.message_sha256,
                &permit.target,
            )
            .unwrap();
        assert!(ledger.mark_review_dispatch_started(&permit, 200).is_err());
        let mut changed = permit.clone();
        changed.max_execution_seconds = 1;
        assert!(ledger.mark_review_dispatch_started(&changed, 102).is_err());
        ledger.revoke_review(&request.grant_id, 103).unwrap();
        let frozen = fs::read(&ledger.path).unwrap();
        ledger.authorize_review(request.clone(), 104).unwrap();
        ledger.revoke_review(&request.grant_id, 104).unwrap();
        assert_eq!(fs::read(&ledger.path).unwrap(), frozen);
        assert!(ledger.mark_review_dispatch_started(&permit, 105).is_err());
        let mut new_grant = request.clone();
        new_grant.grant_id = "replacement".into();
        assert!(ledger.authorize_review(new_grant, 104).is_err());
        let wrong = Target::new(2, "https://chatgpt.com/c/other".into()).unwrap();
        assert!(
            ledger
                .check_review(&request.grant_id, &request.messages[0], &wrong, 102)
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn category_limits_quota_and_project_identity_are_closed() {
        let (root, ledger, request) = fixture("alpha");
        for mutation in 0..5 {
            let mut bad = request.clone();
            match mutation {
                0 => bad.category = "PROMOTION".into(),
                1 => bad.max_messages = 33,
                2 => bad.max_execution_seconds = 181,
                3 => bad.expires_utc = 100 + MAX_GRANT_SECONDS + 1,
                _ => bad.messages[0].event_id = expected_event("beta", "review-one"),
            };
            assert!(ledger.authorize_review(bad, 100).is_err());
            assert!(!ledger.path.exists());
        }
        let mut limited = request.clone();
        limited.messages.push(ReviewMessageIdentity {
            request_id: "review-two".into(),
            event_id: expected_event("alpha", "review-two"),
            message_sha256: "c".repeat(64),
            review_key: "f".repeat(64),
        });
        ledger.authorize_review(limited.clone(), 100).unwrap();
        ledger
            .reserve_review(
                &limited.grant_id,
                &limited.messages[0],
                &limited.target,
                101,
            )
            .unwrap();
        assert!(
            ledger
                .reserve_review(
                    &limited.grant_id,
                    &limited.messages[1],
                    &limited.target,
                    102
                )
                .is_err()
        );
        let beta = WorkLedger::open(&root, "beta").unwrap();
        assert!(
            beta.check_review(
                &limited.grant_id,
                &limited.messages[0],
                &limited.target,
                102
            )
            .is_err()
        );
        assert!(!beta.path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_work_bytes_stay_exact_and_preexisting_work_cannot_be_granted() {
        let (root, ledger, request) = fixture("alpha");
        ledger
            .prepare(
                "legacy",
                &expected_event("alpha", "legacy"),
                &"c".repeat(64),
                &request.target,
            )
            .unwrap();
        let before = fs::read(&ledger.path).unwrap();
        assert!(
            !String::from_utf8(before.clone())
                .unwrap()
                .contains("reviewAuthorizations")
        );
        let decoded = ledger.load().unwrap();
        assert!(decoded.review_authorizations.is_empty());
        assert_eq!(
            serde_json::to_vec(&decoded).unwrap(),
            before[..before.len() - 1]
        );
        let mut reused = request.clone();
        reused.messages[0] = ReviewMessageIdentity {
            request_id: "legacy".into(),
            event_id: expected_event("alpha", "legacy"),
            message_sha256: "c".repeat(64),
            review_key: "f".repeat(64),
        };
        assert!(ledger.authorize_review(reused, 100).is_err());
        assert_eq!(fs::read(&ledger.path).unwrap(), before);
        ledger.authorize_review(request.clone(), 100).unwrap();
        assert_eq!(
            ledger.record("legacy").unwrap().unwrap().message_sha256,
            "c".repeat(64)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preflight_and_reservation_refuse_active_and_equivalent_prior_work() {
        let (root, ledger, request) = fixture("alpha");
        ledger.authorize_review(request.clone(), 100).unwrap();
        ledger
            .prepare(
                "unrelated",
                &expected_event("alpha", "unrelated"),
                &"d".repeat(64),
                &request.target,
            )
            .unwrap();
        let before = fs::read(&ledger.path).unwrap();
        for reserve in [false, true] {
            let result = if reserve {
                ledger.reserve_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    101,
                )
            } else {
                ledger.preflight_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    101,
                )
            };
            assert_eq!(result.unwrap_err(), "WEB_WORK_ACTIVE_REQUEST");
            assert_eq!(fs::read(&ledger.path).unwrap(), before);
        }
        ledger
            .resolve(
                "unrelated",
                &request.target,
                "retired",
                "finished unrelated request",
            )
            .unwrap();
        assert!(
            ledger
                .preflight_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    102
                )
                .is_ok()
        );
        ledger
            .prepare(
                "equivalent",
                &expected_event("alpha", "equivalent"),
                &request.messages[0].message_sha256,
                &request.target,
            )
            .unwrap();
        ledger
            .resolve(
                "equivalent",
                &request.target,
                "retired",
                "retired equivalent request",
            )
            .unwrap();
        assert_eq!(
            ledger
                .preflight_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    103
                )
                .unwrap_err(),
            "WEB_REVIEW_REQUEST_ALREADY_EXISTS"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn equivalent_review_key_cannot_be_reissued_under_new_ids_or_wire_hash() {
        let (root, ledger, request) = fixture("alpha");
        ledger.authorize_review(request.clone(), 100).unwrap();
        ledger
            .reserve_review(
                &request.grant_id,
                &request.messages[0],
                &request.target,
                101,
            )
            .unwrap();
        ledger.revoke_review(&request.grant_id, 102).unwrap();
        let before = fs::read(&ledger.path).unwrap();
        let mut renamed = request.clone();
        renamed.grant_id = "grant-renamed".into();
        renamed.messages[0].request_id = "review-renamed".into();
        renamed.messages[0].event_id = expected_event("alpha", "review-renamed");
        renamed.messages[0].message_sha256 = "d".repeat(64);
        assert!(ledger.authorize_review(renamed, 103).is_err());
        assert_eq!(fs::read(&ledger.path).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reserved_review_resumes_before_or_after_prepare_without_refund_or_replay() {
        let (root, ledger, request) = fixture("alpha");
        ledger.authorize_review(request.clone(), 100).unwrap();
        assert_eq!(
            ledger
                .resume_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    101
                )
                .unwrap_err(),
            "WEB_REVIEW_RESERVATION_REQUIRED"
        );
        let permit = ledger
            .reserve_review(
                &request.grant_id,
                &request.messages[0],
                &request.target,
                101,
            )
            .unwrap();
        let before = fs::read(&ledger.path).unwrap();
        assert_eq!(
            ledger
                .resume_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    102
                )
                .unwrap(),
            permit
        );
        assert_eq!(fs::read(&ledger.path).unwrap(), before);
        ledger
            .prepare(
                &permit.request_id,
                &permit.event_id,
                &permit.message_sha256,
                &permit.target,
            )
            .unwrap();
        let before = fs::read(&ledger.path).unwrap();
        let resumed = WorkLedger::open(&root, "alpha")
            .unwrap()
            .resume_review(
                &request.grant_id,
                &request.messages[0],
                &request.target,
                103,
            )
            .unwrap();
        assert_eq!(resumed, permit);
        assert_eq!(fs::read(&ledger.path).unwrap(), before);
        assert_eq!(
            ledger.load().unwrap().review_authorizations[0]
                .reservations
                .len(),
            1
        );
        ledger.mark_review_dispatch_started(&resumed, 104).unwrap();
        let spent = fs::read(&ledger.path).unwrap();
        assert_eq!(
            ledger
                .resume_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    105
                )
                .unwrap_err(),
            "WEB_REVIEW_DISPATCH_ALREADY_STARTED"
        );
        assert_eq!(fs::read(&ledger.path).unwrap(), spent);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resume_never_replaces_changed_closed_unrelated_revoked_or_expired_work() {
        for state in ["changed", "closed", "unrelated", "revoked", "expired"] {
            let (root, ledger, request) = fixture("alpha");
            ledger.authorize_review(request.clone(), 100).unwrap();
            ledger
                .reserve_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    101,
                )
                .unwrap();
            if state == "unrelated" {
                ledger
                    .prepare(
                        "another",
                        &expected_event("alpha", "another"),
                        &"f".repeat(64),
                        &request.target,
                    )
                    .unwrap();
            } else {
                let hash = if state == "changed" {
                    "f".repeat(64)
                } else {
                    request.messages[0].message_sha256.clone()
                };
                ledger
                    .prepare(
                        &request.messages[0].request_id,
                        &request.messages[0].event_id,
                        &hash,
                        &request.target,
                    )
                    .unwrap();
            }
            if state == "closed" {
                ledger
                    .resolve(
                        &request.messages[0].request_id,
                        &request.target,
                        "retired",
                        "explicitly retired",
                    )
                    .unwrap();
            }
            if state == "revoked" {
                ledger.revoke_review(&request.grant_id, 102).unwrap();
            }
            let before = fs::read(&ledger.path).unwrap();
            assert!(
                ledger
                    .resume_review(
                        &request.grant_id,
                        &request.messages[0],
                        &request.target,
                        if state == "expired" { 200 } else { 103 }
                    )
                    .is_err(),
                "{state}"
            );
            assert_eq!(fs::read(&ledger.path).unwrap(), before);
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn ordinary_request_cannot_publish_a_granted_id_after_restart_or_revocation() {
        let (root, ledger, request) = fixture("alpha");
        ledger.require_ordinary_request("ordinary").unwrap();
        ledger.authorize_review(request.clone(), 100).unwrap();
        let before = fs::read(&ledger.path).unwrap();
        assert_eq!(
            ledger
                .require_ordinary_request(&request.messages[0].request_id)
                .unwrap_err(),
            "WEB_REVIEW_SCOPED_WORKFLOW_REQUIRED"
        );
        assert_eq!(fs::read(&ledger.path).unwrap(), before);
        ledger.revoke_review(&request.grant_id, 101).unwrap();
        let reopened = WorkLedger::open(&root, "alpha").unwrap();
        assert!(
            reopened
                .require_ordinary_request(&request.messages[0].request_id)
                .is_err()
        );
        reopened.require_ordinary_request("ordinary").unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recorded_dispatch_remains_evidence_after_revocation_without_becoming_send_permission() {
        let (root, ledger, request) = fixture("alpha");
        ledger.authorize_review(request.clone(), 100).unwrap();
        let permit = ledger
            .reserve_review(
                &request.grant_id,
                &request.messages[0],
                &request.target,
                101,
            )
            .unwrap();
        ledger
            .prepare(
                &permit.request_id,
                &permit.event_id,
                &permit.message_sha256,
                &permit.target,
            )
            .unwrap();
        let record = ledger.record(&permit.request_id).unwrap().unwrap();
        let references = [format!("catdesk-review-v1:{}", request.grant_id)];
        assert!(
            !ledger
                .review_dispatch_recorded(&record, &references)
                .unwrap()
        );
        ledger.mark_review_dispatch_started(&permit, 102).unwrap();
        ledger.revoke_review(&request.grant_id, 250).unwrap();
        assert!(
            ledger
                .review_dispatch_recorded(&record, &references)
                .unwrap()
        );
        assert!(
            ledger
                .check_review(
                    &request.grant_id,
                    &request.messages[0],
                    &request.target,
                    251
                )
                .is_err()
        );
        assert!(!ledger.review_dispatch_recorded(&record, &[]).unwrap());
        assert!(
            !ledger
                .review_dispatch_recorded(&record, &[references[0].clone(), references[0].clone()])
                .unwrap()
        );
        assert!(
            !ledger
                .review_dispatch_recorded(&record, &["catdesk-review-v1:other".into()])
                .unwrap()
        );
        let mut forged = record.clone();
        forged.target = Target::new(2, "https://chatgpt.com/c/other".into()).unwrap();
        assert!(
            !ledger
                .review_dispatch_recorded(&forged, &references)
                .unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registration_lock_serializes_same_project_reservations_and_preserves_sibling() {
        use crate::delegated::autonomy_projects::AutonomousProjectRegistryStoreV1;
        use std::sync::{Arc, Barrier};
        use std::time::{Duration, Instant};

        let (root, ledger, request) = fixture("alpha");
        fs::create_dir(root.join(".catdesk/projects")).unwrap();
        ledger.authorize_review(request.clone(), 100).unwrap();
        let sibling = WorkLedger::open(&root, "beta").unwrap();
        sibling
            .prepare(
                "sibling-work",
                &expected_event("beta", "sibling-work"),
                &"f".repeat(64),
                &request.target,
            )
            .unwrap();
        let sibling_before = fs::read(&sibling.path).unwrap();
        let start = Arc::new(Barrier::new(2));
        let threads: Vec<_> = (0..2)
            .map(|_| {
                let root = root.clone();
                let request = request.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    let registry = AutonomousProjectRegistryStoreV1::open_read_only(
                        root.join(".catdesk/projects"),
                    )
                    .unwrap();
                    let ledger = WorkLedger::open(&root, "alpha").unwrap();
                    start.wait();
                    let deadline = Instant::now() + Duration::from_secs(2);
                    loop {
                        let result = registry.with_registration_lock(|| {
                            ledger.reserve_review(
                                &request.grant_id,
                                &request.messages[0],
                                &request.target,
                                101,
                            )
                        });
                        if result.as_ref().err().map(String::as_str)
                            == Some("project registration is already in progress")
                            && Instant::now() < deadline
                        {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        return result;
                    }
                })
            })
            .collect();
        let results: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| result.as_ref().err().map(String::as_str)
                    == Some("WEB_REVIEW_REQUEST_ALREADY_RESERVED"))
                .count(),
            1
        );
        assert_eq!(fs::read(&sibling.path).unwrap(), sibling_before);
        assert_eq!(
            ledger.load().unwrap().review_authorizations[0]
                .reservations
                .len(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }
}
