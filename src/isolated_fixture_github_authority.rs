//! T0478 R3B4-B2: GitHub REST witnesses for a fixed CatDesk branch.
//! Not a protected release approval or Git signature. Only the fixed HTTPS
//! fetch paths can construct witnesses; caller-provided JSON is never authority.
//! This dormant module does NOT start an agent or serve fixture files.

use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration;

const REPOSITORY: &str = "vnath99/CatDesk";
const BRANCH: &str = "orchestrator/chatgpt-codex-autonomous-loop";
const WORKFLOW: &str = ".github/workflows/ci.yml";
const WORKFLOW_FILE: &str = "ci.yml";
const MAX_RESPONSE: usize = 256 * 1024;
const MAX_MATCHING_RUNS: usize = 20;
const REQUIRED_JOBS: [&str; 3] = [
    "Rust",
    "Python Wake and advisor tests",
    "Independent WakeHost Rust",
];

/// The sole constructor lives inside fetch_live_witness; JSON alone cannot
/// create this type. The eventual snapshot producer must keep the value
/// in-process and recheck it against the exact Git object bytes.
#[derive(Clone, Debug)]
pub(crate) struct LiveGithubSourceWitness {
    commit_sha: String,
    run_id: u64,
}

impl LiveGithubSourceWitness {
    pub(crate) fn commit_sha(&self) -> &str {
        &self.commit_sha
    }
    pub(crate) fn run_id(&self) -> u64 {
        self.run_id
    }
}

/// An exact commit's completed push CI, usable only by the read-only owner
/// diagnostic. This type cannot satisfy the current-head fixture launch gate.
#[derive(Clone, Debug)]
pub(crate) struct HistoricalGithubDiagnosticWitness {
    commit_sha: String,
    run_id: u64,
    run_attempt: u64,
}

impl HistoricalGithubDiagnosticWitness {
    pub(crate) fn commit_sha(&self) -> &str {
        &self.commit_sha
    }
    pub(crate) fn run_id(&self) -> u64 {
        self.run_id
    }
    pub(crate) fn run_attempt(&self) -> u64 {
        self.run_attempt
    }
}

fn is_full_commit(sha: &str) -> bool {
    sha.len() == 40
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn branch_matches(bytes: &[u8], expected_sha: &str) -> Result<(), &'static str> {
    let data: Value =
        serde_json::from_slice(bytes).map_err(|_| "GitHub branch response invalid")?;
    if data.get("name").and_then(Value::as_str) != Some(BRANCH)
        || data.pointer("/commit/sha").and_then(Value::as_str) != Some(expected_sha)
    {
        return Err("GitHub branch head does not match expected commit");
    }
    Ok(())
}

fn select_successful_run(bytes: &[u8], expected_sha: &str) -> Result<u64, &'static str> {
    let data: Value = serde_json::from_slice(bytes).map_err(|_| "GitHub runs response invalid")?;
    let runs = data
        .get("workflow_runs")
        .and_then(Value::as_array)
        .ok_or("GitHub workflow runs unavailable")?;
    if runs.len() > 20 {
        return Err("GitHub workflow run response exceeds bound");
    }
    // Prefer the greatest GitHub run ID for the exact commit rather than
    // relying on response order. An older green run cannot mask a newer
    // cancelled, failed, or still-running matching run.
    let newest = runs
        .iter()
        .filter(|run| {
            run.get("head_sha").and_then(Value::as_str) == Some(expected_sha)
                && run.get("head_branch").and_then(Value::as_str) == Some(BRANCH)
                && run.get("path").and_then(Value::as_str) == Some(WORKFLOW)
                && run.get("event").and_then(Value::as_str) == Some("push")
        })
        .max_by_key(|run| run.get("id").and_then(Value::as_u64).unwrap_or(0))
        .ok_or("GitHub CI for expected commit is missing")?;
    if newest.get("status").and_then(Value::as_str) != Some("completed")
        || newest.get("conclusion").and_then(Value::as_str) != Some("success")
    {
        return Err("GitHub CI for expected commit is not green");
    }
    newest
        .get("id")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
        .ok_or("GitHub CI run identifier invalid")
}

fn select_historical_diagnostic_run(
    bytes: &[u8],
    expected_sha: &str,
) -> Result<(u64, u64), &'static str> {
    let data: Value = serde_json::from_slice(bytes).map_err(|_| "GitHub runs response invalid")?;
    let runs = data
        .get("workflow_runs")
        .and_then(Value::as_array)
        .ok_or("GitHub workflow runs unavailable")?;
    let total = data
        .get("total_count")
        .and_then(Value::as_u64)
        .ok_or("GitHub workflow run count missing")?;
    if total == 0 {
        return Err("GitHub push CI for historical commit is missing");
    }
    if total > MAX_MATCHING_RUNS as u64 || runs.len() as u64 != total {
        return Err("GitHub historical workflow run inventory is incomplete or unexpected");
    }
    // The API query is fixed to this SHA, workflow, branch and push event, but
    // all returned identities are checked again before choosing the newest.
    if runs.iter().any(|run| {
        run.get("head_sha").and_then(Value::as_str) != Some(expected_sha)
            || run.get("head_branch").and_then(Value::as_str) != Some(BRANCH)
            || run.get("path").and_then(Value::as_str) != Some(WORKFLOW)
            || run.get("event").and_then(Value::as_str) != Some("push")
            || run
                .get("id")
                .and_then(Value::as_u64)
                .is_none_or(|id| id == 0)
            || run
                .get("run_attempt")
                .and_then(Value::as_u64)
                .is_none_or(|attempt| attempt == 0)
    }) {
        return Err("GitHub historical push CI identity does not match request");
    }
    let mut seen_ids = HashSet::with_capacity(runs.len());
    if runs
        .iter()
        .any(|run| !seen_ids.insert(run.get("id").and_then(Value::as_u64).unwrap_or(0)))
    {
        return Err("GitHub historical push CI run identifiers duplicated");
    }
    let newest = runs
        .iter()
        .max_by_key(|run| run.get("id").and_then(Value::as_u64).unwrap_or(0))
        .ok_or("GitHub push CI for historical commit is missing")?;
    if newest.get("status").and_then(Value::as_str) != Some("completed")
        || newest.get("conclusion").and_then(Value::as_str) != Some("success")
    {
        return Err("GitHub push CI for historical commit is not green");
    }
    Ok((
        newest.get("id").and_then(Value::as_u64).unwrap_or(0),
        newest
            .get("run_attempt")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    ))
}

fn jobs_are_green(bytes: &[u8], run_id: u64) -> Result<(), &'static str> {
    let data: Value = serde_json::from_slice(bytes).map_err(|_| "GitHub jobs response invalid")?;
    let jobs = data
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or("GitHub CI jobs missing")?;
    let reported = data
        .get("total_count")
        .and_then(Value::as_u64)
        .ok_or("GitHub CI job count missing")?;
    if reported != REQUIRED_JOBS.len() as u64 || jobs.len() != REQUIRED_JOBS.len() {
        return Err("GitHub CI job inventory is incomplete or unexpected");
    }
    for required in REQUIRED_JOBS {
        if jobs
            .iter()
            .filter(|job| {
                job.get("name").and_then(Value::as_str) == Some(required)
                    && job.get("run_id").and_then(Value::as_u64) == Some(run_id)
                    && job.get("status").and_then(Value::as_str) == Some("completed")
                    && job.get("conclusion").and_then(Value::as_str) == Some("success")
            })
            .count()
            != 1
        {
            return Err("GitHub required CI job not green");
        }
    }
    Ok(())
}

fn historical_jobs_are_green_windows(
    bytes: &[u8],
    run_id: u64,
    run_attempt: u64,
    expected_sha: &str,
) -> Result<(), &'static str> {
    jobs_are_green(bytes, run_id)?;
    let data: Value = serde_json::from_slice(bytes).map_err(|_| "GitHub jobs response invalid")?;
    let jobs = data
        .get("jobs")
        .and_then(Value::as_array)
        .ok_or("GitHub CI jobs missing")?;
    if jobs.iter().any(|job| {
        job.get("head_sha").and_then(Value::as_str) != Some(expected_sha)
            || job.get("head_branch").and_then(Value::as_str) != Some(BRANCH)
            || job.get("run_attempt").and_then(Value::as_u64) != Some(run_attempt)
            || !job
                .get("labels")
                .and_then(Value::as_array)
                .is_some_and(|labels| {
                    labels
                        .iter()
                        .any(|label| label.as_str() == Some("windows-latest"))
                })
    }) {
        return Err("GitHub historical CI job source or Windows runner mismatch");
    }
    Ok(())
}

/// The only network source of this authority is HTTPS to the exact GitHub API
/// host, with redirects forbidden and no access token or caller URL accepted.
async fn get_fixed(client: &reqwest::Client, path: &str) -> Result<Vec<u8>, String> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/{path}");
    let mut response = client
        .get(&url)
        .send()
        .await
        .map_err(|_| "GitHub verification request failed")?;
    if response.status() != reqwest::StatusCode::OK {
        return Err("GitHub verification unavailable or not authorized".into());
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE as u64)
    {
        return Err("GitHub verification response exceeds bound".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "GitHub verification response incomplete")?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE {
            return Err("GitHub verification response exceeds bound".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Verify current branch tip, *latest* corresponding push CI run, and all
/// three named jobs. Recheck branch head to refuse drift during the operation.
/// This witness is only a live-source gate, never a production signing grant.
pub(crate) async fn fetch_live_witness(
    expected_sha: &str,
) -> Result<LiveGithubSourceWitness, String> {
    if !is_full_commit(expected_sha) {
        return Err("GitHub source commit syntax invalid".into());
    }
    let client = reqwest::Client::builder()
        .user_agent("CatDesk-Readonly-Fixture-Source/1")
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|_| "GitHub verification client unavailable")?;
    let branch_path = format!("branches/{BRANCH}");
    let branch = get_fixed(&client, &branch_path).await?;
    branch_matches(&branch, expected_sha).map_err(str::to_owned)?;

    let runs = get_fixed(
        &client,
        &format!("actions/runs?branch={BRANCH}&per_page=20"),
    )
    .await?;
    let run_id = select_successful_run(&runs, expected_sha).map_err(str::to_owned)?;
    let jobs = get_fixed(&client, &format!("actions/runs/{run_id}/jobs?per_page=10")).await?;
    jobs_are_green(&jobs, run_id).map_err(str::to_owned)?;

    let final_branch = get_fixed(&client, &branch_path).await?;
    branch_matches(&final_branch, expected_sha).map_err(str::to_owned)?;
    Ok(LiveGithubSourceWitness {
        commit_sha: expected_sha.to_owned(),
        run_id,
    })
}

/// Verify an immutable source SHA's push CI on the fixed feature branch and
/// workflow, without requiring that SHA to remain the branch tip. This is
/// diagnostic provenance only, never fixture launch or staging authority.
async fn verify_historical_diagnostic_with<F, Fut>(
    expected_sha: &str,
    mut fetch: F,
) -> Result<HistoricalGithubDiagnosticWitness, String>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
{
    if !is_full_commit(expected_sha) {
        return Err("GitHub source commit syntax invalid".into());
    }
    let runs_path = format!(
        "actions/workflows/{WORKFLOW_FILE}/runs?branch={BRANCH}&event=push&head_sha={expected_sha}&per_page={MAX_MATCHING_RUNS}"
    );
    let runs = fetch(runs_path.clone()).await?;
    let (run_id, run_attempt) =
        select_historical_diagnostic_run(&runs, expected_sha).map_err(str::to_owned)?;
    let jobs = fetch(format!(
        "actions/runs/{run_id}/jobs?filter=latest&per_page=10"
    ))
    .await?;
    historical_jobs_are_green_windows(&jobs, run_id, run_attempt, expected_sha)
        .map_err(str::to_owned)?;
    // A rerun can change the current attempt while its jobs are fetched.
    // Refuse that race and a newly arrived push run for this exact SHA.
    let final_runs = fetch(runs_path).await?;
    if select_historical_diagnostic_run(&final_runs, expected_sha).map_err(str::to_owned)?
        != (run_id, run_attempt)
    {
        return Err("GitHub historical push CI changed during verification".into());
    }
    Ok(HistoricalGithubDiagnosticWitness {
        commit_sha: expected_sha.to_owned(),
        run_id,
        run_attempt,
    })
}

pub(crate) async fn fetch_historical_diagnostic_witness(
    expected_sha: &str,
) -> Result<HistoricalGithubDiagnosticWitness, String> {
    if !is_full_commit(expected_sha) {
        return Err("GitHub source commit syntax invalid".into());
    }
    let client = reqwest::Client::builder()
        .user_agent("CatDesk-Readonly-Fixture-Diagnostic/1")
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|_| "GitHub verification client unavailable")?;
    verify_historical_diagnostic_with(expected_sha, move |path| {
        let client = client.clone();
        async move { get_fixed(&client, &path).await }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";

    fn branch(sha: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"name":BRANCH,"commit":{"sha":sha}})).unwrap()
    }

    fn runs(status: &str, result: &str, sha: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"workflow_runs":[{
            "id":42,"head_sha":sha,"head_branch":BRANCH,
            "path":WORKFLOW,"event":"push","status":status,"conclusion":result
        }]}))
        .unwrap()
    }

    fn jobs(changed: Option<&str>) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "total_count":3,
            "jobs":REQUIRED_JOBS.iter().map(|name| json!({
                "name":name,"run_id":42,"status":"completed",
                "conclusion":if Some(*name)==changed {"failure"} else {"success"}
            })).collect::<Vec<_>>()
        }))
        .unwrap()
    }

    fn historical_run(id: u64, status: &str, conclusion: &str) -> Value {
        json!({
            "id":id,"run_attempt":1,"head_sha":SHA,"head_branch":BRANCH,
            "path":WORKFLOW,"event":"push","status":status,"conclusion":conclusion
        })
    }

    fn historical_runs(runs: Vec<Value>, total_count: usize) -> Vec<u8> {
        serde_json::to_vec(&json!({"total_count":total_count,"workflow_runs":runs})).unwrap()
    }

    fn historical_jobs() -> Value {
        json!({
            "total_count":3,
            "jobs":REQUIRED_JOBS.iter().map(|name| json!({
                "name":name,"run_id":42,"run_attempt":1,"head_sha":SHA,"head_branch":BRANCH,
                "labels":["windows-latest"],"status":"completed","conclusion":"success"
            })).collect::<Vec<_>>()
        })
    }

    #[test]
    fn branch_identity_is_exact() {
        assert!(branch_matches(&branch(SHA), SHA).is_ok());
        assert!(branch_matches(&branch(SHA), &"0".repeat(40)).is_err());
        assert!(branch_matches(br#"{"name":"main","commit":{"sha":"x"}}"#, SHA).is_err());
        assert!(branch_matches(b"{}", SHA).is_err());
    }

    #[test]
    fn latest_ci_must_match_branch_sha_event_and_success() {
        assert_eq!(
            select_successful_run(&runs("completed", "success", SHA), SHA),
            Ok(42)
        );
        for (state, result) in [
            ("queued", "null"),
            ("in_progress", "success"),
            ("completed", "failure"),
            ("completed", "cancelled"),
        ] {
            assert!(select_successful_run(&runs(state, result, SHA), SHA).is_err());
        }
        assert!(
            select_successful_run(&runs("completed", "success", SHA), &"0".repeat(40)).is_err()
        );
        let wrong_event = serde_json::to_vec(&json!({"workflow_runs":[{
            "id":42,"head_sha":SHA,"head_branch":BRANCH,
            "path":WORKFLOW,"event":"pull_request","status":"completed","conclusion":"success"
        }]}))
        .unwrap();
        assert!(select_successful_run(&wrong_event, SHA).is_err());
    }

    #[test]
    fn newer_failed_or_unfinished_run_cannot_be_masked_by_green_run() {
        let mixed = serde_json::to_vec(&json!({
            "workflow_runs":[
                {"id":42,"head_sha":SHA,"head_branch":BRANCH,"path":WORKFLOW,
                 "event":"push","status":"completed","conclusion":"success"},
                {"id":44,"head_sha":SHA,"head_branch":BRANCH,"path":WORKFLOW,
                 "event":"push","status":"completed","conclusion":"failure"}
            ]
        }))
        .unwrap();
        assert!(select_successful_run(&mixed, SHA).is_err());
        let pending = serde_json::to_vec(&json!({
            "workflow_runs":[
                {"id":44,"head_sha":SHA,"head_branch":BRANCH,"path":WORKFLOW,
                 "event":"push","status":"in_progress","conclusion":null},
                {"id":42,"head_sha":SHA,"head_branch":BRANCH,"path":WORKFLOW,
                 "event":"push","status":"completed","conclusion":"success"}
            ]
        }))
        .unwrap();
        assert!(select_successful_run(&pending, SHA).is_err());
    }

    #[test]
    fn every_expected_ci_job_must_succeed_exactly_once() {
        assert_eq!(jobs_are_green(&jobs(None), 42), Ok(()));
        for name in REQUIRED_JOBS {
            assert!(jobs_are_green(&jobs(Some(name)), 42).is_err());
        }
        assert!(jobs_are_green(&jobs(None), 41).is_err());
        let truncated = serde_json::to_vec(&json!({"total_count":3,"jobs":[]})).unwrap();
        assert!(jobs_are_green(&truncated, 42).is_err());
    }

    #[test]
    fn witness_commit_syntax_rejects_nonhex_and_short_values() {
        assert!(is_full_commit(SHA));
        assert!(!is_full_commit("HEAD"));
        assert!(!is_full_commit(&"g".repeat(40)));
        assert!(!is_full_commit(&"A".repeat(40)));
    }

    #[test]
    fn historical_diagnostic_selects_latest_exact_commit_push_run() {
        let one = historical_runs(vec![historical_run(42, "completed", "success")], 1);
        assert_eq!(select_historical_diagnostic_run(&one, SHA), Ok((42, 1)));
        let newer_failed = historical_runs(
            vec![
                historical_run(42, "completed", "success"),
                historical_run(44, "completed", "failure"),
            ],
            2,
        );
        assert!(select_historical_diagnostic_run(&newer_failed, SHA).is_err());
        let newer_pending = historical_runs(
            vec![
                historical_run(44, "in_progress", "success"),
                historical_run(42, "completed", "success"),
            ],
            2,
        );
        assert!(select_historical_diagnostic_run(&newer_pending, SHA).is_err());
    }

    #[test]
    fn historical_diagnostic_rejects_unbounded_or_misidentified_runs() {
        let good = historical_run(42, "completed", "success");
        assert!(select_historical_diagnostic_run(&historical_runs(vec![], 0), SHA).is_err());
        assert!(
            select_historical_diagnostic_run(&historical_runs(vec![good.clone()], 2), SHA).is_err()
        );
        assert!(
            select_historical_diagnostic_run(&historical_runs(vec![good.clone()], 21), SHA)
                .is_err()
        );
        assert!(
            select_historical_diagnostic_run(
                &historical_runs(vec![good.clone(), good.clone()], 2),
                SHA
            )
            .is_err()
        );
        for (field, changed) in [
            (
                "head_sha",
                json!("0000000000000000000000000000000000000000"),
            ),
            ("head_branch", json!("main")),
            ("path", json!(".github/workflows/other.yml")),
            ("event", json!("workflow_dispatch")),
            ("id", json!(0)),
            ("run_attempt", json!(0)),
        ] {
            let mut bad = good.clone();
            bad[field] = changed;
            assert!(
                select_historical_diagnostic_run(&historical_runs(vec![bad], 1), SHA).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn historical_diagnostic_requires_exact_green_windows_job_identity() {
        let good = historical_jobs();
        let bytes = serde_json::to_vec(&good).unwrap();
        assert_eq!(
            historical_jobs_are_green_windows(&bytes, 42, 1, SHA),
            Ok(())
        );
        for (field, changed) in [
            (
                "head_sha",
                json!("0000000000000000000000000000000000000000"),
            ),
            ("head_branch", json!("main")),
            ("run_attempt", json!(2)),
            ("labels", json!(["ubuntu-latest"])),
            ("conclusion", json!("failure")),
        ] {
            let mut bad = good.clone();
            bad["jobs"][0][field] = changed;
            let bytes = serde_json::to_vec(&bad).unwrap();
            assert!(
                historical_jobs_are_green_windows(&bytes, 42, 1, SHA).is_err(),
                "{field}"
            );
        }
        let mut duplicate = good;
        duplicate["jobs"][1]["name"] = json!(REQUIRED_JOBS[0]);
        let bytes = serde_json::to_vec(&duplicate).unwrap();
        assert!(historical_jobs_are_green_windows(&bytes, 42, 1, SHA).is_err());
    }

    #[tokio::test]
    async fn historical_diagnostic_fetches_only_fixed_paths_and_rechecks_attempt() {
        use std::collections::VecDeque;

        let runs_path = format!(
            "actions/workflows/ci.yml/runs?branch={BRANCH}&event=push&head_sha={SHA}&per_page=20"
        );
        let jobs_path = "actions/runs/42/jobs?filter=latest&per_page=10".to_owned();
        let green_run = historical_runs(vec![historical_run(42, "completed", "success")], 1);
        let mut replies = VecDeque::from([
            (runs_path.clone(), green_run.clone()),
            (
                jobs_path.clone(),
                serde_json::to_vec(&historical_jobs()).unwrap(),
            ),
            (runs_path.clone(), green_run.clone()),
        ]);
        let witness = verify_historical_diagnostic_with(SHA, |path| {
            let (expected_path, bytes) = replies.pop_front().expect("only three API requests");
            assert_eq!(path, expected_path);
            async move { Ok(bytes) }
        })
        .await
        .expect("fixed historical push CI");
        assert!(replies.is_empty());
        assert_eq!(witness.commit_sha(), SHA);
        assert_eq!(witness.run_id(), 42);
        assert_eq!(witness.run_attempt(), 1);

        let mut rerun = historical_run(42, "completed", "success");
        rerun["run_attempt"] = json!(2);
        let mut replies = VecDeque::from([
            (runs_path.clone(), green_run),
            (jobs_path, serde_json::to_vec(&historical_jobs()).unwrap()),
            (runs_path, historical_runs(vec![rerun], 1)),
        ]);
        assert!(
            verify_historical_diagnostic_with(SHA, |path| {
                let (expected_path, bytes) = replies.pop_front().expect("only three API requests");
                assert_eq!(path, expected_path);
                async move { Ok(bytes) }
            })
            .await
            .is_err()
        );
    }
}
