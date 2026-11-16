use super::*;
use std::collections::BTreeMap;

const TOKEN: &str = "fixture-mcp-credential-for-child-only-test";

fn connect_args() -> Vec<String> {
    build_runtime_connect_command(
        Path::new("unused-client.exe"),
        "fixture-alias",
        "fixture-tunnel",
        "http://127.0.0.1:3200/AbCdEf123456789012345678/mcp",
    )
    .unwrap()
    .args
}

fn child_environment(command: &Command) -> BTreeMap<OsString, Option<OsString>> {
    command
        .as_std()
        .get_envs()
        .map(|(k, v)| (k.into(), v.map(Into::into)))
        .collect()
}

#[test]
fn runtime_mcp_auth_is_child_only_and_never_an_argument() {
    let keys = [
        RUNTIME_MCP_AUTHORIZATION_ENV,
        "MCP_EXTRA_HEADERS",
        "MCP_DISCOVERY_EXTRA_HEADERS",
    ];
    let before: Vec<_> = keys.iter().map(std::env::var_os).collect();
    let args = connect_args();
    let mut command = Command::new("unused-client.exe");
    append_client_args(
        &mut command,
        Path::new("unused-client.exe"),
        args.iter().map(String::as_str),
    );
    configure_runtime_mcp_auth(&mut command, &args, Some(TOKEN), |_| None).unwrap();
    let env = child_environment(&command);
    assert_eq!(env.len(), 3);
    assert_eq!(
        env[&OsString::from(RUNTIME_MCP_AUTHORIZATION_ENV)],
        Some(format!("Bearer {TOKEN}").into())
    );
    for key in &keys[1..] {
        assert_eq!(
            env[&OsString::from(key)],
            Some(RUNTIME_MCP_AUTHORIZATION_REFERENCE.into())
        );
    }
    assert!(
        !command
            .as_std()
            .get_args()
            .any(|value| value.to_string_lossy().contains(TOKEN))
    );
    assert!(
        keys.iter().map(std::env::var_os).collect::<Vec<_>>() == before,
        "parent environment changed"
    );
}

#[test]
fn runtime_mcp_auth_refuses_invalid_tokens_and_conflicting_header_policy_before_launch() {
    let args = connect_args();
    for token in [
        "",
        "short",
        "fixture credential contains spaces",
        "fixture-credential\nwith-newline",
        "credential-non-ascii-\u{e9}-invalid",
    ] {
        let mut command = Command::new("unused-client.exe");
        let error =
            configure_runtime_mcp_auth(&mut command, &args, Some(token), |_| None).unwrap_err();
        assert!(error.to_string().contains("LOCAL_MCP_AUTH_TOKEN_INVALID"));
        assert!(child_environment(&command).is_empty());
    }
    for conflicting in ["MCP_EXTRA_HEADERS", "MCP_DISCOVERY_EXTRA_HEADERS"] {
        let mut command = Command::new("unused-client.exe");
        let error = configure_runtime_mcp_auth(&mut command, &args, Some(TOKEN), |key| {
            (key == conflicting).then(|| OsString::from("Authorization: env:DIFFERENT_POLICY"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("LOCAL_MCP_AUTH_HEADER_CONFLICT"));
        assert!(child_environment(&command).is_empty());
    }
    for existing in ["", RUNTIME_MCP_AUTHORIZATION_REFERENCE] {
        let mut command = Command::new("unused-client.exe");
        configure_runtime_mcp_auth(&mut command, &args, Some(TOKEN), |_| Some(existing.into()))
            .unwrap();
    }
}

#[test]
fn runtime_mcp_auth_preserves_legacy_and_refuses_wrong_target_or_command() {
    let mut legacy = Command::new("unused-client.exe");
    legacy.env("MCP_EXTRA_HEADERS", "existing-legacy-policy");
    let before = child_environment(&legacy);
    configure_runtime_mcp_auth(&mut legacy, &connect_args(), None, |_| {
        Some("other-policy".into())
    })
    .unwrap();
    assert_eq!(child_environment(&legacy), before);
    let mut wrong_command = connect_args();
    wrong_command[1] = "status".into();
    let mut remote = connect_args();
    *remote.last_mut().unwrap() = "https://example.invalid/mcp".into();
    let mut duplicate_target = connect_args();
    duplicate_target.extend([
        "--mcp-server-url".into(),
        "http://127.0.0.1:3200/AbCdEf123456789012345678/mcp".into(),
    ]);
    let missing_target = connect_args()[..2].to_vec();
    for args in [wrong_command, remote, duplicate_target, missing_target] {
        let mut command = Command::new("unused-client.exe");
        assert!(configure_runtime_mcp_auth(&mut command, &args, Some(TOKEN), |_| None).is_err());
        assert!(child_environment(&command).is_empty());
    }
}

#[test]
fn runtime_mcp_auth_redacts_unexpected_native_echo_on_success_and_failure() {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    for (success, token) in [true, false].into_iter().flat_map(|success| {
        [TOKEN, "fixture-credential-with-\"quote-and-\\slash-123"].map(|token| (success, token))
    }) {
        let status = std::process::ExitStatus::from_raw(if success {
            0
        } else if cfg!(windows) {
            1
        } else {
            256
        });
        let output = std::process::Output {
            status,
            stdout: format!("OK {token}\nJSON {}", serde_json::to_string(token).unwrap())
                .into_bytes(),
            stderr: format!("Authorization: Bearer {token}\n").into_bytes(),
        };
        let output = redact_runtime_mcp_auth_output(output, Some(token));
        let encoded = serde_json::to_string(token).unwrap();
        let escaped = &encoded[1..encoded.len() - 1];
        assert!(!String::from_utf8_lossy(&output.stdout).contains(token));
        assert!(!String::from_utf8_lossy(&output.stdout).contains(escaped));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(token));
        let rendered = match command_output_to_string(output) {
            Ok(text) => text,
            Err(error) => error.to_string(),
        };
        assert!(!rendered.contains(token));
        assert!(!rendered.contains(escaped));
    }
}

#[cfg(windows)]
#[tokio::test]
async fn runtime_mcp_auth_real_windows_child_receives_bridge_without_parent_mutation() {
    let directory =
        std::env::temp_dir().join(format!("catdesk-runtime-auth-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("fixture-client.cmd");
    std::fs::write(&path, format!(
        "@echo off\r\nif not \"%CATDESK_TUNNEL_MCP_AUTHORIZATION%\"==\"Bearer {TOKEN}\" exit /b 11\r\nif not \"%MCP_EXTRA_HEADERS%\"==\"{RUNTIME_MCP_AUTHORIZATION_REFERENCE}\" exit /b 12\r\nif not \"%MCP_DISCOVERY_EXTRA_HEADERS%\"==\"{RUNTIME_MCP_AUTHORIZATION_REFERENCE}\" exit /b 13\r\necho AUTH_CHILD_OK\r\necho {TOKEN}\r\nexit /b 0\r\n"
    )).unwrap();
    let keys = [
        RUNTIME_MCP_AUTHORIZATION_ENV,
        "MCP_EXTRA_HEADERS",
        "MCP_DISCOVERY_EXTRA_HEADERS",
    ];
    let before: Vec<_> = keys.iter().map(std::env::var_os).collect();
    let args = connect_args();
    let mut command = command_for_path(&path);
    append_client_args(&mut command, &path, args.iter().map(String::as_str));
    configure_runtime_mcp_auth(&mut command, &args, Some(TOKEN), |_| None).unwrap();
    let output = tokio::time::timeout(COMMAND_TIMEOUT, command.output())
        .await
        .unwrap()
        .unwrap();
    let result =
        command_output_to_string(redact_runtime_mcp_auth_output(output, Some(TOKEN))).unwrap();
    assert!(result.contains("AUTH_CHILD_OK"));
    assert!(!result.contains(TOKEN));
    assert!(
        keys.iter().map(std::env::var_os).collect::<Vec<_>>() == before,
        "parent environment changed"
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&directory).unwrap();
}
