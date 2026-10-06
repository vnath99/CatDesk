//! Independently invokable, read-only CatDesk stable wake readiness host.
//!
//! Invocation: `catdesk-stable-wake-host --workspace <absolute-workspace>`.
//! An optional `--expected-target-sha256 <64-hex>` only constrains the exact
//! configured target; it cannot choose, discover, or mutate a conversation.

#[path = "../stable_wake_core.rs"]
mod stable_wake_core;

use std::{env, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match parse_args(env::args().skip(1).collect()) {
        Ok((workspace, expected_target_sha256)) => {
            match stable_wake_core::evaluate_host(&workspace, expected_target_sha256.as_deref()) {
                Ok(readiness) => {
                    // Deliberately bounded and non-secret: no path, target URL,
                    // profile, credentials, or durable record is serialized.
                    println!(
                        "{{\"status\":\"READY\",\"actionableCount\":{},\"staleCount\":{},\"targetSha256\":\"{}\"}}",
                        readiness.pending_count,
                        readiness.stale_count,
                        readiness.target_sha256.expect("successful target identity"),
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    // Error vocabulary is fixed and bounded by the core.
                    println!(
                        "{{\"status\":\"REFUSED\",\"reason\":\"{}\"}}",
                        json_escape(&error)
                    );
                    ExitCode::from(2)
                }
            }
        }
        Err(error) => {
            println!("{{\"status\":\"REFUSED\",\"reason\":\"{}\"}}", error);
            ExitCode::from(2)
        }
    }
}

fn parse_args(args: Vec<String>) -> Result<(PathBuf, Option<String>), &'static str> {
    let mut values = args.into_iter();
    if values.next().as_deref() != Some("--workspace") {
        return Err("invalid_arguments");
    }
    let workspace = values.next().ok_or("invalid_arguments")?;
    let expected_target_sha256 = match values.next() {
        None => None,
        Some(flag) if flag == "--expected-target-sha256" => {
            Some(values.next().ok_or("invalid_arguments")?)
        }
        Some(_) => return Err("invalid_arguments"),
    };
    if values.next().is_some() || workspace.len() > 32_768 {
        return Err("invalid_arguments");
    }
    if expected_target_sha256.as_deref().is_some_and(|value| {
        value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err("invalid_arguments");
    }
    Ok((PathBuf::from(workspace), expected_target_sha256))
}

fn json_escape(value: &str) -> String {
    // All production errors are fixed ASCII vocabulary; retain JSON correctness
    // if a future bounded error gains punctuation.
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::parse_args;

    #[test]
    fn command_line_has_only_fixed_bounded_authority() {
        assert!(parse_args(vec!["--workspace".into(), "C:\\safe".into()]).is_ok());
        assert!(
            parse_args(vec![
                "--workspace".into(),
                "C:\\safe".into(),
                "--url".into(),
                "x".into()
            ])
            .is_err()
        );
        assert!(
            parse_args(vec![
                "--workspace".into(),
                "C:\\safe".into(),
                "--expected-target-sha256".into(),
                "0".repeat(64)
            ])
            .is_ok()
        );
    }
}
