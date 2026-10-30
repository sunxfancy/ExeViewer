#[test]
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn diagnostic_scenarios() {
    let status = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/diagnose_scenarios.py"
        ))
        .arg(env!("CARGO_BIN_EXE_ExeViewer"))
        .status()
        .expect("diagnostic fixtures require python3, gcc, and glibc on Linux x86-64");
    assert!(status.success(), "diagnostic scenario suite failed");
}
