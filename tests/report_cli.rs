use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ExeViewer"))
        .args(args)
        .output()
        .expect("run CLI")
}

#[test]
fn report_is_plain_text_and_does_not_claim_compatibility() {
    let output = run(&["--report", env!("CARGO_BIN_EXE_ExeViewer")]);
    assert!(output.status.success(), "{:?}", output);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("SHA-256:"));
    assert!(text.contains("NEEDED:"));
    assert!(text.contains("Compatibility: NOT ASSESSED"));
    assert!(!text.contains('\x1b'));
    assert!(output.stderr.is_empty());
}

#[test]
fn redirected_stdout_automatically_selects_report() {
    let explicit = run(&["--report", env!("CARGO_BIN_EXE_ExeViewer")]);
    let automatic = run(&[env!("CARGO_BIN_EXE_ExeViewer")]);
    assert!(automatic.status.success());
    assert_eq!(explicit.stdout, automatic.stdout);
}

#[test]
fn non_elf_is_an_error_without_partial_report_or_panic() {
    let output = run(&["--report", concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!error.is_empty());
    assert!(!error.contains("panicked"));
}

#[test]
fn repository_sample_can_be_inspected() {
    let output = run(&["--report", concat!(env!("CARGO_MANIFEST_DIR"), "/test-program/slip")]);
    assert!(output.status.success(), "{:?}", output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Compatibility: NOT ASSESSED"));
}

#[test]
fn minimal_static_elf_does_not_require_sections_or_dynamic_tables() {
    let mut bytes = [0u8; 64];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[58..60].copy_from_slice(&64u16.to_le_bytes());
    let path = std::env::temp_dir().join(format!("exeviewer-static-{}.elf", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    let output = run(&["--report", path.to_str().unwrap()]);
    std::fs::remove_file(path).unwrap();
    assert!(output.status.success(), "{:?}", output);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("No dynamic table."));
    assert!(text.contains("Interpreter: not declared"));
}

#[test]
fn interactive_mode_requires_terminal_and_conflicts_with_report() {
    for args in [
        vec!["--interactive", env!("CARGO_BIN_EXE_ExeViewer")],
        vec!["--interactive", "--report", env!("CARGO_BIN_EXE_ExeViewer")],
    ] {
        let output = run(&args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}
