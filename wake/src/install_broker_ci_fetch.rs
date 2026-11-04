//! Broker-owned, fixed-origin exact-CI read before the first INTENT record.
//!
//! Only a policy-verified approval can reach the production entry point. The
//! request path is assembled from compiled repository/workflow identity and
//! the signed CI tuple. No candidate JSON, URL, header, credential or proxy
//! selection enters WinHTTP. All network errors deny acceptance.

#![cfg(windows)]

use crate::{
    Result,
    install_broker_approval::ExactCiV1,
    install_broker_ci_witness::{
        CiJsonPage, MAX_PAGE_BYTES, MAX_RUNS, PER_PAGE, compare_ci_claim_to_independent_json,
        validate_claim,
    },
    install_broker_policy::{PolicyCheckedOfflinePairV2, PolicyVerifiedApprovalV1},
};
use serde::Deserialize;
use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const API_HOST: &str = "api.github.com";
const API_PREFIX: &str = "/repos/vnath99/CatDesk";
const BRANCH_QUERY: &str = "orchestrator%2Fchatgpt-codex-autonomous-loop";
const WORKFLOW_ID: u64 = 311_012_404;
const TOTAL_DEADLINE: Duration = Duration::from_secs(30);
const PHASE_TIMEOUT_MS: i32 = 5_000;
const READ_CHUNK: usize = 8 * 1024;
const MAX_TOTAL_BYTES: usize = 7 * MAX_PAGE_BYTES;
const WINHTTP_ACCESS_TYPE_NO_PROXY: u32 = 1;
const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
const WINHTTP_OPTION_DISABLE_FEATURE: u32 = 63;
const WINHTTP_OPTION_ENABLE_FEATURE: u32 = 79;
const WINHTTP_OPTION_SECURE_PROTOCOLS: u32 = 84;
const WINHTTP_OPTION_REDIRECT_POLICY: u32 = 88;
const WINHTTP_OPTION_MAX_RESPONSE_HEADER_SIZE: u32 = 91;
const WINHTTP_DISABLE_COOKIES: u32 = 1;
const WINHTTP_DISABLE_REDIRECTS: u32 = 2;
const WINHTTP_DISABLE_AUTHENTICATION: u32 = 4;
const WINHTTP_ENABLE_SSL_REVOCATION: u32 = 1;
const WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2: u32 = 0x0000_0800;
const WINHTTP_REDIRECT_POLICY_NEVER: u32 = 0;
const WINHTTP_QUERY_STATUS_CODE: u32 = 19;
const WINHTTP_QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
const HTTPS_PORT: u16 = 443;
static NETWORK_WORKER_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

struct NetworkWorkerPermit;

impl NetworkWorkerPermit {
    fn acquire() -> Result<Self> {
        NETWORK_WORKER_IN_FLIGHT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "WAKE_BROKER_CI_NETWORK_BUSY".to_owned())?;
        Ok(Self)
    }
}

impl Drop for NetworkWorkerPermit {
    fn drop(&mut self) {
        NETWORK_WORKER_IN_FLIGHT.store(false, Ordering::Release);
    }
}

#[link(name = "winhttp")]
unsafe extern "system" {
    fn WinHttpOpen(
        agent: *const u16,
        access_type: u32,
        proxy: *const u16,
        proxy_bypass: *const u16,
        flags: u32,
    ) -> *mut c_void;
    fn WinHttpConnect(
        session: *mut c_void,
        server: *const u16,
        port: u16,
        reserved: u32,
    ) -> *mut c_void;
    fn WinHttpOpenRequest(
        connect: *mut c_void,
        verb: *const u16,
        object: *const u16,
        version: *const u16,
        referrer: *const u16,
        accept_types: *const *const u16,
        flags: u32,
    ) -> *mut c_void;
    fn WinHttpSetOption(handle: *mut c_void, option: u32, buffer: *const c_void, size: u32) -> i32;
    fn WinHttpSetTimeouts(
        handle: *mut c_void,
        resolve: i32,
        connect: i32,
        send: i32,
        receive: i32,
    ) -> i32;
    fn WinHttpSendRequest(
        request: *mut c_void,
        headers: *const u16,
        headers_length: u32,
        optional: *mut c_void,
        optional_length: u32,
        total_length: u32,
        context: usize,
    ) -> i32;
    fn WinHttpReceiveResponse(request: *mut c_void, reserved: *mut c_void) -> i32;
    fn WinHttpQueryHeaders(
        request: *mut c_void,
        info_level: u32,
        name: *const u16,
        buffer: *mut c_void,
        size: *mut u32,
        index: *mut u32,
    ) -> i32;
    fn WinHttpReadData(request: *mut c_void, buffer: *mut c_void, len: u32, read: *mut u32) -> i32;
    fn WinHttpCloseHandle(handle: *mut c_void) -> i32;
}

struct InternetHandle(*mut c_void);

impl InternetHandle {
    fn new(raw: *mut c_void) -> Result<Self> {
        if raw.is_null() {
            Err("WAKE_BROKER_CI_NETWORK_FAILED".into())
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for InternetHandle {
    fn drop(&mut self) {
        unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_u32_option(handle: *mut c_void, option: u32, value: u32) -> Result<()> {
    if unsafe {
        WinHttpSetOption(
            handle,
            option,
            (&raw const value).cast(),
            size_of::<u32>() as u32,
        )
    } == 0
    {
        return Err("WAKE_BROKER_CI_NETWORK_POLICY_FAILED".into());
    }
    Ok(())
}

fn remaining_ms(deadline: Instant) -> Result<i32> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err("WAKE_BROKER_CI_DEADLINE".into());
    }
    Ok(remaining.as_millis().clamp(1, PHASE_TIMEOUT_MS as u128) as i32)
}

fn set_remaining_timeouts(handle: *mut c_void, deadline: Instant) -> Result<()> {
    let ms = remaining_ms(deadline)?;
    if unsafe { WinHttpSetTimeouts(handle, ms, ms, ms, ms) } == 0 {
        return Err("WAKE_BROKER_CI_NETWORK_POLICY_FAILED".into());
    }
    Ok(())
}

struct HttpReply {
    status: u32,
    body: Vec<u8>,
}

trait CiTransport {
    fn get(&mut self, object: &str, deadline: Instant) -> Result<HttpReply>;
}

struct FixedWinHttp {
    _session: InternetHandle,
    connect: InternetHandle,
}

impl FixedWinHttp {
    fn open(deadline: Instant) -> Result<Self> {
        remaining_ms(deadline)?;
        let agent = wide("CatDesk-WakeInstallBroker-CI/1");
        let session = InternetHandle::new(unsafe {
            WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                ptr::null(),
                ptr::null(),
                0,
            )
        })?;
        set_u32_option(
            session.0,
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2,
        )?;
        set_remaining_timeouts(session.0, deadline)?;
        let host = wide(API_HOST);
        let connect = InternetHandle::new(unsafe {
            WinHttpConnect(session.0, host.as_ptr(), HTTPS_PORT, 0)
        })?;
        Ok(Self {
            _session: session,
            connect,
        })
    }
}

impl CiTransport for FixedWinHttp {
    fn get(&mut self, object: &str, deadline: Instant) -> Result<HttpReply> {
        remaining_ms(deadline)?;
        // `object` is private to the fetch sequence and consists only of the
        // compiled path and validated fields from the signed CI tuple.
        let verb = wide("GET");
        let object = wide(object);
        let request = InternetHandle::new(unsafe {
            WinHttpOpenRequest(
                self.connect.0,
                verb.as_ptr(),
                object.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        })?;
        set_u32_option(
            request.0,
            WINHTTP_OPTION_DISABLE_FEATURE,
            WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_AUTHENTICATION,
        )?;
        set_u32_option(
            request.0,
            WINHTTP_OPTION_REDIRECT_POLICY,
            WINHTTP_REDIRECT_POLICY_NEVER,
        )?;
        set_u32_option(
            request.0,
            WINHTTP_OPTION_ENABLE_FEATURE,
            WINHTTP_ENABLE_SSL_REVOCATION,
        )?;
        set_u32_option(
            request.0,
            WINHTTP_OPTION_MAX_RESPONSE_HEADER_SIZE,
            16 * 1024,
        )?;
        set_remaining_timeouts(request.0, deadline)?;
        let headers =
            wide("Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n");
        if unsafe {
            WinHttpSendRequest(
                request.0,
                headers.as_ptr(),
                (headers.len() - 1) as u32,
                ptr::null_mut(),
                0,
                0,
                0,
            )
        } == 0
        {
            return Err("WAKE_BROKER_CI_NETWORK_FAILED".into());
        }
        set_remaining_timeouts(request.0, deadline)?;
        if unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) } == 0 {
            return Err("WAKE_BROKER_CI_NETWORK_FAILED".into());
        }
        let mut status = 0u32;
        let mut status_size = size_of::<u32>() as u32;
        if unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                ptr::null(),
                (&raw mut status).cast(),
                &raw mut status_size,
                ptr::null_mut(),
            )
        } == 0
            || status_size != size_of::<u32>() as u32
        {
            return Err("WAKE_BROKER_CI_NETWORK_FAILED".into());
        }
        if status != 200 {
            return Ok(HttpReply {
                status,
                body: Vec::new(),
            });
        }
        let mut body = Vec::new();
        let mut chunk = [0u8; READ_CHUNK];
        loop {
            set_remaining_timeouts(request.0, deadline)?;
            let wanted = (MAX_PAGE_BYTES + 1 - body.len()).min(chunk.len());
            let mut count = 0u32;
            if unsafe {
                WinHttpReadData(
                    request.0,
                    chunk.as_mut_ptr().cast(),
                    wanted as u32,
                    &raw mut count,
                )
            } == 0
            {
                return Err("WAKE_BROKER_CI_NETWORK_FAILED".into());
            }
            if count == 0 {
                break;
            }
            if count as usize > wanted {
                return Err("WAKE_BROKER_CI_NETWORK_FAILED".into());
            }
            body.extend_from_slice(&chunk[..count as usize]);
            if body.len() > MAX_PAGE_BYTES {
                return Err("WAKE_BROKER_CI_PAGE_TOO_LARGE".into());
            }
        }
        Ok(HttpReply { status, body })
    }
}

#[derive(Deserialize)]
struct CountOnly {
    total_count: u64,
}

fn run_path(ci: &ExactCiV1, page: usize) -> String {
    format!(
        "{API_PREFIX}/actions/workflows/{WORKFLOW_ID}/runs?branch={BRANCH_QUERY}&event=push&head_sha={}&per_page={PER_PAGE}&page={page}",
        ci.head_sha
    )
}

fn jobs_path(ci: &ExactCiV1) -> String {
    format!(
        "{API_PREFIX}/actions/runs/{}/attempts/{}/jobs?per_page={PER_PAGE}&page=1",
        ci.run_id, ci.run_attempt
    )
}

fn page<T: CiTransport>(
    transport: &mut T,
    path: &str,
    deadline: Instant,
    remaining_bytes: &mut usize,
) -> Result<Vec<u8>> {
    remaining_ms(deadline)?;
    let reply = transport.get(path, deadline)?;
    if reply.status != 200 {
        return Err("WAKE_BROKER_CI_HTTP_STATUS".into());
    }
    if reply.body.is_empty() || reply.body.len() > MAX_PAGE_BYTES {
        return Err("WAKE_BROKER_CI_PAGE_SIZE".into());
    }
    *remaining_bytes = remaining_bytes
        .checked_sub(reply.body.len())
        .ok_or("WAKE_BROKER_CI_TOTAL_SIZE")?;
    Ok(reply.body)
}

fn page_count(first: &[u8], limit: usize) -> Result<usize> {
    let count: CountOnly =
        serde_json::from_slice(first).map_err(|_| "WAKE_BROKER_CI_JSON_INVALID")?;
    let total = usize::try_from(count.total_count).map_err(|_| "WAKE_BROKER_CI_COUNT_INVALID")?;
    if total > limit {
        return Err("WAKE_BROKER_CI_COUNT_INVALID".into());
    }
    Ok(total.max(1).div_ceil(PER_PAGE))
}

fn run_pages<T: CiTransport>(
    transport: &mut T,
    ci: &ExactCiV1,
    deadline: Instant,
    remaining_bytes: &mut usize,
) -> Result<Vec<Vec<u8>>> {
    let first = page(transport, &run_path(ci, 1), deadline, remaining_bytes)?;
    let count = page_count(&first, MAX_RUNS)?;
    let mut result = vec![first];
    for number in 2..=count {
        result.push(page(
            transport,
            &run_path(ci, number),
            deadline,
            remaining_bytes,
        )?);
    }
    Ok(result)
}

fn numbered_pages(bodies: &[Vec<u8>]) -> Vec<CiJsonPage<'_>> {
    bodies
        .iter()
        .enumerate()
        .map(|(index, body)| CiJsonPage {
            page_number: (index + 1) as u32,
            body,
        })
        .collect()
}

fn fetch_with<T: CiTransport>(ci: &ExactCiV1, transport: &mut T, deadline: Instant) -> Result<()> {
    validate_claim(ci).map_err(str::to_owned)?;
    let mut remaining_bytes = MAX_TOTAL_BYTES;
    let before = run_pages(transport, ci, deadline, &mut remaining_bytes)?;
    let jobs = page(transport, &jobs_path(ci), deadline, &mut remaining_bytes)?;
    if page_count(&jobs, 3)? != 1 {
        return Err("WAKE_BROKER_CI_COUNT_INVALID".into());
    }
    let after = run_pages(transport, ci, deadline, &mut remaining_bytes)?;
    compare_ci_claim_to_independent_json(
        ci,
        &numbered_pages(&before),
        &[CiJsonPage {
            page_number: 1,
            body: &jobs,
        }],
        &numbered_pages(&after),
    )
    .map_err(str::to_owned)
}

/// Called only from protected ledger acceptance after the envelope has been
/// verified with protected policy and pinned observations. The network worker
/// owns a copy of the signed CI claim and no ledger handle. A total deadline
/// releases the ledger lock even if a platform network call stalls; late work
/// cannot write an INTENT or acquire install authority. WinHTTP may outlive the
/// wait despite its per-phase timeouts, so a process-wide permit prevents a
/// second worker until the first exits. The current service is one-shot and
/// exits after its refused submission; this function does not assume a stalled
/// native call can be canceled or joined safely.
pub(crate) fn require_broker_owned_exact_ci(approved: &PolicyVerifiedApprovalV1<'_>) -> Result<()> {
    require_exact_claim(approved.verified().payload().ci.clone())
}

/// Reuses the same fixed origin, deadline and independent comparison for V2.
/// A supplied JSON CI tuple cannot call this protected-policy entry point.
pub(crate) fn require_broker_owned_offline_exact_ci(
    checked: &PolicyCheckedOfflinePairV2<'_>,
) -> Result<()> {
    checked.revalidate()?;
    require_exact_claim(checked.checked().approval().ci.clone())?;
    checked.revalidate()
}

fn require_exact_claim(ci: ExactCiV1) -> Result<()> {
    validate_claim(&ci).map_err(str::to_owned)?;
    let permit = NetworkWorkerPermit::acquire()?;
    let deadline = Instant::now() + TOTAL_DEADLINE;
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("wake-broker-exact-ci".into())
        .spawn(move || {
            let _permit = permit;
            let result = FixedWinHttp::open(deadline)
                .and_then(|mut transport| fetch_with(&ci, &mut transport, deadline));
            let _ = sender.send(result);
        })
        .map_err(|_| "WAKE_BROKER_CI_NETWORK_FAILED")?;
    let wait = deadline.saturating_duration_since(Instant::now());
    receiver
        .recv_timeout(wait)
        .map_err(|_| "WAKE_BROKER_CI_DEADLINE".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_broker_approval::tests::fixture;
    use serde_json::{Value, json};
    use std::collections::VecDeque;

    struct MockTransport {
        replies: VecDeque<(String, HttpReply)>,
    }

    impl CiTransport for MockTransport {
        fn get(&mut self, object: &str, _deadline: Instant) -> Result<HttpReply> {
            let (expected, reply) = self.replies.pop_front().expect("unexpected request");
            assert_eq!(object, expected);
            Ok(reply)
        }
    }

    fn reply(value: Value) -> HttpReply {
        HttpReply {
            status: 200,
            body: serde_json::to_vec(&value).expect("JSON fixture"),
        }
    }

    fn run(ci: &ExactCiV1, id: u64) -> Value {
        json!({
            "id": id,
            "workflow_id": ci.workflow_id,
            "path": ci.workflow_path,
            "repository": {"full_name":ci.repository},
            "head_repository": {"full_name":ci.repository},
            "head_branch":ci.branch,
            "head_sha":ci.head_sha,
            "event":"push",
            "status":"completed",
            "conclusion":"success",
            "run_attempt":ci.run_attempt,
            "run_number":1
        })
    }

    fn runs(ci: &ExactCiV1) -> Value {
        json!({"total_count":1,"workflow_runs":[run(ci,ci.run_id)]})
    }

    fn jobs(ci: &ExactCiV1) -> Value {
        json!({"total_count":3,"jobs":ci.jobs.iter().map(|job|json!({
            "id":job.job_id,
            "run_id":ci.run_id,
            "run_attempt":ci.run_attempt,
            "head_sha":ci.head_sha,
            "name":job.name,
            "status":"completed",
            "conclusion":job.conclusion,
            "labels":["windows-latest"]
        })).collect::<Vec<_>>()})
    }

    fn good(ci: &ExactCiV1) -> MockTransport {
        MockTransport {
            replies: VecDeque::from([
                (run_path(ci, 1), reply(runs(ci))),
                (jobs_path(ci), reply(jobs(ci))),
                (run_path(ci, 1), reply(runs(ci))),
            ]),
        }
    }

    fn check(ci: &ExactCiV1, mock: &mut MockTransport) -> Result<()> {
        fetch_with(ci, mock, Instant::now() + Duration::from_secs(1))
    }

    #[test]
    fn exact_fixed_sequence_accepts_signed_tuple() {
        let ci = fixture().0.ci;
        let mut mock = good(&ci);
        check(&ci, &mut mock).expect("exact CI");
        assert!(mock.replies.is_empty());
        assert!(
            run_path(&ci, 1)
                .starts_with("/repos/vnath99/CatDesk/actions/workflows/311012404/runs?")
        );
        assert!(run_path(&ci, 1).contains("branch=orchestrator%2Fchatgpt-codex-autonomous-loop"));
    }

    #[test]
    fn complete_two_page_run_inventory_is_fetched_twice() {
        let ci = fixture().0.ci;
        let mut all = vec![run(&ci, ci.run_id)];
        all.extend((1..=100).map(|id| run(&ci, id)));
        let first = reply(json!({"total_count":101,"workflow_runs":all[..100]}));
        let second = reply(json!({"total_count":101,"workflow_runs":all[100..]}));
        let mut mock = MockTransport {
            replies: VecDeque::from([
                (
                    run_path(&ci, 1),
                    HttpReply {
                        status: first.status,
                        body: first.body.clone(),
                    },
                ),
                (
                    run_path(&ci, 2),
                    HttpReply {
                        status: second.status,
                        body: second.body.clone(),
                    },
                ),
                (jobs_path(&ci), reply(jobs(&ci))),
                (run_path(&ci, 1), first),
                (run_path(&ci, 2), second),
            ]),
        };
        check(&ci, &mut mock).expect("complete two-page CI inventory");
        assert!(mock.replies.is_empty());
    }

    #[test]
    fn invalid_signed_tuple_never_reaches_network() {
        let mut ci = fixture().0.ci;
        ci.head_sha = "../../other".into();
        let mut mock = MockTransport {
            replies: VecDeque::new(),
        };
        assert!(check(&ci, &mut mock).is_err());
    }

    #[test]
    fn non_200_and_oversized_pages_refuse_before_next_request() {
        let ci = fixture().0.ci;
        for status in [301, 302, 401, 403, 404, 429, 500] {
            let mut mock = good(&ci);
            mock.replies.front_mut().unwrap().1.status = status;
            assert_eq!(
                check(&ci, &mut mock).unwrap_err(),
                "WAKE_BROKER_CI_HTTP_STATUS"
            );
            assert_eq!(mock.replies.len(), 2);
        }
        let mut mock = good(&ci);
        mock.replies.front_mut().unwrap().1.body = vec![b'x'; MAX_PAGE_BYTES + 1];
        assert_eq!(
            check(&ci, &mut mock).unwrap_err(),
            "WAKE_BROKER_CI_PAGE_SIZE"
        );
    }

    #[test]
    fn incomplete_or_changed_inventory_refuses() {
        let ci = fixture().0.ci;
        let mut mock = good(&ci);
        mock.replies.front_mut().unwrap().1.body =
            serde_json::to_vec(&json!({"total_count":301,"workflow_runs":[]})).unwrap();
        assert_eq!(
            check(&ci, &mut mock).unwrap_err(),
            "WAKE_BROKER_CI_COUNT_INVALID"
        );

        let mut mock = good(&ci);
        mock.replies.back_mut().unwrap().1.body =
            serde_json::to_vec(&json!({"total_count":0,"workflow_runs":[]})).unwrap();
        assert!(check(&ci, &mut mock).is_err());

        let mut mock = good(&ci);
        mock.replies.get_mut(1).unwrap().1.body =
            serde_json::to_vec(&json!({"total_count":2,"jobs":[]})).unwrap();
        assert!(check(&ci, &mut mock).is_err());
    }

    #[test]
    fn expired_deadline_refuses_without_request() {
        let ci = fixture().0.ci;
        let mut mock = good(&ci);
        assert_eq!(
            fetch_with(&ci, &mut mock, Instant::now() - Duration::from_secs(1)).unwrap_err(),
            "WAKE_BROKER_CI_DEADLINE"
        );
        assert_eq!(mock.replies.len(), 3);
    }

    #[test]
    fn only_one_network_worker_can_be_in_flight() {
        let first = NetworkWorkerPermit::acquire().expect("first worker");
        assert_eq!(
            NetworkWorkerPermit::acquire().err().unwrap(),
            "WAKE_BROKER_CI_NETWORK_BUSY"
        );
        drop(first);
        let second = NetworkWorkerPermit::acquire().expect("released worker");
        drop(second);
    }

    #[test]
    #[ignore = "one-off read-only WinHTTP probe of an older green run, never current-source acceptance"]
    fn older_known_green_run_exercises_real_winhttp_transport() {
        let mut ci = fixture().0.ci;
        ci.head_sha = "4e9a2e212c47b0b7229b41874a3b1ec5bfd7d083".into();
        ci.run_id = 38_646_210_548;
        ci.jobs[0].job_id = 116_133_577_772;
        ci.jobs[1].job_id = 116_133_578_007;
        ci.jobs[2].job_id = 116_133_578_237;
        let deadline = Instant::now() + TOTAL_DEADLINE;
        let mut transport = FixedWinHttp::open(deadline).expect("WinHTTP fixed origin");
        fetch_with(&ci, &mut transport, deadline).expect("older read-only exact CI probe");
    }
}
