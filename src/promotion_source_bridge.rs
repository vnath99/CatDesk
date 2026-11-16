//! Bounded old-listener bridge for the existing reviewed promotion worker.
//! It reuses qualified-stopped headless startup; it never installs an image,
//! creates release authority, disarms a Job, or starts Wake/browser/tunnel work.
use super::*;
use crate::reviewed_candidate_owner::OwnedCandidateChild;
use crate::windows_protected_fs::{
    ProtectedDirectoryGuard as Guard, retain_reviewed_regular_artifact,
};
use std::os::windows::fs::OpenOptionsExt;

const ROUTE: &str = "catdesk-qualified-stopped-v2";
const ENDPOINT: &str = "http://127.0.0.1:3200/catdesk-qualified-stopped-v2/mcp";

pub(super) fn lifecycle_lease(workspace: &Path) -> Result<fs::File, String> {
    let mut parent = Guard::acquire(workspace, "promotion source lifecycle")?;
    parent.descend_existing(".catdesk", "promotion source lifecycle")?;
    parent.descend_existing("release-pair-control", "promotion source lifecycle")?;
    let path = parent.path().join("stopped-owner.lock");
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| "promotion source lifecycle unavailable")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("promotion source lifecycle lock is unsafe".into());
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(path)
        .map_err(|_| "promotion source public lifecycle is owned".into())
}

fn qualified_config(workspace: &Path, prior: &str) -> Result<(PathBuf, String), String> {
    let proof =
        stopped::result(workspace)?.ok_or("promotion source has no stopped qualification")?;
    let path = workspace.join(".catdesk/release-pair-control/qualification-config.toml");
    let observed =
        fs::read_to_string(&path).map_err(|_| "promotion source qualified config unavailable")?;
    validate_qualified_config(&proof, prior, &observed)?;
    Ok((path, format!("{:x}", Sha256::digest(observed.as_bytes()))))
}

fn validate_qualified_config(
    proof: &stopped::ResultRecord,
    prior: &str,
    observed: &str,
) -> Result<(), String> {
    if proof.state != "QUALIFIED_STOPPED"
        || proof.profile != "HEADLESS_LOCAL"
        || proof.candidate_sha256 != prior
    {
        return Err("promotion source qualification differs".into());
    }
    let mut expected = crate::state::AppConfig::default();
    expected.identity.installation_id = Some(proof.transaction_id.clone());
    expected.mcp.route_id = Some(ROUTE.into());
    let text = toml::to_string(&expected).map_err(|_| "promotion source config unavailable")?;
    if observed != text {
        return Err("promotion source qualified config changed".into());
    }
    Ok(())
}

fn command(image: &Path, workspace: &Path, config: &Path, token: &str) -> Command {
    // Reuse the existing installed foreground runtime's fixed read-only mode.
    // This bridge owns no output pipe and does not export a serving endpoint.
    let mut cmd =
        crate::operator_facade::promotion_source_command(workspace, image, config, 120, token);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    cmd
}

pub(super) struct SourceBridge {
    owner: OwnedCandidateChild,
    _config: crate::windows_protected_fs::RetainedReviewedArtifact,
    _lifecycle: Option<fs::File>,
}

impl SourceBridge {
    fn start(
        workspace: &Path,
        auth: &ReviewedPromotionAuthorizationV1,
    ) -> Result<(Self, usize), String> {
        let (config, config_hash) = qualified_config(workspace, &auth.prior_canonical_sha256)?;
        let mut control = Guard::acquire(workspace, "promotion source config")?;
        control.descend_existing(".catdesk", "promotion source config")?;
        control.descend_existing("release-pair-control", "promotion source config")?;
        let config_pin = retain_reviewed_regular_artifact(
            &control,
            "qualification-config.toml",
            &config_hash,
            65536,
            "promotion source config",
        )?;
        let mut release = Guard::acquire(workspace, "promotion source image")?;
        release.descend_existing("target", "promotion source image")?;
        release.descend_existing("release", "promotion source image")?;
        let image_pin = retain_reviewed_regular_artifact(
            &release,
            "catdesk.exe",
            &auth.prior_canonical_sha256,
            128 * 1024 * 1024,
            "promotion source image",
        )?;
        let token = Uuid::new_v4().simple().to_string();
        let mut child = command(
            &release.path().join("catdesk.exe"),
            workspace,
            &config,
            &token,
        );
        let owner = OwnedCandidateChild::spawn(&mut child)?;
        let mut bridge = Self {
            owner,
            _config: config_pin,
            _lifecycle: None,
        };
        let outcome = (|| {
            if !wait_child_ready(bridge.owner.child_mut(), 3200, Duration::from_secs(15)) {
                return Err("promotion source did not become ready".into());
            }
            retirement::verify_bridge_listener(workspace, auth, bridge.owner.pid())?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "promotion source MCP verifier unavailable")?;
            runtime.block_on(crate::operator_facade::verify_promotion_source_mcp(
                ENDPOINT, &token,
            ))?;
            retirement::verify_bridge_listener(workspace, auth, bridge.owner.pid())?;
            if bridge
                .owner
                .child_mut()
                .try_wait()
                .map_err(|_| "promotion source process unproven")?
                .is_some()
            {
                return Err("promotion source exited during verification".into());
            }
            Ok(10)
        })();
        // The promotion must be able to replace the old image after the exact
        // existing handoff stops it. Its disk hash is revalidated by that worker.
        drop(image_pin);
        match outcome {
            Ok(count) => Ok((bridge, count)),
            Err(error) => {
                bridge.stop()?;
                Err(error)
            }
        }
    }

    pub(super) fn stop(&mut self) -> Result<(), String> {
        self.owner.terminate_tree_and_confirm_quiescent()
    }
}

pub(super) fn qualify(
    workspace: &Path,
    auth: &ReviewedPromotionAuthorizationV1,
) -> Result<serde_json::Value, String> {
    let (mut bridge, count) = SourceBridge::start(workspace, auth)?;
    bridge.stop()?;
    Ok(
        serde_json::json!({"state":"QUALIFIED_PROMOTION_SOURCE_STOPPED","priorCanonicalSha256":auth.prior_canonical_sha256,
        "unauthorizedStatus":401,"authorizedStatus":200,"initializeVerified":true,"toolsListVerified":true,"toolCount":count,
        "ownedTreeExited":true,"maximumRuntimeSeconds":120,"productionBinaryChanged":false,"releaseAuthorityCreated":false,
        "browserStarted":false,"wakeStarted":false,"tunnelStarted":false}),
    )
}

pub(super) fn start_if_required(
    workspace: &Path,
    auth: &ReviewedPromotionAuthorizationV1,
) -> Result<Option<SourceBridge>, String> {
    if !auth.local_backend_only {
        return Ok(None);
    }
    let listener_possible = [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
    ]
    .iter()
    .any(|ip| {
        TcpStream::connect_timeout(&SocketAddr::new(*ip, 3200), Duration::from_millis(250)).is_ok()
    });
    if listener_possible {
        return Ok(None);
    }
    let _mutation = retirement::mutation_lease(workspace)?;
    let lifecycle = lifecycle_lease(workspace)?;
    retirement::admit_stopped_source(workspace, auth)?;
    let (mut bridge, _) = SourceBridge::start(workspace, auth)?;
    bridge._lifecycle = Some(lifecycle);
    Ok(Some(bridge))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_existing_qualified_stopped_profile_and_exact_config_are_eligible() {
        // In-memory predicate input only; no release authority is created.
        let fixture = serde_json::json!({"schemaVersion":2,"mode":"QUALIFIED_STOPPED","profile":"HEADLESS_LOCAL",
            "state":"QUALIFIED_STOPPED","transactionId":"a".repeat(32),"candidateSha256":"b".repeat(64),"receiptSha256":"c".repeat(64)});
        let proof: stopped::ResultRecord = serde_json::from_value(fixture.clone()).unwrap();
        let mut config = crate::state::AppConfig::default();
        config.identity.installation_id = Some(proof.transaction_id.clone());
        config.mcp.route_id = Some(ROUTE.into());
        let text = toml::to_string(&config).unwrap();
        validate_qualified_config(&proof, &"b".repeat(64), &text).unwrap();
        for (key, value) in [
            ("state", "STOPPED_RECOVERY_REQUIRED"),
            ("profile", "UNKNOWN"),
            ("candidateSha256", "changed"),
        ] {
            let mut changed = fixture.clone();
            changed[key] = value.into();
            let bad: stopped::ResultRecord = serde_json::from_value(changed).unwrap();
            assert!(validate_qualified_config(&bad, &"b".repeat(64), &text).is_err());
        }
        assert!(validate_qualified_config(&proof, &"d".repeat(64), &text).is_err());
        config.ngrok_authtoken = Some("fixture-value-not-a-secret".into());
        assert!(
            validate_qualified_config(&proof, &"b".repeat(64), &toml::to_string(&config).unwrap())
                .is_err()
        );
        assert!(validate_qualified_config(&proof, &"b".repeat(64), &(text + "\n")).is_err());
    }

    #[test]
    fn public_lifecycle_lease_is_exclusive_and_preserves_existing_bytes() {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let root = temp.join(format!("CatDesk-SourceBridge-Fixture-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join(".catdesk/release-pair-control")).unwrap();
        let path = root.join(".catdesk/release-pair-control/stopped-owner.lock");
        fs::write(&path, b"existing lifecycle lock").unwrap();
        let lease = lifecycle_lease(&root).unwrap();
        assert!(lifecycle_lease(&root).is_err());
        drop(lease);
        drop(lifecycle_lease(&root).unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"existing lifecycle lock");
        assert_eq!(root.canonicalize().unwrap().parent(), Some(temp.as_path()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bridge_launch_is_fixed_bounded_local_and_keeps_token_off_command_line() {
        let workspace = Path::new(r"C:\fixture");
        let config = workspace.join("qualified.toml");
        let cmd = command(
            &workspace.join("catdesk.exe"),
            workspace,
            &config,
            "fixture-only-token",
        );
        let args: Vec<_> = cmd
            .get_args()
            .map(|v| v.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            args,
            vec![
                "--headless-mcp",
                "--host",
                "127.0.0.1",
                "--port",
                "3200",
                "--workspace",
                r"C:\fixture",
                "--config-path",
                r"C:\fixture\qualified.toml",
                "--mcp-path",
                "/catdesk-qualified-stopped-v2/mcp",
                "--mode",
                "computer",
                "--tool-mode",
                "read-only",
                "--no-ngrok",
                "--no-web-auto",
                "--max-runtime-seconds",
                "120"
            ]
        );
        assert!(!args.iter().any(|v| v.contains("fixture-only-token")));
        let env: std::collections::BTreeMap<_, _> = cmd.get_envs().collect();
        assert_eq!(
            env.get(std::ffi::OsStr::new("CATDESK_MCP_AUTH_TOKEN")),
            Some(&Some(std::ffi::OsStr::new("fixture-only-token")))
        );
        for name in ["CONTROL_PLANE_API_KEY", "CATDESK_OPENAI_TUNNEL_ID"] {
            assert_eq!(env.get(std::ffi::OsStr::new(name)), Some(&None));
        }
    }
}
