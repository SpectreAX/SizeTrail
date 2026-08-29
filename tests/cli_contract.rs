#![allow(clippy::disallowed_methods)]

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;

#[test]
fn exit_codes_distinguish_complete_fatal_usage_and_informational_scans() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");

    let complete = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--root"])
        .arg(fixture.path())
        .output()
        .expect("complete scan must run");
    assert_eq!(
        complete.status.code(),
        Some(0),
        "complete scan stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&complete.stdout),
        String::from_utf8_lossy(&complete.stderr)
    );
    let complete_json: Value =
        serde_json::from_slice(&complete.stdout).expect("complete stdout must be JSON");
    assert_eq!(
        complete_json["payload"]["regions"][1]["status"],
        "excluded_by_user"
    );

    let informational = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--root"])
        .arg(fixture.path())
        .env("SIZETRAIL_NO_XCODE_PROBE", "1")
        .output()
        .expect("informational scan must run");
    assert_eq!(informational.status.code(), Some(3));
    assert!(serde_json::from_slice::<Value>(&informational.stdout).is_ok());

    let usage = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--exclude", "missing", "--root"])
        .arg(fixture.path())
        .output()
        .expect("usage-error scan must run");
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());

    let fatal = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json"])
        .env_remove("HOME")
        .output()
        .expect("fatal scan must run");
    assert_eq!(fatal.status.code(), Some(1));
    assert!(fatal.stdout.is_empty());
}

#[test]
fn exact_existing_exclude_is_reported_without_becoming_an_error() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let excluded = fixture.path().join("Library/Developer/Xcode/DerivedData");
    std::fs::create_dir_all(&excluded).expect("excluded fixture must be created");
    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--exclude"])
        .arg(&excluded)
        .args(["--root"])
        .arg(fixture.path())
        .env("SIZETRAIL_NO_XCODE_PROBE", "1")
        .output()
        .expect("excluded scan must run");

    assert_eq!(output.status.code(), Some(3));
    let json: Value = serde_json::from_slice(&output.stdout).expect("stdout must be JSON");
    assert!(
        json["payload"]["coverage_gaps"]
            .as_array()
            .expect("coverage gaps must be an array")
            .iter()
            .any(|gap| gap["status"] == "excluded_by_user")
    );
}

/// Q48: a published tarball must be able to say which build it is. Without this, the v0.1.0 defect
/// notice telling holders to upgrade is unactionable, and bug reports arrive without a build.
#[test]
fn the_binary_reports_its_build_version_on_the_cli_and_in_json() {
    let expected = env!("CARGO_PKG_VERSION");

    let flag = cargo_bin_cmd!("sizetrail")
        .arg("--version")
        .output()
        .expect("version flag must run");
    assert!(flag.status.success(), "--version must exit cleanly");
    let printed = String::from_utf8_lossy(&flag.stdout);
    assert!(
        printed.contains(expected),
        "--version printed {printed:?} without the build version"
    );

    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let scan = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--root"])
        .arg(fixture.path())
        .output()
        .expect("scan must run");
    let document: Value =
        serde_json::from_slice(&scan.stdout).expect("scan must emit a JSON document");
    assert_eq!(document["tool_version"], expected);
    assert_ne!(
        document["tool_version"], document["schema_version"],
        "the build version must be distinguishable from the schema version"
    );

    let doctor = cargo_bin_cmd!("sizetrail")
        .args(["doctor", "--json", "--no-xcode"])
        .output()
        .expect("doctor must run");
    let diagnosis: Value =
        serde_json::from_slice(&doctor.stdout).expect("doctor must emit a JSON document");
    assert_eq!(diagnosis["tool_version"], expected);
}
