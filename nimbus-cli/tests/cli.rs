//! Smoke tests for the CLI surface. They only exercise argument parsing, so
//! they need no NetworkManager and touch nothing.

use std::process::Command;

fn help(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_nimbus"))
        .args(args)
        .output()
        .expect("nimbus should be runnable");
    assert!(
        output.status.success(),
        "`nimbus {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn help_lists_every_documented_command() {
    let help = help(&["--help"]);
    for command in [
        "start",
        "plan",
        "stop",
        "status",
        "devices",
        "stations",
        "scan",
        "interfaces",
        "qr",
    ] {
        assert!(
            help.contains(command),
            "`{command}` is missing from --help:\n{help}"
        );
    }
}

#[test]
fn start_help_documents_password_and_detach() {
    let help = help(&["start", "--help"]);
    assert!(help.contains("--password"));
    assert!(help.contains("--detach"));
    assert!(help.contains("--country"));
}

#[test]
fn plan_does_not_ask_for_a_password() {
    let help = help(&["plan", "--help"]);
    assert!(!help.contains("--password"));
}
