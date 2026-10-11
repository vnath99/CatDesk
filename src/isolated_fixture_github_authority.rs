//! T0478 R3B4-B2: live GitHub REST witness for a fixed CatDesk branch.
//! Not a protected release approval or Git signature. Only the private live
//! HTTPS path can construct a witness; caller-provided JSON is never authority.
//! This dormant module does NOT start an agent or serve fixture files.

use serde_json::Value;
use std::time::Duration;

const REPOSITORY: &str = "vnath99/CatDesk";
const BRANCH: &str = "orchestrator/chatgpt-codex-autonomous-loop";
const WORKFLOW: &str = ".github/workflows/ci.yml";
const MAX_RESPONSE: usize = 256 * 1024;
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
}
