//! Explicit stopped-only retirement of an audited failed promotion.
//! Original authority and handoff bytes are moved, never erased or rewritten.
use super::*;
use std::collections::BTreeMap;

const LABEL: &str = "stopped promotion retirement";
const JOURNAL: &str = "promotion-retirement.json";
const RECORDS: [&str; 6] = [
    "authorization.json",
    "claim.json",
    "preflight.json",
    "result.json",
    "reviewed-build-attestation.json",
    "worker-owner",
];

pub(crate) struct MutationLease {
    _namespace: Guard,
    _lock: crate::windows_protected_fs::DeleteOnCloseRegularLock,
}

pub(crate) fn mutation_lease(workspace: &Path) -> Result<MutationLease, String> {
    let parent = metadata(workspace)?;
    let lock = create_delete_on_close_regular_lock(&parent, "promotion-operation.lock", LABEL)?;
    Ok(MutationLease {
        _namespace: parent,
        _lock: lock,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Plan {
    schema_version: u8,
    transaction_id: String,
    prior_sha256: String,
    candidate_sha256: String,
    records: BTreeMap<String, String>,
    historical_handoff_sha256: Option<String>,
    trusted_release_evidence_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema_version: u8,
    phase: String,
    evidence_sha256: String,
    plan: Plan,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|_| format!("{LABEL} serialization failed"))
}
fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|v| v.is_ascii_hexdigit() && !v.is_ascii_uppercase())
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|v| v.is_ascii_hexdigit() && !v.is_ascii_uppercase())
}
fn archive_name(id: &str) -> Result<String, String> {
    if !valid_id(id) {
        return Err(format!("{LABEL} transaction is invalid"));
    }
    Ok(format!("promotion-retired-{id}"))
}

#[cfg(windows)]
use crate::windows_protected_fs::{
    ProtectedDirectoryGuard as Guard, create_delete_on_close_regular_lock,
    read_optional_relative_regular, read_relative_regular, rename_exact_regular_child,
    write_new_regular_in, write_unique_regular_for_atomic_replace,
};

#[cfg(windows)]
fn read_json<T: serde::de::DeserializeOwned>(parent: &Guard, name: &str) -> Result<T, String> {
    serde_json::from_slice(&read_relative_regular(parent, name, 65536, LABEL)?)
        .map_err(|_| format!("{LABEL} record is invalid"))
}

#[cfg(windows)]
fn metadata(workspace: &Path) -> Result<Guard, String> {
    let root = crate::command::normalize_windows_verbatim_path(
        workspace
            .canonicalize()
            .map_err(|_| format!("{LABEL} workspace unavailable"))?,
    );
    let mut guard = Guard::acquire(&root, LABEL)?;
    guard.descend_existing(".catdesk", LABEL)?;
    Ok(guard)
}

#[cfg(windows)]
fn child(parent: &Guard, name: &str) -> Result<Guard, String> {
    let mut guard = parent.try_clone(LABEL)?;
    guard.descend_existing(name, LABEL)?;
    Ok(guard)
}

#[cfg(windows)]
fn optional_child(parent: &Guard, name: &str) -> Result<Option<Guard>, String> {
    let mut guard = parent.try_clone(LABEL)?;
    if guard.descend_optional_existing(name, LABEL)? {
        Ok(Some(guard))
    } else {
        Ok(None)
    }
}

#[cfg(windows)]
fn journal(parent: &Guard) -> Result<Option<Journal>, String> {
    let Some(raw) = read_optional_relative_regular(parent, JOURNAL, 65536, LABEL)? else {
        return Ok(None);
    };
    let value: Journal =
        serde_json::from_slice(&raw).map_err(|_| format!("{LABEL} journal invalid"))?;
    if value.schema_version != 1
        || value.plan.schema_version != 1
        || !valid_id(&value.plan.transaction_id)
        || !valid_digest(&value.evidence_sha256)
        || digest(&bytes(&value.plan)?) != value.evidence_sha256
        || !matches!(
            value.phase.as_str(),
            "PREPARED" | "PROMOTION_ARCHIVED" | "COMPLETED"
        )
    {
        return Err(format!("{LABEL} journal differs"));
    }
    Ok(Some(value))
}

#[cfg(windows)]
fn update_journal(parent: &Guard, value: &Journal) -> Result<(), String> {
    let encoded = bytes(value)?;
    if read_optional_relative_regular(parent, JOURNAL, 65536, LABEL)?.is_none() {
        write_new_regular_in(parent, JOURNAL, &encoded, LABEL)
    } else {
        write_unique_regular_for_atomic_replace(parent, &encoded, LABEL)?
            .commit_replace(JOURNAL, LABEL)
    }
}

#[cfg(windows)]
fn record_hashes(control: &Guard) -> Result<BTreeMap<String, String>, String> {
    let mut names = fs::read_dir(control.path())
        .map_err(|_| format!("{LABEL} inventory unavailable"))?
        .map(|entry| entry.map(|v| v.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("{LABEL} inventory unavailable"))?;
    names.sort();
    if names != RECORDS {
        return Err(format!("{LABEL} unexpected control records"));
    }
    RECORDS
        .into_iter()
        .map(|name| {
            Ok((
                name.into(),
                digest(&read_relative_regular(control, name, 16384, LABEL)?),
            ))
        })
        .collect()
}

#[cfg(windows)]
fn verify_archive(parent: &Guard, value: &Journal) -> Result<(), String> {
    let archived = child(parent, &archive_name(&value.plan.transaction_id)?)?;
    if record_hashes(&archived)? != value.plan.records {
        return Err(format!("{LABEL} archive drifted"));
    }
    if let Some(expected) = &value.plan.historical_handoff_sha256 {
        let handoffs = child(parent, "restart-handoff")?;
        if digest(&read_relative_regular(
            &handoffs,
            &format!("retired-{expected}.json"),
            4096,
            LABEL,
        )?) != *expected
        {
            return Err(format!("{LABEL} historical handoff differs"));
        }
    }
    Ok(())
}

/// A pending retirement fences new mutation; status remains readable.
#[cfg(windows)]
pub(crate) fn ensure_ready(workspace: &Path) -> Result<(), String> {
    let parent = metadata(workspace)?;
    if let Some(value) = journal(&parent)? {
        if value.phase != "COMPLETED" {
            return Err(format!("{LABEL} requires explicit resume"));
        }
        verify_archive(&parent, &value)?;
    }
    Ok(())
}
#[cfg(not(windows))]
pub(crate) fn ensure_ready(_workspace: &Path) -> Result<(), String> {
    Ok(())
}

// Fixed read-only code. Workspace and image paths enter as JSON on stdin, never
// as interpolated PowerShell source. The existing LKG authority verifier is reused.
#[cfg(windows)]
const INSPECTION: &str = r#"
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; [Console]::InputEncoding=[Text.UTF8Encoding]::new($false)
try {
 $i=[Console]::In.ReadToEnd()|ConvertFrom-Json
 . (Join-Path $i.workspace 'scripts/catdesk-release-recovery.ps1')
 $pair=Get-CatDeskCanonicalPairEvidence $i.workspace
 $trusted=Get-CatDeskLastKnownGoodRelease $i.workspace
 if(!$trusted -or $trusted.Hash -cne $pair.Hash -or $pair.Hash -cne $i.priorSha256){throw 'untrusted'}
 if($i.bridgePid){
  $p=Get-Process -Id $i.bridgePid -ErrorAction Stop
  if(!$p.Path -or [IO.Path]::GetFullPath($p.Path) -ine [IO.Path]::GetFullPath($pair.Binary)){throw 'bridge image'}
  $listeners=@(Get-NetTCPConnection -State Listen -ErrorAction Stop|Where-Object LocalPort -eq 3200)
  if(!$listeners.Count -or @($listeners|Where-Object {$_.OwningProcess -ne $i.bridgePid -or $_.LocalAddress -notin @('127.0.0.1','::1')}).Count){throw 'bridge listener'}
  @{canonicalSha256=$pair.Hash;bridgePid=$i.bridgePid;listenerVerified=$true}|ConvertTo-Json -Compress
  exit 0
 }
 if(@(Get-NetTCPConnection -State Listen -ErrorAction Stop|Where-Object LocalPort -eq 3200).Count){throw 'listener'}
 $canonical=[IO.Path]::GetFullPath($pair.Binary); $candidate=[IO.Path]::GetFullPath($i.candidate)
 foreach($p in @(Get-CimInstance Win32_Process -Filter "Name='catdesk.exe' OR Name='CatDeskBinagotchy.exe'" -OperationTimeoutSec 5 -ErrorAction Stop)) {
  if(!$p.ExecutablePath -or !$p.CommandLine){throw 'unknown image'}
  $image=[IO.Path]::GetFullPath($p.ExecutablePath)
  if($image -ieq $canonical -or $image -ieq $candidate){throw 'live image'}
  if($p.CommandLine.Contains('--catdesk-reviewed-promotion-worker') -and $p.CommandLine.Contains($i.transactionId)){
   if(!$i.ownedWorkerPid -or $p.ProcessId -ne $i.ownedWorkerPid -or $image -ine [IO.Path]::GetFullPath($i.ownedWorkerImage)){throw 'live promotion worker'}
  }
 }
 foreach($pidValue in $i.historicalPids){
  $p=Get-Process -Id $pidValue -ErrorAction SilentlyContinue
  if($p -and $p.StartTime.ToUniversalTime() -le ([DateTimeOffset]::FromUnixTimeSeconds($i.historicalCompletedUnix).UtcDateTime.AddSeconds(2))){throw 'historical owner'}
 }
 $original=$null; $qualified=$null
 if($i.historicalPids.Count){
  [void](Get-CatDeskQualifiedStoppedAuthority $i.workspace $pair.Hash)
  $authority=ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText((Join-Path $i.workspace '.catdesk/release-pair-control/authorization.json')))
  $q=ConvertFrom-CatDeskLiteralJson ([IO.File]::ReadAllText((Join-Path $i.workspace '.catdesk/release-pair-control/qualification.json')))
  $original=$authority.authorization.originalBinarySha256; $qualified=$q.qualifiedAtUnix
 }
 @{canonicalSha256=$pair.Hash;slot=$trusted.Slot;generation=$trusted.Generation;metadataSha256=Get-CatDeskRecoverySha256 $trusted.Metadata;originalSha256=$original;qualifiedUnix=$qualified;ownersEmpty=$true}|ConvertTo-Json -Compress
} catch { [Console]::Error.Write('PROMOTION_RETIREMENT_INSPECTION_REFUSED'); exit 2 }
"#;

#[cfg(windows)]
fn inspect_owners(
    workspace: &Path,
    auth: &ReviewedPromotionAuthorizationV1,
    handoff: Option<&ReloadHandoffStateV1>,
    helper_hash: &str,
) -> Result<serde_json::Value, String> {
    inspect_owners_context(workspace, auth, handoff, helper_hash, false, None)
}

fn inspect_owners_context(
    workspace: &Path,
    auth: &ReviewedPromotionAuthorizationV1,
    handoff: Option<&ReloadHandoffStateV1>,
    helper_hash: &str,
    owned_worker: bool,
    bridge_pid: Option<u32>,
) -> Result<serde_json::Value, String> {
    use crate::reviewed_candidate_owner::OwnedCandidateChild;
    let interpreter = crate::operator_facade::trusted_windows_powershell(workspace)?;
    let mut scripts = Guard::acquire(workspace, LABEL)?;
    scripts.descend_existing("scripts", LABEL)?;
    let helper = "catdesk-release-recovery.ps1";
    let _helper = crate::windows_protected_fs::retain_reviewed_regular_artifact(
        &scripts,
        helper,
        helper_hash,
        131072,
        LABEL,
    )?;
    let mut command = Command::new(interpreter);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            INSPECTION,
        ])
        .env_remove("PSModulePath")
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut owner = OwnedCandidateChild::spawn(&mut command)?;
    let input = serde_json::json!({"workspace":crate::command::normalize_windows_verbatim_path(workspace.to_path_buf()),
        "candidate":crate::command::normalize_windows_verbatim_path(workspace.join(&auth.candidate_relative_path)),
        "priorSha256":auth.prior_canonical_sha256,"transactionId":auth.transaction_id,"historicalPids":handoff.map(|v|vec![v.old_pid,v.new_pid.unwrap_or(0)]).unwrap_or_default(),
        "historicalCompletedUnix":handoff.map(|v|v.completed_at_unix).unwrap_or(0),
        "bridgePid":bridge_pid,"ownedWorkerPid":owned_worker.then_some(std::process::id()),
        "ownedWorkerImage":if owned_worker {Some(crate::command::normalize_windows_verbatim_path(std::env::current_exe().map_err(|_|"promotion worker image unavailable")?))}else{None}});
    owner
        .child_mut()
        .stdin
        .take()
        .ok_or("retirement input unavailable")?
        .write_all(&bytes(&input)?)
        .map_err(|_| "retirement input failed")?;
    let out = owner
        .child_mut()
        .stdout
        .take()
        .ok_or("retirement output unavailable")?;
    let err = owner
        .child_mut()
        .stderr
        .take()
        .ok_or("retirement error unavailable")?;
    let out = thread::spawn(move || {
        let mut bytes = Vec::new();
        out.take(4097).read_to_end(&mut bytes).map(|_| bytes)
    });
    let err = thread::spawn(move || {
        let mut bytes = Vec::new();
        err.take(4097).read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = Instant::now();
    let mut success = false;
    while started.elapsed() < Duration::from_secs(20) {
        if let Some(status) = owner
            .child_mut()
            .try_wait()
            .map_err(|_| "retirement inspection wait failed")?
        {
            success = status.success();
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    owner.terminate_tree_and_confirm_quiescent()?;
    let out = out
        .join()
        .map_err(|_| "retirement output reader failed")?
        .map_err(|_| "retirement output failed")?;
    let err = err
        .join()
        .map_err(|_| "retirement error reader failed")?
        .map_err(|_| "retirement error failed")?;
    if !success || out.len() > 4096 || !err.is_empty() {
        return Err(format!("{LABEL} ownership or trust unproven"));
    }
    let proof: serde_json::Value =
        serde_json::from_slice(&out).map_err(|_| format!("{LABEL} proof invalid"))?;
    let scope_matches = match bridge_pid {
        Some(pid) => proof["listenerVerified"] == true && proof["bridgePid"] == pid,
        None => proof["ownersEmpty"] == true,
    };
    if !scope_matches || proof["canonicalSha256"] != auth.prior_canonical_sha256 {
        return Err(format!("{LABEL} ownership proof differs"));
    }
    Ok(proof)
}

fn accepted_helper(
    workspace: &Path,
    auth: &ReviewedPromotionAuthorizationV1,
) -> Result<String, String> {
    validate_committed_snapshot(workspace, &auth.snapshot_expected)?
        .entries
        .into_iter()
        .find(|v| v.relative_path == "scripts/catdesk-release-recovery.ps1")
        .map(|v| v.sha256)
        .ok_or_else(|| "accepted recovery helper unavailable".into())
}

pub(super) fn verify_bridge_listener(
    workspace: &Path,
    auth: &ReviewedPromotionAuthorizationV1,
    pid: u32,
) -> Result<(), String> {
    inspect_owners_context(
        workspace,
        auth,
        None,
        &accepted_helper(workspace, auth)?,
        false,
        Some(pid),
    )
    .map(|_| ())
}

pub(super) fn admit_stopped_source(
    workspace: &Path,
    auth: &ReviewedPromotionAuthorizationV1,
) -> Result<(), String> {
    let parent = metadata(workspace)?;
    let j = journal(&parent)?.ok_or("promotion source requires completed explicit retirement")?;
    if j.phase != "COMPLETED"
        || j.plan.prior_sha256 != auth.prior_canonical_sha256
        || j.plan.candidate_sha256 != auth.candidate_sha256
    {
        return Err("promotion source retirement binding differs".into());
    }
    verify_archive(&parent, &j)?;
    let handoffs = child(&parent, "restart-handoff")?;
    if read_optional_relative_regular(&handoffs, "latest.json", 4096, LABEL)?.is_some() {
        return Err("promotion source has an unresolved new handoff".into());
    }
    inspect_owners_context(
        workspace,
        auth,
        None,
        &accepted_helper(workspace, auth)?,
        true,
        None,
    )
    .map(|_| ())
}

pub(crate) fn qualify_stopped_source(
    workspace: &Path,
    transaction: &str,
    expected: &str,
) -> Result<serde_json::Value, String> {
    let parent = metadata(workspace)?;
    let _lock = create_delete_on_close_regular_lock(&parent, "promotion-operation.lock", LABEL)?;
    let _lifecycle = super::source_bridge::lifecycle_lease(workspace)?;
    let previous = journal(&parent)?;
    let plan = inspect_locked(workspace, &parent, transaction, previous.as_ref())?;
    if digest(&bytes(&plan)?) != expected {
        return Err("promotion source evidence differs".into());
    }
    let control = child(&parent, "promotion-control")?;
    let auth: ReviewedPromotionAuthorizationV1 = read_json(&control, "authorization.json")?;
    super::source_bridge::qualify(workspace, &auth)
}

#[cfg(windows)]
fn validate_terminal_binding(
    auth: &ReviewedPromotionAuthorizationV1,
    preflight: &ReviewedPromotionPreflightV1,
    claim: &ReviewedPromotionClaimV1,
    result: &ReviewedPromotionResultV1,
    owner: &[u8],
    transaction: &str,
) -> Result<(), String> {
    if auth.schema_version != 1
        || !auth.local_backend_only
        || auth.transaction_id != transaction
        || !valid_id(transaction)
        || auth.expires_at_unix >= now_unix()
        || preflight.schema_version != 1
        || preflight.expires_at_unix != auth.expires_at_unix
        || preflight.authorization != *auth
        || claim.schema_version != 1
        || claim.state != "CLAIMED_PENDING"
        || claim.transaction_id != transaction
        || claim.authorization_id != auth.authorization_id
        || claim.claim_id != auth.claim_id
        || claim.candidate_relative_path != auth.candidate_relative_path
        || result.schema_version != 1
        || result.state != "PROMOTION_FAILED_OR_AMBIGUOUS"
        || result.transaction_id != transaction
        || result.authorization_generation != auth.generation
        || result.candidate_sha256 != auth.candidate_sha256
        || result.serving_state.is_some()
        || owner != transaction.as_bytes()
        || !valid_digest(&auth.prior_canonical_sha256)
        || auth.prior_canonical_sha256 == auth.candidate_sha256
    {
        return Err(format!(
            "{LABEL} exact terminal pre-mutation evidence required"
        ));
    }
    Ok(())
}

fn inspect_locked(
    workspace: &Path,
    parent: &Guard,
    transaction: &str,
    prior: Option<&Journal>,
) -> Result<Plan, String> {
    let archived = archive_name(transaction)?;
    let control =
        if prior.is_some_and(|v| v.plan.transaction_id == transaction && v.phase != "COMPLETED") {
            match (
                optional_child(parent, "promotion-control")?,
                optional_child(parent, &archived)?,
            ) {
                (Some(current), None) => current,
                (None, Some(old)) => old,
                _ => return Err(format!("{LABEL} recovery location ambiguous")),
            }
        } else {
            child(parent, "promotion-control")?
        };
    let hashes = record_hashes(&control)?;
    let auth: ReviewedPromotionAuthorizationV1 = read_json(&control, "authorization.json")?;
    let claim: ReviewedPromotionClaimV1 = read_json(&control, "claim.json")?;
    let result: ReviewedPromotionResultV1 = read_json(&control, "result.json")?;
    let preflight: ReviewedPromotionPreflightV1 = read_json(&control, "preflight.json")?;
    validate_terminal_binding(
        &auth,
        &preflight,
        &claim,
        &result,
        &read_relative_regular(&control, "worker-owner", 32, LABEL)?,
        transaction,
    )?;
    if workspace
        .join(".catdesk/promotion-recovery/promotion-transaction.json")
        .try_exists()
        .map_err(|_| "promotion transaction unavailable")?
    {
        return Err(format!(
            "{LABEL} disk transaction requires its original recovery"
        ));
    }
    let attestation = validate_producer_attestation(
        workspace,
        &auth.review_session_id,
        &auth.review_record_id,
        &auth.review_record_sha256,
        &auth.snapshot_expected,
        &auth.candidate_relative_path,
        &auth.candidate_sha256,
    )?;
    if attestation.attestation_digest != auth.build_attestation_sha256
        || attestation.build_attempt_id != auth.build_attempt_id
    {
        return Err(format!("{LABEL} attestation differs"));
    }
    let mut handoffs = parent.try_clone(LABEL)?;
    handoffs.descend_existing("restart-handoff", LABEL)?;
    let raw = read_optional_relative_regular(&handoffs, "latest.json", 4096, LABEL)?;
    let saved = prior
        .filter(|j| j.plan.transaction_id == transaction)
        .and_then(|j| j.plan.historical_handoff_sha256.as_ref());
    let raw = match (raw, saved) {
        (None, Some(sha)) => Some(read_relative_regular(
            &handoffs,
            &format!("retired-{sha}.json"),
            4096,
            LABEL,
        )?),
        (v, _) => v,
    };
    let handoff: Option<ReloadHandoffStateV1> = raw
        .as_ref()
        .map(|v| {
            serde_json::from_slice(v).map_err(|_| format!("{LABEL} historical receipt invalid"))
        })
        .transpose()?;
    let snapshot = validate_committed_snapshot(workspace, &auth.snapshot_expected)?;
    let helper = snapshot
        .entries
        .iter()
        .find(|v| v.relative_path == "scripts/catdesk-release-recovery.ps1")
        .ok_or("reviewed recovery helper is absent from the accepted snapshot")?;
    let proof = inspect_owners(workspace, &auth, handoff.as_ref(), &helper.sha256)?;
    if let Some(h) = &handoff {
        if h.schema_version != 1
            || h.status != "REPLACEMENT_READY_PENDING_TRANSPORT_RECONNECT"
            || h.stage != "complete"
            || h.port != 3200
            || h.old_pid == 0
            || h.new_pid.is_none_or(|v| v == 0)
            || h.rollback_attempted
            || h.rollback_ready
            || h.completed_at_unix == 0
            || h.replacement_sha256 == auth.prior_canonical_sha256
            || h.replacement_sha256 == auth.candidate_sha256
            || proof["originalSha256"] != h.replacement_sha256
            || proof["qualifiedUnix"]
                .as_u64()
                .is_none_or(|v| v <= h.completed_at_unix)
        {
            return Err(format!(
                "{LABEL} historical receipt is not superseded by verified stopped authority"
            ));
        }
    }
    let plan = Plan {
        schema_version: 1,
        transaction_id: transaction.into(),
        prior_sha256: auth.prior_canonical_sha256,
        candidate_sha256: auth.candidate_sha256,
        records: hashes,
        historical_handoff_sha256: raw.as_ref().map(|v| digest(v)),
        trusted_release_evidence_sha256: digest(&bytes(&proof)?),
    };
    if prior.is_some_and(|j| j.plan.transaction_id == transaction && j.plan != plan) {
        return Err(format!("{LABEL} recovery evidence changed"));
    }
    Ok(plan)
}

#[cfg(windows)]
pub(crate) fn inspect(workspace: &Path, transaction: &str) -> Result<serde_json::Value, String> {
    let parent = metadata(workspace)?;
    let _lock = create_delete_on_close_regular_lock(&parent, "promotion-operation.lock", LABEL)?;
    let previous = journal(&parent)?;
    if let Some(value) = &previous {
        if value.phase == "COMPLETED" && value.plan.transaction_id == transaction {
            verify_archive(&parent, value)?;
            return Ok(summary(value));
        }
    }
    let plan = inspect_locked(workspace, &parent, transaction, previous.as_ref())?;
    Ok(summary(&Journal {
        schema_version: 1,
        phase: "ELIGIBLE".into(),
        evidence_sha256: digest(&bytes(&plan)?),
        plan,
    }))
}

fn summary(value: &Journal) -> serde_json::Value {
    serde_json::json!({"state":value.phase,"transactionId":value.plan.transaction_id,"evidenceSha256":value.evidence_sha256,
        "candidateSha256":value.plan.candidate_sha256,"priorCanonicalSha256":value.plan.prior_sha256,
        "historicalReceiptIncluded":value.plan.historical_handoff_sha256.is_some(),"productionBinaryChanged":false,"sourceAuthorityCreated":false})
}

#[cfg(windows)]
fn archive_locked(
    parent: &Guard,
    value: &mut Journal,
    mut checkpoint: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    if value.phase == "PREPARED" {
        let name = archive_name(&value.plan.transaction_id)?;
        match (
            optional_child(parent, "promotion-control")?,
            optional_child(parent, &name)?,
        ) {
            (Some(current), None) => {
                if record_hashes(&current)? != value.plan.records {
                    return Err(format!("{LABEL} source records changed"));
                }
                let mut current = current.reopen_direct_child_for_delete(LABEL)?;
                current.rename_direct_child_within_parent(parent, &name, LABEL)?;
            }
            (None, Some(old)) if record_hashes(&old)? == value.plan.records => {}
            _ => return Err(format!("{LABEL} archive location ambiguous")),
        }
        checkpoint("PROMOTION_RENAMED")?;
        value.phase = "PROMOTION_ARCHIVED".into();
        update_journal(parent, value)?;
    }
    if value.phase == "PROMOTION_ARCHIVED" {
        if let Some(expected) = &value.plan.historical_handoff_sha256 {
            let handoffs = child(parent, "restart-handoff")?;
            let name = format!("retired-{expected}.json");
            match (
                read_optional_relative_regular(&handoffs, "latest.json", 4096, LABEL)?,
                read_optional_relative_regular(&handoffs, &name, 4096, LABEL)?,
            ) {
                (Some(raw), None) if digest(&raw) == *expected => {
                    rename_exact_regular_child(&handoffs, "latest.json", &name, &raw, LABEL)?
                }
                (None, Some(raw)) if digest(&raw) == *expected => {}
                _ => return Err(format!("{LABEL} handoff archive differs")),
            }
        }
        checkpoint("HANDOFF_RENAMED")?;
        verify_archive(parent, value)?;
        value.phase = "COMPLETED".into();
        update_journal(parent, value)?;
    }
    verify_archive(parent, value)
}

#[cfg(windows)]
pub(crate) fn retire(
    workspace: &Path,
    transaction: &str,
    expected: &str,
) -> Result<serde_json::Value, String> {
    if !valid_digest(expected) {
        return Err(format!("{LABEL} evidence digest invalid"));
    }
    let parent = metadata(workspace)?;
    let _lock = create_delete_on_close_regular_lock(&parent, "promotion-operation.lock", LABEL)?;
    let previous = journal(&parent)?;
    if let Some(value) = &previous {
        if value.phase == "COMPLETED" && value.plan.transaction_id == transaction {
            if value.evidence_sha256 != expected {
                return Err(format!("{LABEL} evidence differs"));
            }
            verify_archive(&parent, value)?;
            return Ok(summary(value));
        }
    }
    if previous
        .as_ref()
        .is_some_and(|v| v.phase != "COMPLETED" && v.plan.transaction_id != transaction)
    {
        return Err(format!("{LABEL} another recovery is pending"));
    }
    // Reuse the public lifecycle's existing exclusive stopped-owner lease.
    use std::os::windows::fs::OpenOptionsExt;
    let lifecycle = child(&parent, "release-pair-control")?;
    let _lease = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(lifecycle.path().join("stopped-owner.lock"))
        .map_err(|_| format!("{LABEL} public lifecycle owner is active"))?;
    let plan = inspect_locked(workspace, &parent, transaction, previous.as_ref())?;
    let evidence = digest(&bytes(&plan)?);
    if evidence != expected {
        return Err(format!("{LABEL} material evidence changed"));
    }
    // Retain the actual trusted disk pair through every metadata transition.
    // These handles forbid writes/deletes and never replace executable bytes.
    let mut release = Guard::acquire(workspace, LABEL)?;
    release.descend_existing("target", LABEL)?;
    release.descend_existing("release", LABEL)?;
    let _canonical = crate::windows_protected_fs::retain_reviewed_regular_artifact(
        &release,
        "catdesk.exe",
        &plan.prior_sha256,
        128 * 1024 * 1024,
        LABEL,
    )?;
    let fingerprint = read_relative_regular(&release, "catdesk.exe.sha256", 128, LABEL)?;
    if std::str::from_utf8(&fingerprint)
        .map_err(|_| "retirement fingerprint invalid")?
        .trim()
        .to_ascii_lowercase()
        != plan.prior_sha256
    {
        return Err(format!("{LABEL} canonical fingerprint changed"));
    }
    let _fingerprint = crate::windows_protected_fs::retain_reviewed_regular_artifact(
        &release,
        "catdesk.exe.sha256",
        &digest(&fingerprint),
        128,
        LABEL,
    )?;
    if let Some(old) = &previous {
        if old.phase == "COMPLETED" {
            verify_archive(&parent, old)?;
            // Previous completion remains durable before replacing the current journal.
            let name = format!(
                "promotion-retirement-receipt-{}.json",
                old.plan.transaction_id
            );
            let encoded = bytes(old)?;
            match read_optional_relative_regular(&parent, &name, 65536, LABEL)? {
                Some(saved) if saved == encoded => {}
                None => write_new_regular_in(&parent, &name, &encoded, LABEL)?,
                _ => return Err(format!("{LABEL} previous completion differs")),
            }
        }
    }
    let mut value = previous
        .filter(|v| v.plan.transaction_id == transaction)
        .unwrap_or(Journal {
            schema_version: 1,
            phase: "PREPARED".into(),
            evidence_sha256: evidence,
            plan,
        });
    update_journal(&parent, &value)?;
    archive_locked(&parent, &mut value, |_| Ok(()))?;
    Ok(summary(&value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reviewed_candidate_owner::OwnedCandidateChild;
    use std::sync::mpsc;

    fn cleanup_fixture(root: PathBuf) {
        let resolved = root.canonicalize().unwrap();
        let temp = std::env::temp_dir().canonicalize().unwrap();
        assert_eq!(resolved.parent(), Some(temp.as_path()));
        assert!(
            resolved
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("CatDesk-Retirement-Fixture-")
        );
        fs::remove_dir_all(resolved).unwrap();
    }

    fn terminal_fixture() -> (
        ReviewedPromotionAuthorizationV1,
        ReviewedPromotionPreflightV1,
        ReviewedPromotionClaimV1,
        ReviewedPromotionResultV1,
    ) {
        let auth = ReviewedPromotionAuthorizationV1 {
            schema_version: 1,
            generation: 1,
            authorization_id: "a".repeat(32),
            expires_at_unix: 1,
            candidate_relative_path: "candidate/catdesk.exe".into(),
            candidate_sha256: "b".repeat(64),
            prior_canonical_sha256: "c".repeat(64),
            promotion_script_sha256: "d".repeat(64),
            trusted_powershell_path: "fixture-interpreter".into(),
            trusted_powershell_sha256: "e".repeat(64),
            review_session_id: "fixture-review".into(),
            review_record_id: "fixture-record".into(),
            review_record_sha256: "f".repeat(64),
            transaction_id: "1".repeat(32),
            claim_id: "2".repeat(32),
            build_attestation_sha256: "3".repeat(64),
            build_attempt_id: "4".repeat(32),
            snapshot_id: "fixture-snapshot".into(),
            snapshot_authority_digest: "5".repeat(64),
            snapshot_manifest_digest: "6".repeat(64),
            build_policy_sha256: "7".repeat(64),
            cargo_sha256: "8".repeat(64),
            rustc_sha256: "9".repeat(64),
            snapshot_expected: ReviewedSourceSnapshotExpectedV1 {
                session_id: "fixture-review".into(),
                project_id: "catdesk".into(),
                approved_contract_hash: "a".repeat(64),
                logical_task_id: "fixture-task".into(),
                completion_artifact_ids: vec![],
                current_outputs: vec![],
                baseline_observations: vec![],
            },
            local_backend_only: true,
        };
        let preflight = ReviewedPromotionPreflightV1 {
            schema_version: 1,
            confirmation_token: "a".repeat(32),
            expires_at_unix: 1,
            authorization: auth.clone(),
        };
        let claim = ReviewedPromotionClaimV1 {
            schema_version: 1,
            authorization_id: auth.authorization_id.clone(),
            claim_id: auth.claim_id.clone(),
            transaction_id: auth.transaction_id.clone(),
            candidate_relative_path: auth.candidate_relative_path.clone(),
            state: "CLAIMED_PENDING".into(),
        };
        let result = ReviewedPromotionResultV1 {
            schema_version: 1,
            state: "PROMOTION_FAILED_OR_AMBIGUOUS".into(),
            authorization_generation: 1,
            candidate_sha256: auth.candidate_sha256.clone(),
            transaction_id: auth.transaction_id.clone(),
            serving_state: None,
        };
        (auth, preflight, claim, result)
    }

    #[test]
    fn only_exact_expired_local_terminal_binding_is_eligible_for_further_checks() {
        let (auth, preflight, claim, result) = terminal_fixture();
        let id = auth.transaction_id.clone();
        validate_terminal_binding(&auth, &preflight, &claim, &result, id.as_bytes(), &id).unwrap();
        for scenario in [
            "live-lease",
            "serving-mode",
            "changed-authority",
            "claim",
            "result",
            "generation",
            "candidate",
            "serving-claim",
            "owner",
            "preflight-expiry",
        ] {
            let (mut a, mut p, mut c, mut r) = terminal_fixture();
            let mut owner = id.as_bytes().to_vec();
            match scenario {
                "live-lease" => {
                    a.expires_at_unix = u64::MAX;
                    p.expires_at_unix = u64::MAX;
                    p.authorization = a.clone()
                }
                "serving-mode" => {
                    a.local_backend_only = false;
                    p.authorization = a.clone()
                }
                "changed-authority" => p.authorization.review_record_sha256 = "0".repeat(64),
                "claim" => c.transaction_id = "9".repeat(32),
                "result" => r.state = "PROMOTION_COMPLETED".into(),
                "generation" => r.authorization_generation = 2,
                "candidate" => r.candidate_sha256 = "f".repeat(64),
                "serving-claim" => r.serving_state = Some("STOPPED_AFTER_BOUNDED_OWNER".into()),
                "owner" => owner.clear(),
                _ => p.expires_at_unix = 2,
            }
            assert!(
                validate_terminal_binding(&a, &p, &c, &r, &owner, &id).is_err(),
                "{scenario}"
            );
        }
    }

    fn fixture() -> (PathBuf, Journal) {
        let root =
            std::env::temp_dir().join(format!("CatDesk-Retirement-Fixture-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join(".catdesk/promotion-control")).unwrap();
        fs::create_dir_all(root.join(".catdesk/restart-handoff")).unwrap();
        let mut records = BTreeMap::new();
        for name in RECORDS {
            let raw = format!("disposable fixture {name}");
            fs::write(root.join(".catdesk/promotion-control").join(name), &raw).unwrap();
            records.insert(name.into(), digest(raw.as_bytes()));
        }
        let receipt = b"disposable historical receipt";
        fs::write(root.join(".catdesk/restart-handoff/latest.json"), receipt).unwrap();
        let plan = Plan {
            schema_version: 1,
            transaction_id: "a".repeat(32),
            prior_sha256: "b".repeat(64),
            candidate_sha256: "c".repeat(64),
            records,
            historical_handoff_sha256: Some(digest(receipt)),
            trusted_release_evidence_sha256: digest(b"DISPOSABLE_FIXTURE_NOT_RELEASE_AUTHORITY"),
        };
        let journal = Journal {
            schema_version: 1,
            phase: "PREPARED".into(),
            evidence_sha256: digest(&bytes(&plan).unwrap()),
            plan,
        };
        let parent = metadata(&root).unwrap();
        update_journal(&parent, &journal).unwrap();
        drop(parent);
        (root, journal)
    }

    #[test]
    fn archive_preserves_bytes_and_completed_history_is_idempotent() {
        let (root, mut value) = fixture();
        let parent = metadata(&root).unwrap();
        assert!(ensure_ready(&root).is_err());
        archive_locked(&parent, &mut value, |_| Ok(())).unwrap();
        assert_eq!(value.phase, "COMPLETED");
        ensure_ready(&root).unwrap();
        archive_locked(&parent, &mut value, |_| {
            panic!("completed history must not execute another move")
        })
        .unwrap();
        assert!(!root.join(".catdesk/promotion-control").exists());
        assert!(!root.join(".catdesk/restart-handoff/latest.json").exists());
        drop(parent);
        cleanup_fixture(root);
    }

    #[test]
    fn every_interruption_boundary_resumes_without_repeating_a_move() {
        for stop in ["PROMOTION_RENAMED", "HANDOFF_RENAMED"] {
            let (root, mut value) = fixture();
            let parent = metadata(&root).unwrap();
            assert!(
                archive_locked(&parent, &mut value, |point| if point == stop {
                    Err("controlled interruption".into())
                } else {
                    Ok(())
                })
                .is_err()
            );
            assert!(ensure_ready(&root).is_err());
            let mut recovered = journal(&parent).unwrap().unwrap();
            archive_locked(&parent, &mut recovered, |_| Ok(())).unwrap();
            ensure_ready(&root).unwrap();
            drop(parent);
            cleanup_fixture(root);
        }
    }

    #[test]
    fn promotion_source_requires_completed_matching_retirement_and_no_new_handoff() {
        let (root, mut value) = fixture();
        let (mut auth, _, _, _) = terminal_fixture();
        auth.prior_canonical_sha256 = value.plan.prior_sha256.clone();
        auth.candidate_sha256 = value.plan.candidate_sha256.clone();
        assert!(admit_stopped_source(&root, &auth).is_err());
        let parent = metadata(&root).unwrap();
        archive_locked(&parent, &mut value, |_| Ok(())).unwrap();
        let mut changed = auth.clone();
        changed.candidate_sha256 = "f".repeat(64);
        assert!(admit_stopped_source(&root, &changed).is_err());
        changed = auth.clone();
        changed.prior_canonical_sha256 = "f".repeat(64);
        assert!(admit_stopped_source(&root, &changed).is_err());
        fs::write(
            root.join(".catdesk/restart-handoff/latest.json"),
            b"unknown new handoff",
        )
        .unwrap();
        assert!(admit_stopped_source(&root, &auth).is_err());
        assert_eq!(
            fs::read(root.join(".catdesk/restart-handoff/latest.json")).unwrap(),
            b"unknown new handoff"
        );
        drop(parent);
        cleanup_fixture(root);
    }

    #[test]
    fn changed_records_archive_collision_and_unknown_receipts_remain_quarantined() {
        for scenario in ["record", "collision", "receipt", "unexpected"] {
            let (root, mut value) = fixture();
            let parent = metadata(&root).unwrap();
            match scenario {
                "record" => fs::write(
                    root.join(".catdesk/promotion-control/claim.json"),
                    b"changed",
                )
                .unwrap(),
                "collision" => fs::create_dir(
                    root.join(".catdesk")
                        .join(archive_name(&value.plan.transaction_id).unwrap()),
                )
                .unwrap(),
                "receipt" => fs::write(
                    root.join(".catdesk/restart-handoff/latest.json"),
                    b"unrelated current receipt",
                )
                .unwrap(),
                _ => fs::write(
                    root.join(".catdesk/promotion-control/unrelated.json"),
                    b"unknown",
                )
                .unwrap(),
            }
            assert!(
                archive_locked(&parent, &mut value, |_| Ok(())).is_err(),
                "{scenario}"
            );
            assert!(ensure_ready(&root).is_err());
            if scenario == "receipt" {
                assert_eq!(
                    fs::read(root.join(".catdesk/restart-handoff/latest.json")).unwrap(),
                    b"unrelated current receipt"
                )
            }
            drop(parent);
            cleanup_fixture(root);
        }
    }

    #[test]
    fn release_operation_lock_is_exclusive_and_does_not_block_another_workspace() {
        let (a, _) = fixture();
        let (b, _) = fixture();
        let lease = mutation_lease(&a).unwrap();
        assert!(mutation_lease(&a).is_err());
        drop(mutation_lease(&b).unwrap());
        drop(lease);
        drop(mutation_lease(&a).unwrap());
        cleanup_fixture(a);
        cleanup_fixture(b);
    }

    #[test]
    fn journal_tampering_never_enables_a_new_preflight() {
        let (root, mut value) = fixture();
        value.plan.candidate_sha256 = "d".repeat(64);
        let parent = metadata(&root).unwrap();
        update_journal(&parent, &value).unwrap();
        assert!(journal(&parent).is_err());
        assert!(ensure_ready(&root).is_err());
        drop(parent);
        cleanup_fixture(root);
    }

    #[test]
    fn hard_kill_fixture() {
        let Ok(root) = std::env::var("CATDESK_RETIREMENT_FIXTURE_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        assert!(
            root.starts_with(std::env::temp_dir())
                && root
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("CatDesk-Retirement-Fixture-")
        );
        let stop = std::env::var("CATDESK_RETIREMENT_FIXTURE_STOP").unwrap();
        let parent = metadata(&root).unwrap();
        let _lock = create_delete_on_close_regular_lock(&parent, "promotion-operation.lock", LABEL)
            .unwrap();
        let mut value = journal(&parent).unwrap().unwrap();
        archive_locked(&parent, &mut value, |point| {
            if point == stop {
                println!("RETIREMENT_CHECKPOINT");
                std::io::stdout().flush().unwrap();
                thread::sleep(Duration::from_secs(60))
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn image_owner_fixture() {
        if std::env::var("CATDESK_RETIREMENT_IMAGE_OWNER").as_deref() == Ok("fixture-only") {
            thread::sleep(Duration::from_secs(60))
        }
    }

    #[test]
    fn actual_owned_image_blocks_readonly_inspection_until_its_job_exits() {
        let (root, _) = fixture();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("candidate")).unwrap();
        fs::create_dir_all(root.join("target/release")).unwrap();
        fs::write(
            root.join("target/release/catdesk.exe"),
            b"disposable prior image",
        )
        .unwrap();
        fs::write(
            root.join("slot.json"),
            b"disposable fixture slot, not release authority",
        )
        .unwrap();
        let helper = r#"
function Get-CatDeskRecoverySha256($Path){$s=[Security.Cryptography.SHA256]::Create();try{([BitConverter]::ToString($s.ComputeHash([IO.File]::ReadAllBytes($Path))).Replace('-','')).ToLowerInvariant()}finally{$s.Dispose()}}
function Get-CatDeskCanonicalPairEvidence($Root){$p=Join-Path $Root 'target/release/catdesk.exe';[pscustomobject]@{Binary=$p;Hash=Get-CatDeskRecoverySha256 $p}}
function Get-CatDeskLastKnownGoodRelease($Root){[pscustomobject]@{Slot='slot-b';Generation=2;Hash=(Get-CatDeskCanonicalPairEvidence $Root).Hash;Metadata=(Join-Path $Root 'slot.json')}}
function Get-NetTCPConnection{[CmdletBinding()]param([string]$State)}
"#;
        let hp = root.join("scripts/catdesk-release-recovery.ps1");
        fs::write(&hp, helper).unwrap();
        let candidate = root.join("candidate/catdesk.exe");
        fs::copy(std::env::current_exe().unwrap(), &candidate).unwrap();
        let (mut auth, _, _, _) = terminal_fixture();
        auth.prior_canonical_sha256 =
            sha256_file(&root.join("target/release/catdesk.exe")).unwrap();
        let mut command = Command::new(&candidate);
        command
            .args([
                "--exact",
                "daemon_reload::retirement::tests::image_owner_fixture",
                "--nocapture",
            ])
            .env("CATDESK_RETIREMENT_IMAGE_OWNER", "fixture-only")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut owner = OwnedCandidateChild::spawn(&mut command).unwrap();
        let refused = inspect_owners(&root, &auth, None, &sha256_file(&hp).unwrap()).is_err();
        owner.terminate_tree_and_confirm_quiescent().unwrap();
        drop(owner);
        assert!(
            refused,
            "a mapped owned candidate must block retirement even without a listener"
        );
        inspect_owners(&root, &auth, None, &sha256_file(&hp).unwrap()).unwrap();
        cleanup_fixture(root);
    }

    #[test]
    fn actual_windows_owner_kill_preserves_journal_and_fresh_owner_recovers() {
        for stop in ["PROMOTION_RENAMED", "HANDOFF_RENAMED"] {
            let (root, _) = fixture();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "daemon_reload::retirement::tests::hard_kill_fixture",
                    "--nocapture",
                ])
                .env("CATDESK_RETIREMENT_FIXTURE_ROOT", &root)
                .env("CATDESK_RETIREMENT_FIXTURE_STOP", stop)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            let mut owner = OwnedCandidateChild::spawn(&mut command).unwrap();
            let out = owner.child_mut().stdout.take().unwrap();
            let (tx, rx) = mpsc::channel();
            let reader = thread::spawn(move || {
                use std::io::BufRead;
                let mut stream = std::io::BufReader::new(out);
                let mut line = String::new();
                while stream.read_line(&mut line).unwrap_or(0) > 0 {
                    if line.contains("RETIREMENT_CHECKPOINT") {
                        let _ = tx.send(());
                        break;
                    }
                    line.clear()
                }
            });
            let reached = rx.recv_timeout(Duration::from_secs(8)).is_ok();
            owner.terminate_tree_and_confirm_quiescent().unwrap();
            drop(owner);
            reader.join().unwrap();
            assert!(reached, "owned fixture did not reach {stop}");
            let parent = metadata(&root).unwrap();
            let _lease = mutation_lease(&root).unwrap();
            let mut value = journal(&parent).unwrap().unwrap();
            archive_locked(&parent, &mut value, |_| Ok(())).unwrap();
            ensure_ready(&root).unwrap();
            drop(_lease);
            drop(parent);
            cleanup_fixture(root);
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn inspect(_workspace: &Path, _transaction: &str) -> Result<serde_json::Value, String> {
    Err("stopped promotion retirement requires Windows".into())
}
#[cfg(not(windows))]
pub(crate) fn retire(
    _workspace: &Path,
    _transaction: &str,
    _expected: &str,
) -> Result<serde_json::Value, String> {
    Err("stopped promotion retirement requires Windows".into())
}
