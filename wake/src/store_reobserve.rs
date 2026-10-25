//! Immutable operator-observation records in the existing private Wake log.
use super::*;
use serde_json::{Value, json};

pub(crate) struct ResponseReobservation {
    pub delivery: Delivery,
    pub timer: TurnTimer,
    journal_id: String,
}

impl Store {
    pub(crate) fn sent_response_scope(
        &self,
        project: &str,
        target: &Target,
        event: &str,
    ) -> Result<(Delivery, TurnTimer)> {
        target.validate()?;
        if !valid_id(project) || !valid_id(event) {
            return Err("RESPONSE_REOBSERVE_SCOPE_INVALID".into());
        }
        let delivery = self
            .read_delivery(event)?
            .ok_or("RESPONSE_REOBSERVE_SENT_REQUIRED")?;
        let timer = self
            .read_timer(event)?
            .ok_or("RESPONSE_REOBSERVE_TIMER_REQUIRED")?;
        delivery.validate()?;
        timer.validate()?;
        if !matches!(
            timer.response_state,
            ResponseState::Attention | ResponseState::Observing | ResponseState::Complete
        ) {
            return Err("RESPONSE_REOBSERVE_TIMER_STATE_REFUSED".into());
        }
        let receipt = delivery
            .receipt
            .as_ref()
            .ok_or("RESPONSE_PROBE_RECEIPT_REQUIRED")?;
        if delivery.phase != Phase::Sent
            || delivery.event.event_id != event
            || delivery.event.project_id != project
            || delivery.event.target_generation != target.generation
            || delivery.target != *target
            || self.read_config()?.targets.get(project) != Some(target)
            || timer.event_id != event
            || timer.project_id != project
            || timer.target_generation != target.generation
            || timer.target_digest != target.digest
            || receipt.event_id != event
            || receipt.target_generation != target.generation
            || receipt.target_digest != target.digest
            || receipt.message_digest != delivery.message_digest
            || delivery.message_digest != digest(&delivery.event.message)
            || receipt.evidence != "EXACT_USER_MESSAGE_APPENDED"
            || timer.started_utc != receipt.sent_utc
        {
            return Err("RESPONSE_REOBSERVE_SCOPE_INVALID".into());
        }
        Ok((delivery, timer))
    }

    pub(crate) fn begin_response_reobservation(
        &self,
        project: &str,
        target: &Target,
        event: &str,
    ) -> Result<ResponseReobservation> {
        let _lock = self.lock()?;
        let (delivery, timer) = self.sent_response_scope(project, target, event)?;
        let journal_id = format!(
            "response-reobserve-{}-{}-{}-{}",
            digest(event),
            now(),
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        );
        self.write_response_observation_log(&journal_id, "begin", &json!({
            "schemaVersion":1,"action":"OPERATOR_RESPONSE_REOBSERVATION_STARTED","observedUtc":now(),
            "delivery":delivery,"priorTimer":timer,"sendAttempted":false
        }))?;
        Ok(ResponseReobservation {
            delivery,
            timer,
            journal_id,
        })
    }

    pub(crate) fn finish_response_reobservation(
        &self,
        attempt: &ResponseReobservation,
        witness: &Value,
    ) -> Result<bool> {
        let _lock = self.lock()?;
        let (delivery, mut timer) = self.sent_response_scope(
            &attempt.delivery.event.project_id,
            &attempt.delivery.target,
            &attempt.delivery.event.event_id,
        )?;
        if delivery != attempt.delivery || timer != attempt.timer {
            return Err("RESPONSE_REOBSERVE_RECORD_CHANGED".into());
        }
        let complete = crate::runtime::validate_response_observation(&delivery, witness)?;
        // The witness is durable before the timer changes. It is not itself a
        // completed timer: recovery still reads the actual timer and receipt.
        self.write_response_observation_log(&attempt.journal_id, "outcome", &json!({
            "schemaVersion":1,"action":"OPERATOR_RESPONSE_REOBSERVATION_WITNESS","observedUtc":now(),
            "witness":witness,"priorTimer":attempt.timer,"sendAttempted":false
        }))?;
        if complete {
            timer.response_state = ResponseState::Complete;
            timer.completed_utc = Some(now().max(timer.started_utc));
            timer.validate()?;
            atomic(&self.timer_path(&timer.event_id), &timer)?;
        }
        Ok(complete)
    }

    pub(crate) fn fail_response_reobservation(
        &self,
        attempt: &ResponseReobservation,
        reason: &str,
    ) -> Result<()> {
        let reason = if valid_id(reason) {
            reason
        } else {
            "RESPONSE_REOBSERVE_FAILED"
        };
        self.write_response_observation_log(&attempt.journal_id, "outcome", &json!({
            "schemaVersion":1,"action":"OPERATOR_RESPONSE_REOBSERVATION_FAILED","observedUtc":now(),
            "reason":reason,"priorTimer":attempt.timer,"sendAttempted":false
        }))
    }

    fn write_response_observation_log(&self, id: &str, suffix: &str, value: &Value) -> Result<()> {
        if self.read_only || !valid_id(id) || !matches!(suffix, "begin" | "outcome") {
            return Err("RESPONSE_REOBSERVE_LOG_REFUSED".into());
        }
        let logs = self.root.join("logs");
        // Store::open already checks ancestry. Recheck these existing Store
        // directories, like other journal writers, without creating a path.
        reject_link(&self.root)?;
        reject_link(&logs)?;
        if !logs.is_dir() {
            return Err("RESPONSE_REOBSERVE_LOG_REFUSED".into());
        }
        let path = logs.join(format!("{id}-{suffix}.json"));
        reject_link(&path)?;
        let bytes = serde_json::to_vec_pretty(value).map_err(|_| "STATE_SERIALIZATION_FAILED")?;
        if bytes.len() as u64 > MAX_FILE {
            return Err("STATE_TOO_LARGE".into());
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| "RESPONSE_REOBSERVE_LOG_REFUSED")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "RESPONSE_REOBSERVE_LOG_REFUSED".into())
    }
}
