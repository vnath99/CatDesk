#[cfg(target_os = "windows")]
mod windows {
    use std::path::Path;
    use std::process::Command;

    fn run_checked_script(root: &Path, relative: &str) {
        let script = root.join(relative);
        assert!(
            script.is_file(),
            "missing PowerShell fixture: {}",
            script.display()
        );
        let output = Command::new("powershell.exe")
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script)
            .current_dir(root)
            .output()
            .unwrap_or_else(|error| panic!("failed to launch {}: {error}", script.display()));
        if !output.status.success() {
            panic!(
                "PowerShell fixture failed: {}\nstatus={}\nstdout={}\nstderr={}",
                script.display(),
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
    }

    fn assert_powershell_parses(root: &Path, relative: &str) {
        let script = root.join(relative);
        let escaped = script.to_string_lossy().replace('\'', "''");
        let parser = format!(
            "[void][scriptblock]::Create((Get-Content -Raw -LiteralPath '{}'))",
            escaped
        );
        let output = Command::new("powershell.exe")
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(parser)
            .current_dir(root)
            .output()
            .unwrap_or_else(|error| panic!("failed to parse {}: {error}", script.display()));
        assert!(
            output.status.success(),
            "PowerShell parse failed: {}\nstatus={}\nstdout={}\nstderr={}",
            script.display(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    #[test]
    fn lifecycle_and_reviewed_release_recovery_fixtures_pass() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for script in [
            "scripts/test-start-catdesk-stack.ps1",
            "scripts/test-stale-canonical-daemon-recovery.ps1",
            "scripts/test-catdesk-lifecycle.ps1",
            "scripts/test-catdesk-autostart-supervisor.ps1",
            "scripts/test-catdesk-production-acceptance.ps1",
            "scripts/test-promote-reviewed-catdesk-build.ps1",
            "scripts/test-restart-catdesk-daemon-process-identity.ps1",
            "scripts/test-secure-mcp-route-validation.ps1",
        ] {
            run_checked_script(root, script);
        }
    }

    #[test]
    fn public_facade_validates_lifecycle_engine_identity_before_dot_source() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let source = std::fs::read_to_string(root.join("catdesk.ps1"))
            .expect("read public lifecycle facade");

        let identity_guard = source
            .find("$engineExpectedPath = [System.IO.Path]::GetFullPath")
            .expect("facade must normalize the expected lifecycle-engine path");
        let file_info_guard = source[identity_guard..]
            .find("$engineInfo -is [System.IO.FileInfo]")
            .map(|offset| identity_guard + offset)
            .expect("facade must require a real lifecycle-engine file leaf");
        let reparse_guard = source[file_info_guard..]
            .find("[System.IO.FileAttributes]::ReparsePoint")
            .map(|offset| file_info_guard + offset)
            .expect("facade must reject a reparse lifecycle-engine leaf");
        let exact_identity_guard = source[reparse_guard..]
            .find("[System.StringComparison]::OrdinalIgnoreCase")
            .map(|offset| reparse_guard + offset)
            .expect("facade must require exact normalized lifecycle-engine identity");
        let dot_source = source[exact_identity_guard..]
            .find(". $enginePath -Workspace")
            .map(|offset| exact_identity_guard + offset)
            .expect("facade must dot-source only the validated lifecycle engine");

        assert!(identity_guard < file_info_guard);
        assert!(file_info_guard < reparse_guard);
        assert!(reparse_guard < exact_identity_guard);
        assert!(exact_identity_guard < dot_source);
        assert!(
            !source.contains("Test-Path -LiteralPath $enginePath -PathType Leaf"),
            "facade must not regress to leaf-existence-only lifecycle-engine validation"
        );
    }

    #[test]
    fn public_status_process_uses_json_state_instead_of_nonzero_exit() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let script = root.join("catdesk.ps1");
        let output = Command::new("powershell.exe")
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script)
            .arg("status")
            .current_dir(root)
            .output()
            .expect("launch public lifecycle status");

        assert!(
            output.status.success(),
            "status process must carry lifecycle failure in JSON, not exit status: status={} stdout={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            output.stderr.is_empty(),
            "public status must keep stderr empty"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("\"command\":\"status\""),
            "missing bounded status JSON: {stdout}"
        );
        assert!(
            stdout.contains("\"state\":"),
            "missing lifecycle state: {stdout}"
        );
    }

    #[test]
    fn public_diagnose_process_returns_bounded_layered_json() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let script = root.join("catdesk.ps1");
        let output = Command::new("powershell.exe")
            .arg("-NoLogo")
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script)
            .arg("diagnose")
            .current_dir(root)
            .output()
            .expect("launch public lifecycle diagnose");

        assert!(
            output.status.success(),
            "diagnose process must carry lifecycle failure in JSON, not exit status: status={} stdout={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            output.stderr.is_empty(),
            "public diagnose must keep stderr empty"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        for required in [
            "\"command\":\"diagnose\"",
            "\"state\":",
            "\"primaryLayer\":",
            "\"nextAction\":",
            "\"layers\":",
        ] {
            assert!(
                stdout.contains(required),
                "missing bounded diagnose field {required}: {stdout}"
            );
        }
        assert!(
            !stdout.to_ascii_lowercase().contains("token=")
                && !stdout.to_ascii_lowercase().contains("https://"),
            "diagnose output must not expose credentials or routes: {stdout}"
        );
    }

    #[test]
    fn wake_installers_use_windows_powershell_compatible_utf8_without_bom() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for relative in ["wake/install.ps1", "wake/install-browser-runtime.ps1"] {
            assert_powershell_parses(root, relative);
            let source = std::fs::read_to_string(root.join(relative))
                .unwrap_or_else(|error| panic!("read {relative}: {error}"));
            assert!(
                !source.contains("utf8NoBOM"),
                "{relative} must not use the PowerShell 7-only utf8NoBOM encoding token"
            );
            assert!(
                source.contains("New-Object Text.UTF8Encoding($false)"),
                "{relative} must write UTF-8 without BOM through a Windows PowerShell-compatible .NET encoding"
            );
        }

        let browser_runtime_installer =
            std::fs::read_to_string(root.join("wake/install-browser-runtime.ps1"))
                .expect("read browser runtime installer");
        assert!(
            browser_runtime_installer.contains("[switch]$ResumeIncompleteInstall"),
            "browser runtime installer must require an explicit resume switch for an interrupted runtime"
        );
        assert!(
            browser_runtime_installer.contains("print(seleniumbase.__version__)"),
            "browser runtime verification must report the SeleniumBase version without embedding a quoted Python literal"
        );
        assert!(
            !browser_runtime_installer.contains("assert seleniumbase.__version__"),
            "browser runtime verification must not regress to the PowerShell-native-argument quote-stripping bug"
        );
        assert!(
            !browser_runtime_installer.contains("[IO.Path]::GetRelativePath"),
            "browser runtime installer must remain compatible with Windows PowerShell 5.1 / .NET Framework"
        );
        assert!(
            browser_runtime_installer.contains("ResumedIncompleteInstall=$resumedIncomplete"),
            "browser runtime installer must surface interrupted-install recovery in its audit output"
        );

        let adapter =
            std::fs::read_to_string(root.join("wake/adapter.py")).expect("read Wake adapter");
        assert!(
            adapter.contains("emit({\"stage\": \"READY\"})"),
            "Wake adapter must acknowledge browser-session readiness before event delivery can be claimed"
        );
        assert!(
            adapter.contains("BROWSER_SESSION_NOT_CREATED")
                && adapter.contains("BROWSER_DRIVER_START_FAILED")
                && adapter.contains("BROWSER_PROFILE_ACCESS_DENIED"),
            "Wake adapter startup failures must use bounded non-sensitive reason codes"
        );
        let wake_bridge = std::fs::read_to_string(root.join("scripts/wake_bridge.py"))
            .expect("read Wake browser primitives");
        assert!(
            wake_bridge.contains("def durable_receipt_round_trip(self, cdp: Any, *")
                && adapter.contains("sink.durable_receipt_round_trip(cdp)")
                && !adapter.contains("durable_receipt_round_trip(sb, cdp)")
                && !adapter.contains("cdp = sink.durable_receipt_round_trip"),
            "Wake adapter and reviewed browser primitives must retain the exact one-CDP durable-receipt API contract without replacing the live CDP handle with the primitive's unit return"
        );
        assert!(
            adapter.contains("open(os.devnull, \"w\")"),
            "Wake adapter must use Python's platform-safe null device even when __file__ uses a Windows extended path"
        );
        assert!(
            !adapter.contains("Path(__file__).anchor + \"NUL\""),
            "Wake adapter must not construct an invalid \\\\?\\C:\\NUL path from an extended-path script location"
        );

        let installer =
            std::fs::read_to_string(root.join("wake/install.ps1")).expect("read wake installer");
        let wake_manifest =
            std::fs::read_to_string(root.join("wake/Cargo.toml")).expect("read wake Cargo.toml");
        let wake_version = wake_manifest
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("version = \"")
                    .and_then(|value| value.strip_suffix('\"'))
            })
            .expect("Wake Cargo.toml must declare a package version");
        assert!(
            installer.contains("$wakeManifest = [IO.File]::ReadAllText($wakeManifestPath)")
                && installer
                    .contains("(?ms)^\\[package\\]\\s*.*?^version\\s*=\\s*\"([^\"]+)\"\\s*$")
                && installer.contains("$version = $versionMatch.Groups[1].Value")
                && installer.contains("$version -notmatch '^1\\.0\\.0-dev\\.\\d+$'"),
            "installer must derive and validate the installed Wake package version from the current immutable manifest"
        );
        assert!(
            installer.contains("publish-reviewed-install")
                && installer.contains("activate-reviewed-install")
                && installer.contains("PowerShell never owns package installation authority")
                && !installer.contains("[IO.File]::Open(")
                && !installer.contains("$installLock"),
            "Wake installer must stage inertly and delegate serialized publication plus activation authority to short-lived Rust processes"
        );
        assert!(
            installer.contains("AlreadyMaterialized = [bool]$publication.alreadyMaterialized")
                && installer.contains("Activation = $handoff"),
            "Wake installer must emit Rust publication and reviewed-activation handoff evidence"
        );
        assert!(
            installer.contains("FinalReleaseComObject($shortcut)")
                && installer.contains("FinalReleaseComObject($shell)")
                && installer.contains("WaitForExit(10000)")
                && installer.contains("$shortcutProcess.Kill()")
                && installer.contains("ShortcutState"),
            "presentation-only shortcut COM must be isolated in a bounded child process with explicit COM release and timeout reporting"
        );
        let wake_host_cli = std::fs::read_to_string(root.join("wake/src/bin/CatDeskWakeHost.rs"))
            .expect("read Wake host CLI");
        assert!(
            wake_host_cli
                .contains("[command] if command == \"start\" => runtime::start_installed(&store)?")
                && !wake_host_cli
                    .contains("[command] if command == \"start\" => runtime::start(&store)?"),
            "Wake CLI start must always launch the hash-verified immutable installed pointer rather than the caller executable"
        );
        let wake_runtime =
            std::fs::read_to_string(root.join("wake/src/runtime.rs")).expect("read Wake runtime");
        assert!(
            wake_runtime.contains("HOST_STOPPED_OWNER_EXIT_TIMEOUT")
                && wake_runtime.contains("for _ in 0..100"),
            "Wake start must give a STOPPED predecessor a bounded singleton-lease handoff window"
        );
        assert!(
            !wake_runtime.contains("Err(e) if e == \"HOST_ALREADY_RUNNING\" => return Ok(())"),
            "Wake start must not report success merely because a STOPPED predecessor still holds the host lease"
        );
        assert!(
            wake_runtime.contains("reviewed-install-handoff.json")
                && wake_runtime.contains("begin_or_resume_reviewed_install_handoff")
                && wake_runtime.contains("complete_reviewed_install_handoff")
                && wake_runtime.contains("HOST_INSTALL_HANDOFF_CONFLICT"),
            "reviewed Wake activation must durably preserve pre-stop desired state and fail closed on candidate drift"
        );
        assert!(
            installer.contains("$guiTarget = Join-Path $wakeSource 'target\\catdesk-gui'")
                && installer.contains("--target-dir $guiTarget"),
            "Wake installer must build the Binagotchy/CatDesk artifact in a dedicated target directory so a live CatDesk MCP executable cannot block the upgrade"
        );
        assert!(
            installer.contains("$binagotchyShortcut.Arguments = '--catdesk-binagotchy-cli'"),
            "normal installed Binagotchy shortcut must use the console shell"
        );
        assert!(
            wake_version.starts_with("1.0.0-dev."),
            "wake immutable release version must remain in the reviewed development release family"
        );
        let recovery = std::fs::read_to_string(root.join("scripts/start-catdesk-stack.ps1"))
            .expect("read recovery lifecycle script");
        assert!(
            recovery.contains("--catdesk-binagotchy-cli"),
            "interactive recovery must relaunch the console Binagotchy companion"
        );
        assert!(
            !recovery.contains("--catdesk-binagotchy-gui"),
            "normal recovery must not regress to the legacy white Win32 Binagotchy surface"
        );
    }
}
