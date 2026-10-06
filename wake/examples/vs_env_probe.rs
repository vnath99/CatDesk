use std::env;
fn main() {
    for k in [
        "VSINSTALLDIR",
        "VCINSTALLDIR",
        "VCToolsInstallDir",
        "VCToolsVersion",
        "WindowsSdkDir",
        "WindowsSDKVersion",
        "UniversalCRTSdkDir",
        "UCRTVersion",
        "Platform",
        "PlatformTarget",
        "VisualStudioVersion",
    ] {
        match env::var(k) {
            Ok(v) => println!("{k}={v}"),
            Err(_) => println!("{k}=<ABSENT>"),
        }
    }
}
