//! Pure comparison of a signed install approval's CI claim with GitHub API JSON.
//!
//! This is a diagnostic building block for a future **offline issuer**, not an
//! installation capability. The issuer must independently fetch every page over
//! HTTPS from the fixed `api.github.com/repos/vnath99/CatDesk` endpoints, with
//! redirects and caller-chosen URLs forbidden. It must use the workflow-runs
//! query for workflow 311012404, the fixed branch, `event=push`, and the exact
//! source SHA, then fetch the selected run's attempt-specific jobs endpoint.
//! Run pages must be fetched again after jobs. Page numbers supplied here must
//! correspond to `page=N&per_page=100` in those requests. Candidate-provided
//! JSON, including a candidate's copy of these responses, has no authority.
//! This module deliberately performs no I/O, signs nothing, and grants nothing.

use crate::install_broker_approval::ExactCiV1;
use serde::Deserialize;
use std::collections::BTreeSet;

const REPOSITORY: &str = "vnath99/CatDesk";
const WORKFLOW_PATH: &str = ".github/workflows/ci.yml";
const WORKFLOW_ID: u64 = 311_012_404;
const BRANCH: &str = "orchestrator/chatgpt-codex-autonomous-loop";
const REQUIRED_JOBS: [&str; 3] = [
    "Rust",
    "Python Wake and advisor tests",
    "Independent WakeHost Rust",
];
const PER_PAGE: usize = 100;
const MAX_RUNS: usize = 300;
const MAX_PAGE_BYTES: usize = 1024 * 1024;

/// Bytes fetched from page `page_number` of a fixed GitHub API request.
/// The number is bookkeeping, not proof of the response's origin.
#[derive(Clone, Copy)]
pub struct CiJsonPage<'a> {
    pub page_number: u32,
    pub body: &'a [u8],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct Repository {
    full_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct WorkflowRun {
    id: u64,
    workflow_id: u64,
    path: String,
    repository: Repository,
    head_repository: Option<Repository>,
    head_branch: Option<String>,
    head_sha: String,
    event: String,
    status: String,
    conclusion: Option<String>,
    run_attempt: u64,
    run_number: u64,
}

#[derive(Deserialize)]
struct RunsResponse {
    total_count: u64,
    workflow_runs: Vec<WorkflowRun>,
}

#[derive(Deserialize)]
struct Job {
    id: u64,
    run_id: u64,
    run_attempt: u64,
    head_sha: String,
    name: String,
    status: String,
    conclusion: Option<String>,
    labels: Vec<String>,
}

#[derive(Deserialize)]
struct JobsResponse {
    total_count: u64,
    jobs: Vec<Job>,
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_claim(ci: &ExactCiV1) -> Result<(), &'static str> {
    if ci.repository != REPOSITORY
        || ci.workflow_path != WORKFLOW_PATH
        || ci.workflow_id != WORKFLOW_ID
        || ci.branch != BRANCH
        || !valid_sha(&ci.head_sha)
        || ci.event != "push"
        || ci.run_status != "completed"
        || ci.run_conclusion != "success"
        || ci.run_id == 0
        || ci.run_attempt == 0
    {
        return Err("signed CI claim has an invalid fixed identity");
    }
    let mut seen = BTreeSet::new();
    for (job, required_name) in ci.jobs.iter().zip(REQUIRED_JOBS) {
        if job.name != required_name
            || job.job_id == 0
            || !seen.insert(job.job_id)
            || job.conclusion != "success"
        {
            return Err("signed CI claim has an invalid job identity");
        }
    }
    Ok(())
}

fn expected_page_len(total: usize, index: usize) -> usize {
    total.saturating_sub(index * PER_PAGE).min(PER_PAGE)
}

fn validate_page_set(
    pages: &[CiJsonPage<'_>],
    total: usize,
    max_total: usize,
) -> Result<(), &'static str> {
    if total > max_total {
        return Err("GitHub inventory exceeds bound");
    }
    let expected_pages = total.max(1).div_ceil(PER_PAGE);
    if pages.len() != expected_pages {
        return Err("GitHub pagination is incomplete");
    }
    for (index, page) in pages.iter().enumerate() {
        if page.page_number != (index + 1) as u32
            || page.body.is_empty()
            || page.body.len() > MAX_PAGE_BYTES
        {
            return Err("GitHub page number or byte bound is invalid");
        }
    }
    Ok(())
}

fn parse_runs(pages: &[CiJsonPage<'_>], ci: &ExactCiV1) -> Result<Vec<WorkflowRun>, &'static str> {
    if pages.is_empty() || pages.len() > MAX_RUNS.div_ceil(PER_PAGE) {
        return Err("GitHub run pagination is empty or exceeds bound");
    }
    let mut runs = Vec::new();
    let mut total = None;
    let mut seen_ids = BTreeSet::new();
    for (index, page) in pages.iter().enumerate() {
        if page.body.is_empty() || page.body.len() > MAX_PAGE_BYTES {
            return Err("GitHub run page exceeds byte bound");
        }
        let parsed: RunsResponse =
            serde_json::from_slice(page.body).map_err(|_| "GitHub run JSON is invalid")?;
        let count =
            usize::try_from(parsed.total_count).map_err(|_| "GitHub run count exceeds bound")?;
        if count > MAX_RUNS || total.is_some_and(|prior| prior != count) {
            return Err("GitHub run count is inconsistent or exceeds bound");
        }
        total = Some(count);
        if parsed.workflow_runs.len() != expected_page_len(count, index) {
            return Err("GitHub run page is incomplete");
        }
        for run in parsed.workflow_runs {
            if run.id == 0
                || run.workflow_id != WORKFLOW_ID
                || run.path != WORKFLOW_PATH
                || run.repository.full_name != REPOSITORY
                || run
                    .head_repository
                    .as_ref()
                    .map(|value| value.full_name.as_str())
                    != Some(REPOSITORY)
                || run.head_branch.as_deref() != Some(BRANCH)
                || run.head_sha != ci.head_sha
                || run.event != "push"
                || run.run_attempt == 0
                || run.run_number == 0
                || !seen_ids.insert(run.id)
            {
                return Err("GitHub run identity is unexpected or duplicated");
            }
            runs.push(run);
        }
    }
    validate_page_set(pages, total.unwrap_or(0), MAX_RUNS)?;
    if runs.is_empty() {
        return Err("GitHub has no matching push CI run");
    }
    Ok(runs)
}

fn parse_jobs(pages: &[CiJsonPage<'_>], ci: &ExactCiV1) -> Result<(), &'static str> {
    if pages.len() != 1 {
        return Err("GitHub job pagination is incomplete or unexpected");
    }
    let page = &pages[0];
    if page.body.is_empty() || page.body.len() > MAX_PAGE_BYTES {
        return Err("GitHub job page exceeds byte bound");
    }
    let parsed: JobsResponse =
        serde_json::from_slice(page.body).map_err(|_| "GitHub job JSON is invalid")?;
    let total =
        usize::try_from(parsed.total_count).map_err(|_| "GitHub job count exceeds bound")?;
    validate_page_set(pages, total, REQUIRED_JOBS.len())?;
    if total != REQUIRED_JOBS.len() || parsed.jobs.len() != total {
        return Err("GitHub job inventory is incomplete or unexpected");
    }
    let mut seen_ids = BTreeSet::new();
    let mut seen_names = BTreeSet::new();
    for job in parsed.jobs {
        let signed = ci
            .jobs
            .iter()
            .find(|expected| expected.name == job.name)
            .ok_or("GitHub job name is unexpected")?;
        if job.id == 0
            || job.id != signed.job_id
            || !seen_ids.insert(job.id)
            || !seen_names.insert(job.name)
            || job.run_id != ci.run_id
            || job.run_attempt != u64::from(ci.run_attempt)
            || job.head_sha != ci.head_sha
            || job.status != "completed"
            || job.conclusion.as_deref() != Some("success")
            || job.labels.len() != 1
            || job.labels[0] != "windows-latest"
        {
            return Err("GitHub job does not match signed successful Windows job");
        }
    }
    Ok(())
}

/// Compare the signed CI claim against two independently fetched, complete run
/// inventories surrounding an independently fetched selected-attempt job
/// inventory. When checking an existing approval, verify its signature first.
/// A future offline issuer may use this on a prospective claim that it built
/// from its own fixed GitHub API fetches before signing. Submitting clients
/// must never choose or supply these bytes.
///
/// For both run inventories, use
/// `/actions/workflows/311012404/runs?branch=<fixed-branch>&event=push&head_sha=<signed-SHA>&per_page=100&page=N`.
/// For jobs, use `/actions/runs/<selected-id>/attempts/<selected-attempt>/jobs?per_page=100&page=N`.
/// A successful comparison is only a fact about those independently obtained
/// bytes. It is not approval to install or a signing decision.
pub fn compare_ci_claim_to_independent_json(
    ci: &ExactCiV1,
    runs_before: &[CiJsonPage<'_>],
    jobs: &[CiJsonPage<'_>],
    runs_after: &[CiJsonPage<'_>],
) -> Result<(), &'static str> {
    validate_claim(ci)?;
    let first = parse_runs(runs_before, ci)?;
    let newest = first
        .iter()
        .max_by_key(|run| run.id)
        .ok_or("GitHub has no matching push CI run")?;
    if newest.id != ci.run_id
        || newest.run_attempt != u64::from(ci.run_attempt)
        || newest.status != "completed"
        || newest.conclusion.as_deref() != Some("success")
    {
        return Err("latest GitHub run does not match signed successful CI claim");
    }
    parse_jobs(jobs, ci)?;
    let second = parse_runs(runs_after, ci)?;
    if first != second {
        return Err("GitHub run inventory changed during CI comparison");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_broker_approval::CiJobV1;
    use serde_json::{Value, json};

    const SHA: &str = "4e9a2e212c47b0b7229b41874a3b1ec5bfd7d083";

    fn claim() -> ExactCiV1 {
        ExactCiV1 {
            repository: REPOSITORY.into(),
            workflow_path: WORKFLOW_PATH.into(),
            workflow_id: WORKFLOW_ID,
            branch: BRANCH.into(),
            head_sha: SHA.into(),
            event: "push".into(),
            run_status: "completed".into(),
            run_conclusion: "success".into(),
            run_id: 42,
            run_attempt: 2,
            jobs: std::array::from_fn(|index| CiJobV1 {
                name: REQUIRED_JOBS[index].into(),
                job_id: 100 + index as u64,
                conclusion: "success".into(),
            }),
        }
    }

    fn run(id: u64, attempt: u64) -> Value {
        json!({
            "id":id,"workflow_id":WORKFLOW_ID,"path":WORKFLOW_PATH,
            "repository":{"full_name":REPOSITORY},
            "head_repository":{"full_name":REPOSITORY},
            "head_branch":BRANCH,"head_sha":SHA,"event":"push",
            "status":"completed","conclusion":"success",
            "run_attempt":attempt,"run_number":id
        })
    }

    fn run_page(runs: Vec<Value>, total: usize) -> Vec<u8> {
        serde_json::to_vec(&json!({"total_count":total,"workflow_runs":runs})).unwrap()
    }

    fn job(id: u64, name: &str) -> Value {
        json!({
            "id":id,"run_id":42,"run_attempt":2,"head_sha":SHA,
            "name":name,"status":"completed","conclusion":"success",
            "labels":["windows-latest"]
        })
    }

    fn job_page(jobs: Vec<Value>, total: usize) -> Vec<u8> {
        serde_json::to_vec(&json!({"total_count":total,"jobs":jobs})).unwrap()
    }

    fn good_jobs() -> Vec<Value> {
        REQUIRED_JOBS
            .iter()
            .enumerate()
            .map(|(index, name)| job(100 + index as u64, name))
            .collect()
    }

    fn compare(ci: &ExactCiV1, first: &[u8], jobs: &[u8], second: &[u8]) -> bool {
        compare_ci_claim_to_independent_json(
            ci,
            &[CiJsonPage {
                page_number: 1,
                body: first,
            }],
            &[CiJsonPage {
                page_number: 1,
                body: jobs,
            }],
            &[CiJsonPage {
                page_number: 1,
                body: second,
            }],
        )
        .is_ok()
    }

    #[test]
    fn accepts_latest_successful_run_and_exact_three_windows_jobs() {
        let runs = run_page(vec![run(42, 2), run(41, 1)], 2);
        let jobs = job_page(good_jobs(), 3);
        assert!(compare(&claim(), &runs, &jobs, &runs));
    }

    #[test]
    fn rejects_newer_failed_or_unsigned_run_and_attempt() {
        let jobs = job_page(good_jobs(), 3);
        let mut latest = run(43, 1);
        latest["conclusion"] = json!("failure");
        let runs = run_page(vec![run(42, 2), latest], 2);
        assert!(!compare(&claim(), &runs, &jobs, &runs));
        let mut rerun = run(42, 3);
        rerun["status"] = json!("in_progress");
        rerun["conclusion"] = Value::Null;
        let runs = run_page(vec![rerun], 1);
        assert!(!compare(&claim(), &runs, &jobs, &runs));
    }

    #[test]
    fn rejects_wrong_run_source_and_signed_claim() {
        let jobs = job_page(good_jobs(), 3);
        for (field, value) in [
            ("workflow_id", json!(1)),
            ("path", json!(".github/workflows/other.yml")),
            ("repository", json!({"full_name":"attacker/repo"})),
            ("head_repository", json!({"full_name":"attacker/repo"})),
            ("head_branch", json!("main")),
            ("head_sha", json!("0".repeat(40))),
            ("event", json!("workflow_dispatch")),
            ("run_number", json!(0)),
        ] {
            let mut changed = run(42, 2);
            changed[field] = value;
            let runs = run_page(vec![changed], 1);
            assert!(!compare(&claim(), &runs, &jobs, &runs), "{field}");
        }
        let runs = run_page(vec![run(42, 2)], 1);
        let mut ci = claim();
        ci.repository = "attacker/repo".into();
        assert!(!compare(&ci, &runs, &jobs, &runs));
        let mut ci = claim();
        ci.jobs[1].job_id = ci.jobs[0].job_id;
        assert!(!compare(&ci, &runs, &jobs, &runs));
    }

    #[test]
    fn rejects_incomplete_or_ambiguous_pagination() {
        let ci = claim();
        let jobs = job_page(good_jobs(), 3);
        let truncated = run_page(vec![run(42, 2)], 2);
        assert!(!compare(&ci, &truncated, &jobs, &truncated));
        let first = run_page(
            (101..=200).rev().map(|id| run(id, 1)).collect(),
            PER_PAGE + 1,
        );
        assert!(!compare(&ci, &first, &jobs, &first));
        let duplicate = run_page(vec![run(42, 2), run(42, 2)], 2);
        assert!(!compare(&ci, &duplicate, &jobs, &duplicate));
        let runs = run_page(vec![run(42, 2)], 1);
        let job_count_wrong = job_page(good_jobs(), 4);
        assert!(!compare(&ci, &runs, &job_count_wrong, &runs));
        let bad_page = CiJsonPage {
            page_number: 2,
            body: &runs,
        };
        assert!(
            compare_ci_claim_to_independent_json(
                &ci,
                &[bad_page],
                &[CiJsonPage {
                    page_number: 1,
                    body: &jobs
                }],
                &[bad_page]
            )
            .is_err()
        );
    }

    #[test]
    fn accepts_complete_two_page_run_inventory_only_when_pages_are_ordered() {
        let mut ci = claim();
        ci.run_id = 200;
        ci.run_attempt = 1;
        let first = run_page((101..=200).rev().map(|id| run(id, 1)).collect(), 101);
        let second = run_page(vec![run(100, 1)], 101);
        let mut jobs = good_jobs();
        for job in &mut jobs {
            job["run_id"] = json!(200);
            job["run_attempt"] = json!(1);
        }
        let job_bytes = job_page(jobs, 3);
        let pages = [
            CiJsonPage {
                page_number: 1,
                body: &first,
            },
            CiJsonPage {
                page_number: 2,
                body: &second,
            },
        ];
        let job_pages = [CiJsonPage {
            page_number: 1,
            body: &job_bytes,
        }];
        assert!(compare_ci_claim_to_independent_json(&ci, &pages, &job_pages, &pages).is_ok());
        assert!(
            compare_ci_claim_to_independent_json(&ci, &pages[..1], &job_pages, &pages[..1])
                .is_err()
        );
        assert!(
            compare_ci_claim_to_independent_json(&ci, &[pages[1], pages[0]], &job_pages, &pages)
                .is_err()
        );
    }

    #[test]
    fn rejects_failed_duplicate_non_windows_or_wrong_attempt_jobs() {
        let runs = run_page(vec![run(42, 2)], 1);
        for (field, value) in [
            ("id", json!(999)),
            ("run_id", json!(41)),
            ("run_attempt", json!(1)),
            ("head_sha", json!("0".repeat(40))),
            ("name", json!("Other")),
            ("status", json!("in_progress")),
            ("conclusion", json!("failure")),
            ("labels", json!(["ubuntu-latest"])),
            ("labels", json!(["windows-latest", "ubuntu-latest"])),
        ] {
            let mut jobs = good_jobs();
            jobs[0][field] = value;
            let bytes = job_page(jobs, 3);
            assert!(!compare(&claim(), &runs, &bytes, &runs), "{field}");
        }
        let mut jobs = good_jobs();
        jobs[1] = jobs[0].clone();
        let bytes = job_page(jobs, 3);
        assert!(!compare(&claim(), &runs, &bytes, &runs));
    }

    #[test]
    fn rejects_snapshot_drift_malformed_json_and_oversized_page() {
        let first = run_page(vec![run(42, 2)], 1);
        let jobs = job_page(good_jobs(), 3);
        let second = run_page(vec![run(43, 1), run(42, 2)], 2);
        assert!(!compare(&claim(), &first, &jobs, &second));
        let mut rerun = run(42, 3);
        rerun["conclusion"] = json!("failure");
        let second = run_page(vec![rerun], 1);
        assert!(!compare(&claim(), &first, &jobs, &second));
        assert!(!compare(&claim(), b"{", &jobs, &first));
        let huge = vec![b' '; MAX_PAGE_BYTES + 1];
        assert!(!compare(&claim(), &huge, &jobs, &huge));
    }
}
