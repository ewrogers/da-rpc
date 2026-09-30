use std::process::Command;

fn invalid_command(filter: Option<&str>) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_darpcd"));
    command.arg("--invalid-option").env_remove("RUST_LOG");
    if let Some(filter) = filter {
        command.env("RUST_LOG", filter);
    }
    command.output().expect("daemon should start")
}

#[test]
fn diagnostics_have_a_timestamp_level_and_fields_on_stderr() {
    let output = invalid_command(None);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    let line = stderr.lines().next().unwrap();
    assert!(
        line.contains("Z ERROR darpcd: invalid command line error="),
        "{stderr}"
    );
    assert!(stderr.contains("usage: darpcd"));
    assert!(!stderr.contains('\u{1b}'));
}

#[test]
fn invalid_filter_fails_startup_and_off_suppresses_diagnostics() {
    let output = invalid_command(Some("darpcd=bogus"));
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("invalid RUST_LOG filter")
    );

    let output = invalid_command(Some("off"));
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("usage: darpcd"));
    assert!(!stderr.contains("ERROR"));
}
