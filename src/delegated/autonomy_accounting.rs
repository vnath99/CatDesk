//! Durable, bounded execution accounting for autonomous work.
//!
//! This ledger deliberately records only provider evidence that CatDesk has
//! observed.  In particular, credit consumption is never inferred from token
//! estimates or UI price displays: it is reported only when two comparable
//! app-server snapshots explicitly provide a numeric credit balance.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::codex_app_server::CodexRoutingTelemetryV1;
use super::runtime::{NormalizedProviderEventKind, RuntimeError};

pub const EXECUTION_ACCOUNTING_SCHEMA_VERSION: u32 = 1;
const MAX_RECORDS: usize = 1_024;
const MAX_ACTIVITY_SPANS_PER_RECORD: usize = 256;
pub const ACTIVITY_SPAN_SCHEMA_VERSION: u32 = 1;

/// A bounded, source-attributed interval. Absence of a span deliberately does
/// not imply idle time or active time; reports retain that remainder as
/// unobserved/unknown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActivityActorV1 {
    CodexProviderActive,
    CatdeskVerificationReviewActive,
    CatdeskOrchestrationActive,
    ChatgptWebActive,
    Waiting,
    UnobservedIdleOrUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActivityPhaseV1 {
    ProviderTurn,
    VerificationReview,
    OrchestrationTransition,
    ChatgptGeneratingObservation,
    WaitingForCapacityOrOperator,
    UnobservedIdleOrUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActivityEvidenceV1 {
    ProviderLifecycle,
    VerifierLifecycle,
    ControllerTransition,
    ChatgptExactTargetGeneratingObservation,
    HistoricalUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActivitySpanStatusV1 {
    Open,
    Completed,
    Interrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySpanV1 {
    pub schema_version: u32,
    pub span_id: String,
    pub session_id: String,
    pub task_id: String,
    pub actor: ActivityActorV1,
    pub phase: ActivityPhaseV1,
    pub started_at_unix_millis: u128,
    pub ended_at_unix_millis: Option<u128>,
    pub evidence: ActivityEvidenceV1,
    pub status: ActivitySpanStatusV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkTimeEntryV1 {
    pub project_id: String,
    pub task_id: String,
    pub session_id: String,
    pub wall_millis: u128,
    pub total_known_active_millis: u128,
    pub provider_active_millis: u128,
    pub verification_review_active_millis: u128,
    pub orchestration_active_millis: u128,
    pub chatgpt_web_active_millis: Option<u128>,
    pub chatgpt_web_status: String,
    pub known_waiting_millis: u128,
    pub unobserved_idle_or_unknown_millis: u128,
    pub evidence_completeness: String,
    pub activity_provenance: Vec<ActivityEvidenceV1>,
    pub active_now: bool,
    pub current_actor: Option<ActivityActorV1>,
    pub current_state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkTimeReportV1 {
    pub as_of_unix_millis: u128,
    pub entries: Vec<WorkTimeEntryV1>,
    pub aggregate: WorkTimeAggregateV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkTimeAggregateV1 {
    pub wall_millis: u128,
    pub total_known_active_millis: u128,
    pub provider_active_millis: u128,
    pub verification_review_active_millis: u128,
    pub orchestration_active_millis: u128,
    pub observed_chatgpt_web_active_millis: u128,
    pub known_waiting_millis: u128,
    pub unobserved_idle_or_unknown_millis: u128,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionUsageSnapshotV1 {
    pub observed_at_unix_millis: u128,
    pub telemetry: CodexRoutingTelemetryV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionUsageDeltaV1 {
    /// `AVAILABLE` is limited to comparable rate-limit percentage windows.
    /// It never represents a guessed number of credits or tokens.
    pub status: String,
    pub reason: String,
    pub rate_limit_used_percent_deltas: Vec<ExecutionRateLimitDeltaV1>,
    /// `UNKNOWN_NOT_CAPTURED` is the safe default for historical sessions.
    pub credit_usage_status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRateLimitDeltaV1 {
    pub limit_name: String,
    pub before_used_percent: u8,
    pub after_used_percent: u8,
    pub used_percent_delta: i16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionAccountingRecordV1 {
    pub schema_version: u32,
    pub record_id: String,
    pub project_id: String,
    pub task_id: String,
    pub catdesk_session_id: String,
    pub provider_session_id: Option<String>,
    pub provider_id: String,
    pub effective_model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub started_at_unix_millis: u128,
    pub ended_at_unix_millis: Option<u128>,
    pub elapsed_millis: Option<u128>,
    pub provider_turns: u32,
    pub normalized_provider_events: u32,
    pub catdesk_tool_calls: u32,
    pub retries: u32,
    pub repair_cycles: u32,
    pub verification_started_at_unix_millis: Option<u128>,
    pub verification_ended_at_unix_millis: Option<u128>,
    pub verification_elapsed_millis: Option<u128>,
    pub verification_result: Option<String>,
    pub verification_reference: Option<String>,
    pub authoritative_diff_reference: Option<String>,
    pub final_review_reference: Option<String>,
    pub pre_codex_usage_snapshot: Option<ExecutionUsageSnapshotV1>,
    pub post_codex_usage_snapshot: Option<ExecutionUsageSnapshotV1>,
    pub usage_delta: ExecutionUsageDeltaV1,
    /// Added in T-0055. serde's default keeps historical v1 ledger records
    /// readable without manufacturing activity evidence.
    #[serde(default)]
    pub activity_spans: Vec<ActivitySpanV1>,
    /// Set permanently when bounded span storage drops an activity boundary.
    /// The marker makes the resulting under-observation explicit without
    /// interrupting normal controller execution or inventing lost duration.
    #[serde(default)]
    pub activity_evidence_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionAccountingLedgerV1 {
    pub schema_version: u32,
    pub records: Vec<ExecutionAccountingRecordV1>,
}

#[derive(Clone, Debug)]
pub struct ExecutionAccountingStoreV1 {
    root: PathBuf,
}

impl ExecutionAccountingStoreV1 {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, RuntimeError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(io_error)?;
        Ok(Self { root })
    }

    pub fn list(
        &self,
        session_id: Option<&str>,
    ) -> Result<Vec<ExecutionAccountingRecordV1>, RuntimeError> {
        let ledger = self.load()?;
        Ok(ledger
            .records
            .into_iter()
            .filter(|record| {
                session_id.is_none_or(|session_id| record.catdesk_session_id == session_id)
            })
            .collect())
    }

    pub fn record_task_started(
        &self,
        project_id: &str,
        task_id: &str,
        session_id: &str,
        provider_session_id: Option<&str>,
        provider_id: &str,
        effective_model: Option<&str>,
        telemetry: Option<CodexRoutingTelemetryV1>,
        now_millis: u128,
    ) -> Result<(), RuntimeError> {
        let mut ledger = self.load()?;
        let record_id = record_id(session_id, task_id);
        let project_id = bounded(project_id, 128);
        if let Some(record) = ledger
            .records
            .iter_mut()
            .find(|record| record.record_id == record_id)
        {
            if record.project_id != project_id {
                return Err(RuntimeError::Validation(
                    "execution accounting record is bound to a different project".into(),
                ));
            }
            record.provider_turns = record.provider_turns.saturating_add(1);
            record.provider_session_id = provider_session_id
                .map(str::to_owned)
                .or(record.provider_session_id.clone());
            record.provider_id = bounded(provider_id, 128);
            record.effective_model = effective_model.map(|value| bounded(value, 256));
            // A retry is a new provider attempt for this logical task, not a
            // post-turn observation.  Only the host-owned post-turn path may
            // fill the second snapshot; otherwise retry preflight telemetry
            // would be misrepresented as measured usage.
            if record.pre_codex_usage_snapshot.is_none() {
                if let Some(telemetry) = telemetry {
                    record_observability(record, telemetry, now_millis);
                }
            }
            return self.save(&ledger);
        }
        if ledger.records.len() >= MAX_RECORDS {
            return Err(RuntimeError::Validation(
                "execution accounting record limit reached".into(),
            ));
        }
        let mut record = ExecutionAccountingRecordV1 {
            schema_version: EXECUTION_ACCOUNTING_SCHEMA_VERSION,
            record_id,
            project_id,
            task_id: bounded(task_id, 128),
            catdesk_session_id: bounded(session_id, 128),
            provider_session_id: provider_session_id.map(|value| bounded(value, 256)),
            provider_id: bounded(provider_id, 128),
            effective_model: effective_model.map(|value| bounded(value, 256)),
            reasoning_effort: None,
            started_at_unix_millis: now_millis,
            ended_at_unix_millis: None,
            elapsed_millis: None,
            provider_turns: 1,
            normalized_provider_events: 0,
            catdesk_tool_calls: 0,
            retries: 0,
            repair_cycles: 0,
            verification_started_at_unix_millis: None,
            verification_ended_at_unix_millis: None,
            verification_elapsed_millis: None,
            verification_result: None,
            verification_reference: None,
            authoritative_diff_reference: None,
            final_review_reference: None,
            pre_codex_usage_snapshot: None,
            post_codex_usage_snapshot: None,
            usage_delta: unknown_usage(
                "authoritative comparable Codex pre/post snapshots were not captured",
            ),
            activity_spans: Vec::new(),
            activity_evidence_truncated: false,
        };
        if let Some(telemetry) = telemetry {
            record_observability(&mut record, telemetry, now_millis);
        }
        ledger.records.push(record);
        self.save(&ledger)
    }

    pub fn record_observability(
        &self,
        session_id: &str,
        telemetry: CodexRoutingTelemetryV1,
        now_millis: u128,
    ) -> Result<(), RuntimeError> {
        let mut ledger = self.load()?;
        if let Some(record) = ledger.records.iter_mut().rev().find(|record| {
            record.catdesk_session_id == session_id && record.ended_at_unix_millis.is_none()
        }) {
            // General app-server notifications are routing evidence, not a
            // provider-turn completion signal.  They may establish a missing
            // pre-turn baseline, but only the explicit host post-turn method
            // below is allowed to create a post-turn snapshot.
            if record.pre_codex_usage_snapshot.is_none() {
                record_observability(record, telemetry.clone(), now_millis);
            }
        } else if let Some(record) = ledger
            .records
            .iter_mut()
            .rev()
            .find(|record| record.catdesk_session_id == session_id)
        {
            if record.pre_codex_usage_snapshot.is_none() {
                record_observability(record, telemetry.clone(), now_millis);
            }
        }
        self.save(&ledger)
    }

    /// Records the one authoritative post-turn Codex observation for the
    /// latest logical task in a session.  It deliberately refuses to infer a
    /// post snapshot from a retry, notification, or replay.  Replays after a
    /// durable post observation are idempotent and retain the original
    /// evidence rather than replacing it with a later unrelated account read.
    pub fn record_post_codex_observability(
        &self,
        session_id: &str,
        telemetry: CodexRoutingTelemetryV1,
        now_millis: u128,
    ) -> Result<bool, RuntimeError> {
        let mut ledger = self.load()?;
        let Some(record) = ledger
            .records
            .iter_mut()
            .rev()
            .find(|record| record.catdesk_session_id == session_id)
        else {
            return Ok(false);
        };
        if record.provider_id != "codex-cli"
            || record.pre_codex_usage_snapshot.is_none()
            || record.post_codex_usage_snapshot.is_some()
        {
            return Ok(false);
        }
        record_observability(record, telemetry, now_millis);
        self.save(&ledger)?;
        Ok(true)
    }

    /// Returns whether exactly one post-turn Codex observation remains due.
    /// This is intentionally narrower than general routing telemetry: it
    /// prevents a restart/replay from reopening the app-server or replacing
    /// the durable post-turn evidence after that evidence has been captured.
    pub fn needs_post_codex_observability(&self, session_id: &str) -> Result<bool, RuntimeError> {
        let ledger = self.load()?;
        Ok(ledger
            .records
            .iter()
            .rev()
            .find(|record| record.catdesk_session_id == session_id)
            .is_some_and(|record| {
                record.provider_id == "codex-cli"
                    && record.pre_codex_usage_snapshot.is_some()
                    && record.post_codex_usage_snapshot.is_none()
            }))
    }

    /// Opens a span only at a durable source boundary. Starting an actor also
    /// closes waiting, so provider retry/backoff cannot be counted active.
    pub fn activity_started(
        &self,
        session_id: &str,
        actor: ActivityActorV1,
        evidence: ActivityEvidenceV1,
        now_millis: u128,
    ) -> Result<(), RuntimeError> {
        self.update_active(session_id, |record| {
            if matches!(
                &actor,
                ActivityActorV1::CodexProviderActive
                    | ActivityActorV1::CatdeskVerificationReviewActive
            ) {
                close_open_actor(
                    record,
                    &ActivityActorV1::Waiting,
                    now_millis,
                    ActivitySpanStatusV1::Completed,
                );
            }
            close_open_actor(
                record,
                &actor,
                now_millis,
                ActivitySpanStatusV1::Interrupted,
            );
            if record.activity_spans.len() < MAX_ACTIVITY_SPANS_PER_RECORD {
                record.activity_spans.push(ActivitySpanV1 {
                    schema_version: ACTIVITY_SPAN_SCHEMA_VERSION,
                    span_id: Uuid::new_v4().to_string(),
                    session_id: record.catdesk_session_id.clone(),
                    task_id: record.task_id.clone(),
                    phase: phase_for_actor(&actor),
                    actor,
                    started_at_unix_millis: now_millis,
                    ended_at_unix_millis: None,
                    evidence,
                    status: ActivitySpanStatusV1::Open,
                });
            } else {
                // Do not fail the controller lifecycle merely because the
                // bounded ledger cannot retain another boundary. Persisting
                // this marker makes every future report fail closed on
                // evidence completeness instead of silently undercounting.
                record.activity_evidence_truncated = true;
            }
        })
    }

    pub fn activity_finished(
        &self,
        session_id: &str,
        actor: ActivityActorV1,
        now_millis: u128,
    ) -> Result<(), RuntimeError> {
        self.update_active(session_id, |record| {
            close_open_actor(record, &actor, now_millis, ActivitySpanStatusV1::Completed)
        })
    }

    /// Waiting is explicit only when CatDesk has transitioned into a known
    /// no-worker state. It never fills arbitrary gaps in historical records.
    pub fn waiting_started(&self, session_id: &str, now_millis: u128) -> Result<(), RuntimeError> {
        self.activity_started(
            session_id,
            ActivityActorV1::Waiting,
            ActivityEvidenceV1::ControllerTransition,
            now_millis,
        )
    }

    /// Crash/reload reconciliation never extends open spans to recovery time.
    /// They are closed at their last authoritative start observation instead.
    pub fn interrupt_open_spans(&self, session_id: &str) -> Result<(), RuntimeError> {
        self.update_active(session_id, |record| {
            for span in &mut record.activity_spans {
                if span.status == ActivitySpanStatusV1::Open {
                    span.ended_at_unix_millis = Some(span.started_at_unix_millis);
                    span.status = ActivitySpanStatusV1::Interrupted;
                }
            }
        })
    }

    pub fn work_time_report(
        &self,
        start_unix_millis: u128,
        end_unix_millis: u128,
        project_id: Option<&str>,
        session_id: Option<&str>,
        task_id: Option<&str>,
        limit: usize,
        as_of_unix_millis: u128,
    ) -> Result<WorkTimeReportV1, RuntimeError> {
        if start_unix_millis == 0
            || end_unix_millis <= start_unix_millis
            || end_unix_millis.saturating_sub(start_unix_millis) > 31 * 24 * 60 * 60 * 1_000
            || limit == 0
            || limit > 100
        {
            return Err(RuntimeError::Validation(
                "work time report window is invalid".into(),
            ));
        }
        let mut records = self
            .load()?
            .records
            .into_iter()
            .filter(|record| project_id.is_none_or(|value| record.project_id == value))
            .filter(|record| session_id.is_none_or(|value| record.catdesk_session_id == value))
            .filter(|record| task_id.is_none_or(|value| record.task_id == value))
            .filter(|record| {
                record.started_at_unix_millis < end_unix_millis
                    && record_authoritative_end(record) > start_unix_millis
            })
            .collect::<Vec<_>>();
        records.sort_by(|left, right| {
            left.catdesk_session_id
                .cmp(&right.catdesk_session_id)
                .then(left.task_id.cmp(&right.task_id))
        });
        records.truncate(limit);
        let entries = records
            .iter()
            .filter_map(|record| work_time_entry(record, start_unix_millis, end_unix_millis))
            .collect::<Vec<_>>();
        let aggregate = aggregate_work_time(&records, start_unix_millis, end_unix_millis);
        Ok(WorkTimeReportV1 {
            as_of_unix_millis,
            entries,
            aggregate,
        })
    }

    pub fn record_events(
        &self,
        session_id: &str,
        normalized_events: u32,
        tool_calls: u32,
    ) -> Result<(), RuntimeError> {
        self.update_active(session_id, |record| {
            record.normalized_provider_events = record
                .normalized_provider_events
                .saturating_add(normalized_events);
            record.catdesk_tool_calls = record.catdesk_tool_calls.saturating_add(tool_calls);
        })
    }
    pub fn record_retry(&self, session_id: &str) -> Result<(), RuntimeError> {
        self.update_active(session_id, |record| {
            record.retries = record.retries.saturating_add(1)
        })
    }
    pub fn record_repair(&self, session_id: &str) -> Result<(), RuntimeError> {
        self.update_active(session_id, |record| {
            record.repair_cycles = record.repair_cycles.saturating_add(1)
        })
    }
    pub fn record_verification_started(
        &self,
        session_id: &str,
        now_millis: u128,
    ) -> Result<(), RuntimeError> {
        self.update_active(session_id, |record| {
            record.verification_started_at_unix_millis = Some(now_millis)
        })
    }
    pub fn record_verification_finished(
        &self,
        session_id: &str,
        result: &str,
        reference: &str,
        now_millis: u128,
    ) -> Result<(), RuntimeError> {
        self.update_active(session_id, |record| {
            record.verification_ended_at_unix_millis = Some(now_millis);
            record.verification_elapsed_millis = record
                .verification_started_at_unix_millis
                .map(|started| now_millis.saturating_sub(started));
            record.verification_result = Some(bounded(result, 128));
            record.verification_reference = Some(bounded(reference, 2_048));
        })
    }
    pub fn finish_task(
        &self,
        session_id: &str,
        diff_reference: Option<&str>,
        review_reference: Option<&str>,
        now_millis: u128,
    ) -> Result<(), RuntimeError> {
        self.update_active(session_id, |record| {
            record.ended_at_unix_millis = Some(now_millis);
            record.elapsed_millis = Some(now_millis.saturating_sub(record.started_at_unix_millis));
            record.authoritative_diff_reference = diff_reference.map(|value| bounded(value, 2_048));
            record.final_review_reference = review_reference.map(|value| bounded(value, 2_048));
            finalize_usage_delta(record);
        })
    }

    pub fn backfill_t0031(&self) -> Result<(), RuntimeError> {
        let mut ledger = self.load()?;
        if ledger
            .records
            .iter()
            .any(|record| record.record_id == "t0031-historical")
        {
            return Ok(());
        }
        ledger.records.push(ExecutionAccountingRecordV1 {
            schema_version: EXECUTION_ACCOUNTING_SCHEMA_VERSION, record_id: "t0031-historical".into(), project_id: "catdesk".into(), task_id: "t0031-milestones1-8".into(), catdesk_session_id: "adc-t0031-milestones1-8-20260808".into(), provider_session_id: None, provider_id: "codex-cli".into(), effective_model: None, reasoning_effort: None,
            started_at_unix_millis: 0, ended_at_unix_millis: None, elapsed_millis: Some(630_023), provider_turns: 1, normalized_provider_events: 91, catdesk_tool_calls: 0, retries: 0, repair_cycles: 0,
            verification_started_at_unix_millis: None, verification_ended_at_unix_millis: None, verification_elapsed_millis: None, verification_result: Some("PASSED".into()), verification_reference: Some("docs/orchestrator/review_bundles/T-0031_MILESTONES_1_8_REVIEW_BUNDLE.md".into()), authoritative_diff_reference: Some("authoritative-diff: clean".into()), final_review_reference: Some("CatDesk independent final review".into()), pre_codex_usage_snapshot: None, post_codex_usage_snapshot: None, usage_delta: unknown_usage("historical T-0031 diagnostics did not capture authoritative Codex pre/post usage snapshots"),
            activity_spans: Vec::new(),
            activity_evidence_truncated: false,
        });
        self.save(&ledger)
    }

    fn update_active<F>(&self, session_id: &str, update: F) -> Result<(), RuntimeError>
    where
        F: FnOnce(&mut ExecutionAccountingRecordV1),
    {
        let mut ledger = self.load()?;
        if let Some(record) = ledger.records.iter_mut().rev().find(|record| {
            record.catdesk_session_id == session_id && record.ended_at_unix_millis.is_none()
        }) {
            update(record);
        }
        self.save(&ledger)
    }
    fn path(&self) -> PathBuf {
        self.root.join("execution-accounting.json")
    }
    fn load(&self) -> Result<ExecutionAccountingLedgerV1, RuntimeError> {
        if !self.path().exists() {
            return Ok(ExecutionAccountingLedgerV1 {
                schema_version: EXECUTION_ACCOUNTING_SCHEMA_VERSION,
                records: Vec::new(),
            });
        }
        let file = OpenOptions::new()
            .read(true)
            .open(self.path())
            .map_err(io_error)?;
        let ledger: ExecutionAccountingLedgerV1 =
            serde_json::from_reader(file).map_err(|error| {
                RuntimeError::Validation(format!(
                    "execution accounting ledger is corrupted: {error}"
                ))
            })?;
        validate(&ledger)?;
        Ok(ledger)
    }
    fn save(&self, ledger: &ExecutionAccountingLedgerV1) -> Result<(), RuntimeError> {
        validate(ledger)?;
        let temporary = self
            .root
            .join(format!(".execution-accounting-{}.tmp", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(io_error)?;
        serde_json::to_writer_pretty(&mut file, ledger).map_err(|_| {
            RuntimeError::Validation("execution accounting serialization failed".into())
        })?;
        file.write_all(b"\n").map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        fs::rename(temporary, self.path()).map_err(io_error)
    }
}

fn close_open_actor(
    record: &mut ExecutionAccountingRecordV1,
    actor: &ActivityActorV1,
    now_millis: u128,
    status: ActivitySpanStatusV1,
) {
    for span in record.activity_spans.iter_mut().rev() {
        if span.status == ActivitySpanStatusV1::Open && &span.actor == actor {
            span.ended_at_unix_millis = Some(now_millis.max(span.started_at_unix_millis));
            span.status = status;
            break;
        }
    }
}

fn phase_for_actor(actor: &ActivityActorV1) -> ActivityPhaseV1 {
    match actor {
        ActivityActorV1::CodexProviderActive => ActivityPhaseV1::ProviderTurn,
        ActivityActorV1::CatdeskVerificationReviewActive => ActivityPhaseV1::VerificationReview,
        ActivityActorV1::CatdeskOrchestrationActive => ActivityPhaseV1::OrchestrationTransition,
        ActivityActorV1::ChatgptWebActive => ActivityPhaseV1::ChatgptGeneratingObservation,
        ActivityActorV1::Waiting => ActivityPhaseV1::WaitingForCapacityOrOperator,
        ActivityActorV1::UnobservedIdleOrUnknown => ActivityPhaseV1::UnobservedIdleOrUnknown,
    }
}

fn clipped_interval(
    start: u128,
    end: u128,
    window_start: u128,
    window_end: u128,
) -> Option<(u128, u128)> {
    let start = start.max(window_start);
    let end = end.min(window_end);
    (end > start).then_some((start, end))
}

fn union_millis(mut intervals: Vec<(u128, u128)>) -> u128 {
    intervals.sort_unstable();
    let mut total = 0u128;
    let mut current: Option<(u128, u128)> = None;
    for (start, end) in intervals {
        match current {
            Some((current_start, current_end)) if start <= current_end => {
                current = Some((current_start, current_end.max(end)));
            }
            Some((current_start, current_end)) => {
                total = total.saturating_add(current_end.saturating_sub(current_start));
                current = Some((start, end));
            }
            None => current = Some((start, end)),
        }
    }
    current.map_or(total, |(start, end)| {
        total.saturating_add(end.saturating_sub(start))
    })
}

fn work_time_entry(
    record: &ExecutionAccountingRecordV1,
    window_start: u128,
    window_end: u128,
) -> Option<WorkTimeEntryV1> {
    let record_end = record_authoritative_end(record);
    let (wall_start, wall_end) = clipped_interval(
        record.started_at_unix_millis,
        record_end,
        window_start,
        window_end,
    )?;
    let mut provider = Vec::new();
    let mut verification = Vec::new();
    let mut orchestration = Vec::new();
    let mut chatgpt = Vec::new();
    let mut waiting = Vec::new();
    let mut incomplete = record.activity_spans.is_empty() || record.activity_evidence_truncated;
    for span in &record.activity_spans {
        if span.schema_version != ACTIVITY_SPAN_SCHEMA_VERSION {
            incomplete = true;
            continue;
        }
        if span.status == ActivitySpanStatusV1::Interrupted {
            // Recovery does not know whether work continued while CatDesk was
            // unavailable. The zero-duration interval remains non-active,
            // but the record can no longer claim complete evidence.
            incomplete = true;
        }
        let Some(end) = span.ended_at_unix_millis else {
            // An open span has no current process-proof in this read-only
            // ledger query, so it is deliberately not extended to as-of.
            incomplete = true;
            continue;
        };
        let Some(interval) =
            clipped_interval(span.started_at_unix_millis, end, wall_start, wall_end)
        else {
            continue;
        };
        match span.actor {
            ActivityActorV1::CodexProviderActive => provider.push(interval),
            ActivityActorV1::CatdeskVerificationReviewActive => verification.push(interval),
            ActivityActorV1::CatdeskOrchestrationActive => orchestration.push(interval),
            ActivityActorV1::ChatgptWebActive => chatgpt.push(interval),
            ActivityActorV1::Waiting => waiting.push(interval),
            ActivityActorV1::UnobservedIdleOrUnknown => incomplete = true,
        }
    }
    let provider_active_millis = union_millis(provider.clone());
    let verification_review_active_millis = union_millis(verification.clone());
    let orchestration_active_millis = union_millis(orchestration.clone());
    let observed_chatgpt_web_active_millis = union_millis(chatgpt.clone());
    let known_waiting_millis = union_millis(waiting.clone());
    let total_known_active_millis = union_millis(
        [provider, verification, orchestration, chatgpt]
            .into_iter()
            .flatten()
            .collect(),
    );
    let wall_millis = wall_end.saturating_sub(wall_start);
    let covered = union_millis(
        [activity_intervals(record, wall_start, wall_end), waiting]
            .into_iter()
            .flatten()
            .collect(),
    );
    let chatgpt_observed = record
        .activity_spans
        .iter()
        .any(|span| span.actor == ActivityActorV1::ChatgptWebActive);
    let mut activity_provenance = Vec::new();
    for span in &record.activity_spans {
        if !activity_provenance.contains(&span.evidence) {
            activity_provenance.push(span.evidence.clone());
        }
    }
    Some(WorkTimeEntryV1 {
        project_id: record.project_id.clone(),
        task_id: record.task_id.clone(),
        session_id: record.catdesk_session_id.clone(),
        wall_millis,
        total_known_active_millis,
        provider_active_millis,
        verification_review_active_millis,
        orchestration_active_millis,
        chatgpt_web_active_millis: chatgpt_observed.then_some(observed_chatgpt_web_active_millis),
        chatgpt_web_status: if chatgpt_observed {
            "OBSERVED_ONLY"
        } else {
            "UNKNOWN_NOT_OBSERVABLE"
        }
        .into(),
        known_waiting_millis,
        unobserved_idle_or_unknown_millis: wall_millis.saturating_sub(covered),
        evidence_completeness: if incomplete {
            "INCOMPLETE_EVIDENCE"
        } else {
            "OBSERVED_SPANS"
        }
        .into(),
        activity_provenance,
        active_now: false,
        current_actor: None,
        // The durable ledger alone cannot prove a process is still active.
        // The MCP facade replaces this with the current persisted session state
        // when it is available, without promoting an open span to active time.
        current_state: "UNKNOWN_NOT_LOADED".into(),
    })
}

fn activity_intervals(
    record: &ExecutionAccountingRecordV1,
    window_start: u128,
    window_end: u128,
) -> Vec<(u128, u128)> {
    record
        .activity_spans
        .iter()
        .filter_map(|span| {
            matches!(
                span.actor,
                ActivityActorV1::CodexProviderActive
                    | ActivityActorV1::CatdeskVerificationReviewActive
                    | ActivityActorV1::CatdeskOrchestrationActive
                    | ActivityActorV1::ChatgptWebActive
            )
            .then_some(span.ended_at_unix_millis)
            .flatten()
            .and_then(|end| {
                clipped_interval(span.started_at_unix_millis, end, window_start, window_end)
            })
        })
        .collect()
}

fn record_authoritative_end(record: &ExecutionAccountingRecordV1) -> u128 {
    record.ended_at_unix_millis.unwrap_or_else(|| {
        record
            .activity_spans
            .iter()
            .map(|span| {
                span.ended_at_unix_millis
                    .unwrap_or(span.started_at_unix_millis)
            })
            .chain(record.verification_ended_at_unix_millis)
            .max()
            .unwrap_or(record.started_at_unix_millis)
    })
}

fn aggregate_work_time(
    records: &[ExecutionAccountingRecordV1],
    window_start: u128,
    window_end: u128,
) -> WorkTimeAggregateV1 {
    let mut wall = Vec::new();
    let mut provider = Vec::new();
    let mut verification = Vec::new();
    let mut orchestration = Vec::new();
    let mut chatgpt = Vec::new();
    let mut waiting = Vec::new();
    for record in records {
        let end = record_authoritative_end(record);
        let Some((record_start, record_end)) =
            clipped_interval(record.started_at_unix_millis, end, window_start, window_end)
        else {
            continue;
        };
        wall.push((record_start, record_end));
        for span in &record.activity_spans {
            let Some(span_end) = span.ended_at_unix_millis else {
                continue;
            };
            let Some(interval) = clipped_interval(
                span.started_at_unix_millis,
                span_end,
                record_start,
                record_end,
            ) else {
                continue;
            };
            match span.actor {
                ActivityActorV1::CodexProviderActive => provider.push(interval),
                ActivityActorV1::CatdeskVerificationReviewActive => verification.push(interval),
                ActivityActorV1::CatdeskOrchestrationActive => orchestration.push(interval),
                ActivityActorV1::ChatgptWebActive => chatgpt.push(interval),
                ActivityActorV1::Waiting => waiting.push(interval),
                ActivityActorV1::UnobservedIdleOrUnknown => {}
            }
        }
    }
    let wall_millis = union_millis(wall);
    let total_known_active_millis = union_millis(
        [
            provider.clone(),
            verification.clone(),
            orchestration.clone(),
            chatgpt.clone(),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );
    let known_waiting_millis = union_millis(waiting.clone());
    WorkTimeAggregateV1 {
        wall_millis,
        total_known_active_millis,
        provider_active_millis: union_millis(provider),
        verification_review_active_millis: union_millis(verification),
        orchestration_active_millis: union_millis(orchestration),
        observed_chatgpt_web_active_millis: union_millis(chatgpt),
        known_waiting_millis,
        unobserved_idle_or_unknown_millis: wall_millis.saturating_sub(union_millis(
            [
                activity_intervals_for_records(records, window_start, window_end),
                waiting,
            ]
            .into_iter()
            .flatten()
            .collect(),
        )),
    }
}

fn activity_intervals_for_records(
    records: &[ExecutionAccountingRecordV1],
    window_start: u128,
    window_end: u128,
) -> Vec<(u128, u128)> {
    records
        .iter()
        .flat_map(|record| activity_intervals(record, window_start, window_end))
        .collect()
}

fn record_observability(
    record: &mut ExecutionAccountingRecordV1,
    telemetry: CodexRoutingTelemetryV1,
    now_millis: u128,
) {
    record.effective_model = telemetry
        .selected_model
        .as_deref()
        .map(|value| bounded(value, 256))
        .or(record.effective_model.clone());
    record.reasoning_effort = telemetry
        .reasoning_effort
        .as_deref()
        .map(|value| bounded(value, 128))
        .or(record.reasoning_effort.clone());
    let snapshot = ExecutionUsageSnapshotV1 {
        observed_at_unix_millis: now_millis,
        telemetry,
    };
    if record.pre_codex_usage_snapshot.is_none() {
        record.pre_codex_usage_snapshot = Some(snapshot);
    } else if record.post_codex_usage_snapshot.is_none() {
        record.post_codex_usage_snapshot = Some(snapshot);
        finalize_usage_delta(record);
    }
}
fn finalize_usage_delta(record: &mut ExecutionAccountingRecordV1) {
    let (Some(before), Some(after)) = (
        &record.pre_codex_usage_snapshot,
        &record.post_codex_usage_snapshot,
    ) else {
        return;
    };
    if after.telemetry.observed_at_unix <= before.telemetry.observed_at_unix {
        record.usage_delta = unknown_usage(
            "Codex post-turn usage snapshot was not newer than the authoritative pre-turn snapshot",
        );
        return;
    }
    let mut deltas = Vec::new();
    for before_window in &before.telemetry.rate_limit_windows {
        if let (Some(before_used), Some(after_window)) = (
            before_window.used_percent,
            after
                .telemetry
                .rate_limit_windows
                .iter()
                .find(|after_window| {
                    after_window.limit_name == before_window.limit_name
                        && after_window.resets_at_unix == before_window.resets_at_unix
                }),
        ) && let Some(after_used) = after_window.used_percent
        {
            deltas.push(ExecutionRateLimitDeltaV1 {
                limit_name: before_window.limit_name.clone(),
                before_used_percent: before_used,
                after_used_percent: after_used,
                used_percent_delta: i16::from(after_used) - i16::from(before_used),
            });
        }
    }
    record.usage_delta = if deltas.is_empty() {
        unknown_usage(
            "Codex usage snapshots were present but no comparable rate-limit window was available",
        )
    } else {
        ExecutionUsageDeltaV1 {
            status: "AVAILABLE".into(),
            reason: "delta is limited to matching authoritative rate-limit windows".into(),
            rate_limit_used_percent_deltas: deltas,
            credit_usage_status: "UNKNOWN_NOT_CAPTURED".into(),
        }
    };
}
fn unknown_usage(reason: &str) -> ExecutionUsageDeltaV1 {
    ExecutionUsageDeltaV1 {
        status: "UNKNOWN_NOT_CAPTURED".into(),
        reason: bounded(reason, 512),
        rate_limit_used_percent_deltas: Vec::new(),
        credit_usage_status: "UNKNOWN_NOT_CAPTURED".into(),
    }
}
fn record_id(session_id: &str, task_id: &str) -> String {
    format!("{}-{}", bounded(session_id, 128), bounded(task_id, 128))
}
fn bounded(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}
fn validate(ledger: &ExecutionAccountingLedgerV1) -> Result<(), RuntimeError> {
    if ledger.schema_version != EXECUTION_ACCOUNTING_SCHEMA_VERSION
        || ledger.records.len() > MAX_RECORDS
        || ledger.records.iter().any(|record| {
            record.schema_version != EXECUTION_ACCOUNTING_SCHEMA_VERSION
                || record.record_id.is_empty()
                || record.record_id.len() > 256
                || record.project_id.is_empty()
                || record.task_id.is_empty()
                || record.catdesk_session_id.is_empty()
                || record.activity_spans.len() > MAX_ACTIVITY_SPANS_PER_RECORD
                || record.activity_spans.iter().any(|span| {
                    span.schema_version != ACTIVITY_SPAN_SCHEMA_VERSION
                        || span.span_id.is_empty()
                        || span.span_id.len() > 64
                        || span.session_id != record.catdesk_session_id
                        || span.task_id != record.task_id
                        || span
                            .ended_at_unix_millis
                            .is_some_and(|end| end < span.started_at_unix_millis)
                })
        })
    {
        return Err(RuntimeError::Validation(
            "execution accounting ledger is invalid".into(),
        ));
    }
    Ok(())
}
fn io_error(error: std::io::Error) -> RuntimeError {
    RuntimeError::Provider(format!("execution accounting I/O error: {error}"))
}

pub fn normalized_tool_call_count(events: &[super::runtime::NormalizedProviderEventV1]) -> u32 {
    events
        .iter()
        .filter(|event| event.kind == NormalizedProviderEventKind::ToolCall)
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backfill_never_guesses_t0031_credit_consumption() {
        let root = std::env::temp_dir().join(format!("catdesk-accounting-{}", Uuid::new_v4()));
        let store = ExecutionAccountingStoreV1::open(&root).expect("store");
        store.backfill_t0031().expect("backfill");
        let record = store
            .list(Some("adc-t0031-milestones1-8-20260808"))
            .expect("list")
            .pop()
            .expect("record");
        assert_eq!(record.elapsed_millis, Some(630_023));
        assert_eq!(
            record.usage_delta.credit_usage_status,
            "UNKNOWN_NOT_CAPTURED"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_read_only_report_does_not_create_a_ledger() {
        let root = std::env::temp_dir().join(format!("catdesk-accounting-{}", Uuid::new_v4()));
        let store = ExecutionAccountingStoreV1::open(&root).expect("store");
        assert!(store.list(None).expect("list").is_empty());
        assert!(!root.join("execution-accounting.json").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn post_turn_host_snapshot_is_retained_after_task_finishes() {
        let root = std::env::temp_dir().join(format!("catdesk-accounting-{}", Uuid::new_v4()));
        let store = ExecutionAccountingStoreV1::open(&root).expect("store");
        let before = CodexRoutingTelemetryV1 {
            observed_at_unix: 1,
            source: "codex-app-server/account-rateLimits-read".into(),
            rate_limit_windows: vec![super::super::codex_app_server::CodexRateLimitWindowV1 {
                limit_name: "weekly".into(),
                used_percent: Some(10),
                resets_at_unix: Some(100),
                reached_limit: false,
            }],
            ..Default::default()
        };
        store
            .record_task_started(
                "catdesk",
                "task",
                "session",
                Some("thread"),
                "codex-cli",
                Some("gpt-5.6-terra"),
                Some(before),
                1_000,
            )
            .expect("start");
        store
            .finish_task("session", None, None, 2_000)
            .expect("finish");
        let after = CodexRoutingTelemetryV1 {
            observed_at_unix: 2,
            source: "codex-app-server/account-rateLimits-read".into(),
            rate_limit_windows: vec![super::super::codex_app_server::CodexRateLimitWindowV1 {
                limit_name: "weekly".into(),
                used_percent: Some(25),
                resets_at_unix: Some(100),
                reached_limit: false,
            }],
            ..Default::default()
        };
        store
            .record_post_codex_observability("session", after, 3_000)
            .expect("post snapshot");
        let record = store
            .list(Some("session"))
            .expect("list")
            .pop()
            .expect("record");
        assert!(record.post_codex_usage_snapshot.is_some());
        assert_eq!(
            record.usage_delta.rate_limit_used_percent_deltas[0].used_percent_delta,
            15
        );
        assert_eq!(
            record.usage_delta.credit_usage_status,
            "UNKNOWN_NOT_CAPTURED"
        );
        let _ = fs::remove_dir_all(root);
    }

    fn rate_limit_telemetry(
        observed_at_unix: u64,
        used_percent: u8,
        resets_at_unix: u64,
    ) -> CodexRoutingTelemetryV1 {
        CodexRoutingTelemetryV1 {
            observed_at_unix,
            source: "codex-app-server/account-rateLimits-read".into(),
            rate_limit_windows: vec![super::super::codex_app_server::CodexRateLimitWindowV1 {
                limit_name: "weekly".into(),
                used_percent: Some(used_percent),
                resets_at_unix: Some(resets_at_unix),
                reached_limit: false,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn retry_and_replay_cannot_replace_the_one_post_turn_snapshot() {
        let root = std::env::temp_dir().join(format!("catdesk-accounting-{}", Uuid::new_v4()));
        let store = ExecutionAccountingStoreV1::open(&root).expect("store");
        store
            .record_task_started(
                "catdesk",
                "task",
                "session",
                Some("thread"),
                "codex-cli",
                Some("gpt-5.6-terra"),
                Some(rate_limit_telemetry(10, 10, 100)),
                10_000,
            )
            .expect("first attempt");
        store
            .record_task_started(
                "catdesk",
                "task",
                "session",
                Some("thread"),
                "codex-cli",
                Some("gpt-5.6-terra"),
                Some(rate_limit_telemetry(11, 30, 100)),
                11_000,
            )
            .expect("retry");
        store
            .record_observability("session", rate_limit_telemetry(11, 30, 100), 11_500)
            .expect("routing notification");
        assert!(
            store
                .needs_post_codex_observability("session")
                .expect("pending")
        );
        assert!(
            store
                .record_post_codex_observability(
                    "session",
                    rate_limit_telemetry(12, 25, 100),
                    12_000
                )
                .expect("post")
        );
        assert!(
            !store
                .record_post_codex_observability(
                    "session",
                    rate_limit_telemetry(13, 40, 100),
                    13_000
                )
                .expect("replay")
        );
        let record = store
            .list(Some("session"))
            .expect("list")
            .pop()
            .expect("record");
        assert_eq!(
            record
                .pre_codex_usage_snapshot
                .expect("pre")
                .telemetry
                .observed_at_unix,
            10
        );
        assert_eq!(
            record
                .post_codex_usage_snapshot
                .expect("post")
                .telemetry
                .observed_at_unix,
            12
        );
        assert_eq!(
            record.usage_delta.rate_limit_used_percent_deltas[0].used_percent_delta,
            15
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_or_incomparable_authoritative_snapshots_remain_unknown() {
        let root = std::env::temp_dir().join(format!("catdesk-accounting-{}", Uuid::new_v4()));
        let store = ExecutionAccountingStoreV1::open(&root).expect("store");
        store
            .record_task_started(
                "catdesk",
                "task",
                "session",
                Some("thread"),
                "codex-cli",
                Some("gpt-5.6-terra"),
                Some(rate_limit_telemetry(20, 10, 100)),
                20_000,
            )
            .expect("start");
        assert!(
            store
                .record_post_codex_observability(
                    "session",
                    rate_limit_telemetry(20, 25, 100),
                    21_000
                )
                .expect("stale post")
        );
        let record = store
            .list(Some("session"))
            .expect("list")
            .pop()
            .expect("record");
        assert_eq!(record.usage_delta.status, "UNKNOWN_NOT_CAPTURED");
        assert!(record.usage_delta.reason.contains("not newer"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn changed_rate_limit_window_is_not_a_comparable_usage_delta() {
        let root = std::env::temp_dir().join(format!("catdesk-accounting-{}", Uuid::new_v4()));
        let store = ExecutionAccountingStoreV1::open(&root).expect("store");
        store
            .record_task_started(
                "catdesk",
                "task",
                "session",
                Some("thread"),
                "codex-cli",
                Some("gpt-5.6-terra"),
                Some(rate_limit_telemetry(20, 10, 100)),
                20_000,
            )
            .expect("start");
        assert!(
            store
                .record_post_codex_observability(
                    "session",
                    rate_limit_telemetry(21, 25, 101),
                    21_000
                )
                .expect("incomparable post")
        );
        let record = store
            .list(Some("session"))
            .expect("list")
            .pop()
            .expect("record");
        assert_eq!(record.usage_delta.status, "UNKNOWN_NOT_CAPTURED");
        assert!(record.usage_delta.reason.contains("no comparable"));
        let _ = fs::remove_dir_all(root);
    }

    fn started_store(
        label: &str,
        session: &str,
        task: &str,
        at: u128,
    ) -> (PathBuf, ExecutionAccountingStoreV1) {
        let root =
            std::env::temp_dir().join(format!("catdesk-accounting-{label}-{}", Uuid::new_v4()));
        let store = ExecutionAccountingStoreV1::open(&root).expect("store");
        store
            .record_task_started(
                "project",
                task,
                session,
                None,
                "codex-cli",
                Some("gpt-5.6-terra"),
                None,
                at,
            )
            .expect("start");
        (root, store)
    }

    #[test]
    fn same_session_task_cannot_rebind_execution_accounting_to_another_project() {
        let root = std::env::temp_dir().join(format!("catdesk-accounting-{}", Uuid::new_v4()));
        let store = ExecutionAccountingStoreV1::open(&root).expect("store");
        store
            .record_task_started(
                "alpha",
                "task",
                "shared-session",
                None,
                "codex-cli",
                Some("gpt-5.6-terra"),
                None,
                1,
            )
            .expect("alpha start");
        assert!(
            store
                .record_task_started(
                    "beta",
                    "task",
                    "shared-session",
                    None,
                    "codex-cli",
                    Some("gpt-5.6-terra"),
                    None,
                    2,
                )
                .is_err(),
            "project-scoped accounting must reject a cross-project replay"
        );
        let records = store.list(Some("shared-session")).expect("records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].project_id, "alpha");
        assert_eq!(records[0].provider_turns, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn report_separates_provider_verification_waiting_and_unknown_time() {
        let (root, store) = started_store("spans", "session", "task", 1_000);
        store
            .activity_started(
                "session",
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                1_100,
            )
            .expect("provider start");
        store
            .activity_finished("session", ActivityActorV1::CodexProviderActive, 2_100)
            .expect("provider end");
        store.waiting_started("session", 2_100).expect("wait start");
        store
            .activity_started(
                "session",
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                3_100,
            )
            .expect("provider resume");
        store
            .activity_finished("session", ActivityActorV1::CodexProviderActive, 4_100)
            .expect("provider end");
        store
            .activity_started(
                "session",
                ActivityActorV1::CatdeskVerificationReviewActive,
                ActivityEvidenceV1::VerifierLifecycle,
                4_200,
            )
            .expect("verify start");
        store
            .activity_finished(
                "session",
                ActivityActorV1::CatdeskVerificationReviewActive,
                4_700,
            )
            .expect("verify end");
        store
            .finish_task("session", None, None, 5_000)
            .expect("finish");
        let report = store
            .work_time_report(1_000, 6_000, None, None, None, 10, 6_000)
            .expect("report");
        let entry = &report.entries[0];
        assert_eq!(entry.provider_active_millis, 2_000);
        assert_eq!(entry.verification_review_active_millis, 500);
        assert_eq!(entry.known_waiting_millis, 1_000);
        assert_eq!(entry.total_known_active_millis, 2_500);
        assert_eq!(entry.chatgpt_web_status, "UNKNOWN_NOT_OBSERVABLE");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn report_uses_union_for_overlapping_actors_and_sessions() {
        let (root, store) = started_store("union", "session-a", "task-a", 1_000);
        store
            .activity_started(
                "session-a",
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                1_000,
            )
            .expect("provider");
        store
            .activity_started(
                "session-a",
                ActivityActorV1::CatdeskOrchestrationActive,
                ActivityEvidenceV1::ControllerTransition,
                2_000,
            )
            .expect("orchestration");
        store
            .activity_finished("session-a", ActivityActorV1::CodexProviderActive, 3_000)
            .expect("provider end");
        store
            .activity_finished(
                "session-a",
                ActivityActorV1::CatdeskOrchestrationActive,
                4_000,
            )
            .expect("orchestration end");
        store
            .finish_task("session-a", None, None, 4_000)
            .expect("finish a");
        store
            .record_task_started(
                "project",
                "task-b",
                "session-b",
                None,
                "codex-cli",
                None,
                None,
                2_000,
            )
            .expect("start b");
        store
            .activity_started(
                "session-b",
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                2_000,
            )
            .expect("provider b");
        store
            .activity_finished("session-b", ActivityActorV1::CodexProviderActive, 5_000)
            .expect("provider b end");
        store
            .finish_task("session-b", None, None, 5_000)
            .expect("finish b");
        let report = store
            .work_time_report(1_000, 6_000, None, None, None, 10, 6_000)
            .expect("report");
        assert_eq!(report.entries[0].total_known_active_millis, 3_000);
        assert_eq!(report.aggregate.total_known_active_millis, 4_000);
        assert_eq!(report.aggregate.wall_millis, 4_000);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn interrupted_or_historical_spans_are_not_stretched_to_query_time() {
        let (root, store) = started_store("interrupt", "session", "task", 1_000);
        store
            .activity_started(
                "session",
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                1_500,
            )
            .expect("provider");
        store.interrupt_open_spans("session").expect("recover");
        store
            .finish_task("session", None, None, 10_000)
            .expect("finish");
        let report = store
            .work_time_report(1_000, 20_000, None, None, None, 10, 20_000)
            .expect("report");
        let entry = &report.entries[0];
        assert_eq!(entry.provider_active_millis, 0);
        assert_eq!(entry.evidence_completeness, "INCOMPLETE_EVIDENCE");
        assert!(!entry.active_now);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn span_cap_persists_evidence_loss_and_report_fails_closed() {
        let (root, store) = started_store("span-cap", "session", "task", 1_000);
        for index in 0..MAX_ACTIVITY_SPANS_PER_RECORD {
            store
                .activity_started(
                    "session",
                    ActivityActorV1::CodexProviderActive,
                    ActivityEvidenceV1::ProviderLifecycle,
                    1_100 + u128::try_from(index).expect("index") * 10,
                )
                .expect("bounded span");
        }
        let before_drop = store
            .list(Some("session"))
            .expect("list")
            .pop()
            .expect("record");
        assert_eq!(
            before_drop.activity_spans.len(),
            MAX_ACTIVITY_SPANS_PER_RECORD
        );
        assert!(!before_drop.activity_evidence_truncated);

        store
            .waiting_started("session", 10_000)
            .expect("dropped span remains non-blocking");
        let after_drop = store
            .list(Some("session"))
            .expect("list")
            .pop()
            .expect("record");
        assert_eq!(
            after_drop.activity_spans.len(),
            MAX_ACTIVITY_SPANS_PER_RECORD
        );
        assert!(after_drop.activity_evidence_truncated);

        store
            .finish_task("session", None, None, 11_000)
            .expect("finish");
        let report = store
            .work_time_report(1_000, 12_000, None, None, None, 10, 12_000)
            .expect("report");
        assert_eq!(
            report.entries[0].evidence_completeness,
            "INCOMPLETE_EVIDENCE"
        );
        assert!(report.entries[0].unobserved_idle_or_unknown_millis > 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn open_span_without_fresh_process_proof_is_incomplete_not_active() {
        let (root, store) = started_store("open", "session", "task", 1_000);
        store
            .activity_started(
                "session",
                ActivityActorV1::CodexProviderActive,
                ActivityEvidenceV1::ProviderLifecycle,
                1_500,
            )
            .expect("provider");
        let report = store
            .work_time_report(1_000, 10_000, None, None, None, 10, 10_000)
            .expect("report");
        let entry = &report.entries[0];
        assert_eq!(entry.provider_active_millis, 0);
        assert_eq!(entry.evidence_completeness, "INCOMPLETE_EVIDENCE");
        assert!(!entry.active_now);
        assert!(entry.current_actor.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn historical_v1_record_without_activity_spans_remains_readable_and_unknown() {
        let (root, store) = started_store("migration", "session", "task", 1_000);
        store
            .finish_task("session", None, None, 2_000)
            .expect("finish");
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path()).expect("ledger")).expect("json");
        legacy
            .pointer_mut("/records/0")
            .expect("record")
            .as_object_mut()
            .expect("object")
            .remove("activitySpans");
        legacy
            .pointer_mut("/records/0")
            .expect("record")
            .as_object_mut()
            .expect("object")
            .remove("activityEvidenceTruncated");
        fs::write(
            store.path(),
            serde_json::to_vec(&legacy).expect("serialize"),
        )
        .expect("legacy write");
        let report = store
            .work_time_report(1_000, 3_000, None, None, None, 10, 3_000)
            .expect("report");
        assert_eq!(
            report.entries[0].evidence_completeness,
            "INCOMPLETE_EVIDENCE"
        );
        assert_eq!(report.entries[0].total_known_active_millis, 0);
        assert!(!store.list(Some("session")).expect("legacy list")[0].activity_evidence_truncated);
        let _ = fs::remove_dir_all(root);
    }
}
