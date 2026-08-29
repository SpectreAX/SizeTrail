#![allow(clippy::disallowed_methods)]

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

#[test]
fn bare_command_prints_help_without_running_a_probe() {
    let output = cargo_bin_cmd!("sizetrail")
        .env("SIZETRAIL_NO_XCODE_PROBE", "1")
        .output()
        .expect("help command must run");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("scan"));
}

#[test]
fn rules_json_is_the_compiled_toml_rule_set() {
    let output = cargo_bin_cmd!("sizetrail")
        .args(["rules", "--json"])
        .output()
        .expect("rules command must run");
    assert!(output.status.success());
    let rules: Value = serde_json::from_slice(&output.stdout).expect("rules stdout must be JSON");
    assert_eq!(rules.as_array().expect("rules must be an array").len(), 5);
    assert!(
        rules
            .as_array()
            .expect("rules must be an array")
            .iter()
            .all(|rule| rule.get("command").is_none()
                && rule["evidence"].as_str().is_some_and(|v| !v.is_empty()))
    );
}

#[test]
fn completion_prints_to_stdout_without_installing_anything() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let before = std::fs::read_dir(fixture.path())
        .expect("fixture must be readable")
        .count();
    let output = cargo_bin_cmd!("sizetrail")
        .args(["completion", "zsh"])
        .current_dir(fixture.path())
        .output()
        .expect("completion command must run");

    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
    assert_eq!(
        std::fs::read_dir(fixture.path())
            .expect("fixture must remain readable")
            .count(),
        before
    );
}

#[test]
fn explain_from_is_snapshot_only_and_validates_schema_and_id_versions() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let report = fixture.path().join("report.json");
    std::fs::write(
        &report,
        r#"{"schema_version":"0.1.0-unstable","environment":{"generated_at_unix_seconds":1800000000},"payload":{"findings":[{"id":"f1:xcode:0123456789abcdef","normalized_path":"~/Library/Developer/Xcode/Archives/Fixture"}]}}"#,
    )
    .expect("snapshot fixture must be written");

    let path = cargo_bin_cmd!("sizetrail")
        .args(["explain", "f1:xcode:0123456789abcdef", "--path", "--from"])
        .arg(&report)
        .output()
        .expect("snapshot explain must run");
    assert!(path.status.success());
    assert_eq!(
        String::from_utf8(path.stdout)
            .expect("path must be UTF-8")
            .trim(),
        "~/Library/Developer/Xcode/Archives/Fixture"
    );

    let wrong_id = cargo_bin_cmd!("sizetrail")
        .args(["explain", "f2:xcode:0123456789abcdef", "--from"])
        .arg(&report)
        .output()
        .expect("unknown id explain must run");
    assert!(!wrong_id.status.success());

    let unknown_schema = report.with_file_name("future.json");
    std::fs::write(
        &unknown_schema,
        r#"{"schema_version":"9.0.0","environment":{},"payload":{"findings":[]}}"#,
    )
    .expect("future fixture must be written");
    let future = cargo_bin_cmd!("sizetrail")
        .args(["explain", "f1:xcode:0123456789abcdef", "--from"])
        .arg(&unknown_schema)
        .output()
        .expect("future-schema explain must run");
    assert!(!future.status.success());
}

#[test]
fn doctor_can_skip_xcode_without_starting_coresimulator() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let output = cargo_bin_cmd!("sizetrail")
        .args(["doctor", "--json", "--no-xcode", "--root"])
        .arg(fixture.path())
        .output()
        .expect("doctor must run");
    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("doctor stdout must be JSON");
    assert_eq!(json["xcode"]["status"], "excluded_by_user");
    assert!(json.get("full_disk_access").is_none());
    assert_eq!(
        json["side_effect_policy"]
            .as_array()
            .expect("registry must be an array")
            .len(),
        6
    );
    assert_eq!(
        json["side_effect_policy"][4]["known_side_effects"]
            .as_array()
            .expect("simctl side effects must be typed")
            .len(),
        1
    );
}

#[test]
fn doctor_labels_term_program_as_an_unverified_hint() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let output = cargo_bin_cmd!("sizetrail")
        .args(["doctor", "--json", "--no-xcode", "--root"])
        .arg(fixture.path())
        .env("TERM_PROGRAM", "FixtureTerminal")
        .output()
        .expect("doctor must run");

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).expect("doctor stdout must be JSON");
    assert_eq!(json["launcher_hint"]["candidate"], "FixtureTerminal");
    assert_eq!(json["launcher_hint"]["confidence"], "unverified");
    assert!(json.get("full_disk_access").is_none());
}

#[test]
fn doctor_reports_probe_gate_and_prints_but_never_executes_settings_advice() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let before = std::fs::read_dir(fixture.path())
        .expect("fixture must be readable")
        .count();
    let output = cargo_bin_cmd!("sizetrail")
        .args(["doctor", "--json", "--root"])
        .arg(fixture.path())
        .env("SIZETRAIL_NO_XCODE_PROBE", "1")
        .output()
        .expect("doctor must run");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(&output.stdout).expect("doctor stdout must be JSON");
    assert_eq!(json["xcode"]["status"], "unmeasurable");
    assert_eq!(json["xcode"]["coverage_gaps"][0]["reason"], "disabled");
    assert_eq!(json["remediation"]["execution"], "user_only");
    assert!(
        json["remediation"]["settings_command"]
            .as_str()
            .expect("settings advice must be text")
            .starts_with("open ")
    );
    assert_eq!(
        std::fs::read_dir(fixture.path())
            .expect("fixture must remain readable")
            .count(),
        before
    );
}
