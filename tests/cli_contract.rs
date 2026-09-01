#![allow(clippy::disallowed_methods)]

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::os::unix::fs::symlink;

fn write_fixture(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("fixture parent must be created");
    }
    std::fs::write(path, contents).expect("fixture file must be written");
}

fn homebrew_fixture() -> tempfile::TempDir {
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    write_fixture(&fixture.path().join("opt/homebrew/bin/brew"), "fixture");
    write_fixture(
        &fixture
            .path()
            .join("opt/homebrew/Cellar/example/1.0/bin/example"),
        "installed",
    );
    write_fixture(
        &fixture.path().join("opt/homebrew/.git/HEAD"),
        "ref: refs/heads/stable\n",
    );
    write_fixture(
        &fixture.path().join("opt/homebrew/.git/refs/heads/stable"),
        SHA,
    );
    write_fixture(
        &fixture
            .path()
            .join(format!("opt/homebrew/.git/describe-cache/{SHA}")),
        "6.0.19\n",
    );
    fixture
}

fn write_default_homebrew_cache_dirs(root: &std::path::Path) {
    for relative in [
        "Library/Caches/Homebrew/downloads",
        "Library/Caches/Homebrew/api",
        "Library/Caches/Homebrew/api-source",
        "Library/Caches/Homebrew/bootsnap",
        "Library/Caches/Homebrew/cargo_cache",
        "Library/Caches/Homebrew/go_cache",
        "Library/Caches/Homebrew/go_mod_cache",
        "Library/Caches/Homebrew/glide_home",
        "Library/Caches/Homebrew/java_cache",
        "Library/Caches/Homebrew/npm_cache",
        "Library/Caches/Homebrew/pip_cache",
        "Library/Caches/Homebrew/gclient_cache",
    ] {
        std::fs::create_dir_all(root.join(relative)).expect("cache fixture must be created");
    }
}

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
    assert!(
        complete_json["payload"]["regions"]
            .as_array()
            .expect("regions must be an array")
            .iter()
            .any(|region| region["id"] == "xcode" && region["status"] == "excluded_by_user")
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

/// Q54: a staged app cask is a declared scope boundary, not an environmental failure.
/// The host that motivated this decision exits 3 solely because eight such casks exist.
#[test]
fn a_cask_moved_outside_the_prefix_does_not_make_the_scan_incomplete() {
    let fixture = homebrew_fixture();
    write_default_homebrew_cache_dirs(fixture.path());
    let cask = fixture.path().join("opt/homebrew/Caskroom/zed/1.15.0");
    std::fs::create_dir_all(&cask).expect("cask fixture must be created");
    symlink("/Applications/Zed.app", cask.join("Zed.app"))
        .expect("staged app link must be created");

    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--no-docker", "--root"])
        .arg(fixture.path())
        .output()
        .expect("Homebrew cask scan must run");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value =
        serde_json::from_slice(&output.stdout).expect("scan must emit one JSON document");
    assert!(
        document["payload"]["regions"]
            .as_array()
            .expect("regions must be an array")
            .iter()
            .any(|region| region["id"] == "homebrew" && region["status"] == "complete"),
        "homebrew region: {}",
        document["payload"]["regions"]
    );
    assert!(
        document["payload"]["coverage_gaps"]
            .as_array()
            .expect("coverage gaps must be an array")
            .iter()
            .any(|gap| {
                gap["reason"] == "cask_artifact_outside_prefix"
                    && gap["status"] == "declared_scope_boundary"
            }),
        "coverage gaps: {}",
        document["payload"]["coverage_gaps"]
    );
    assert!(
        document["payload"]["coverage_gaps"]
            .as_array()
            .expect("coverage gaps must be an array")
            .iter()
            .all(|gap| gap["status"] != "unmeasurable"),
        "an environmental unmeasurable would hide the Q54 mapping"
    );
}

#[test]
fn a_missing_homebrew_cache_root_is_a_declared_boundary_not_an_incomplete_scan() {
    let fixture = homebrew_fixture();
    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--no-docker", "--root"])
        .arg(fixture.path())
        .output()
        .expect("Homebrew scan without cache root must run");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value =
        serde_json::from_slice(&output.stdout).expect("scan must emit one JSON document");
    assert!(
        document["payload"]["regions"]
            .as_array()
            .expect("regions must be an array")
            .iter()
            .any(|region| region["id"] == "homebrew" && region["status"] == "complete")
    );
    assert!(
        document["payload"]["coverage_gaps"]
            .as_array()
            .expect("coverage gaps must be an array")
            .iter()
            .any(|gap| {
                gap["reason"] == "unsupported_path_override"
                    && gap["status"] == "declared_scope_boundary"
            })
    );
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
    assert_eq!(document["schema_version"], sizetrail::model::SCHEMA_VERSION);

    let doctor = cargo_bin_cmd!("sizetrail")
        .args(["doctor", "--json", "--no-xcode"])
        .output()
        .expect("doctor must run");
    let diagnosis: Value =
        serde_json::from_slice(&doctor.stdout).expect("doctor must emit a JSON document");
    assert_eq!(diagnosis["tool_version"], expected);
}

#[test]
fn no_homebrew_is_an_explicit_successful_exclusion() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let output = cargo_bin_cmd!("sizetrail")
        .args([
            "scan",
            "--json",
            "--no-xcode",
            "--no-homebrew",
            "--no-docker",
            "--root",
        ])
        .arg(fixture.path())
        .output()
        .expect("excluded scan must run");

    assert_eq!(output.status.code(), Some(0));
    let document: Value =
        serde_json::from_slice(&output.stdout).expect("scan must emit one JSON document");
    assert!(
        document["payload"]["regions"]
            .as_array()
            .expect("regions must be an array")
            .iter()
            .any(|region| { region["id"] == "homebrew" && region["status"] == "excluded_by_user" })
    );
}

#[test]
fn root_fixture_discovers_homebrew_without_running_brew() {
    let fixture = homebrew_fixture();
    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--no-docker", "--root"])
        .arg(fixture.path())
        .output()
        .expect("Homebrew fixture scan must run");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value =
        serde_json::from_slice(&output.stdout).expect("scan must emit one JSON document");
    assert_eq!(
        document["environment"]["tool_versions"]["homebrew"],
        "6.0.19"
    );
    assert!(
        document["payload"]["findings"]
            .as_array()
            .expect("findings must be an array")
            .iter()
            .any(|finding| {
                finding["rule_id"] == "homebrew.cellar"
                    && finding["id"]
                        .as_str()
                        .is_some_and(|id| id.starts_with("f1:homebrew:"))
                    && finding["subject"]["normalized_path"] == "/opt/homebrew/Cellar/example/1.0"
            })
    );
}

#[test]
fn live_explain_rescans_only_the_homebrew_owner() {
    let fixture = homebrew_fixture();
    let scan = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--no-docker", "--root"])
        .arg(fixture.path())
        .output()
        .expect("Homebrew fixture scan must run");
    let document: Value =
        serde_json::from_slice(&scan.stdout).expect("scan must emit one JSON document");
    let id = document["payload"]["findings"]
        .as_array()
        .expect("findings must be an array")
        .iter()
        .find(|finding| finding["adapter_id"] == "homebrew")
        .and_then(|finding| finding["id"].as_str())
        .expect("Homebrew finding must have an id");

    let explain = cargo_bin_cmd!("sizetrail")
        .args(["explain", id, "--json", "--root"])
        .arg(fixture.path())
        .env("SIZETRAIL_NO_XCODE_PROBE", "1")
        .output()
        .expect("live explain must run");

    assert!(explain.status.success());
    let explanation: Value =
        serde_json::from_slice(&explain.stdout).expect("explain must emit JSON");
    assert_eq!(explanation["provenance"], "live");
    assert_eq!(explanation["finding"]["id"], id);
}

#[test]
fn exact_homebrew_prefix_exclusion_is_applied_before_inventory() {
    let fixture = homebrew_fixture();
    let keg = "/opt/homebrew/Cellar/example/1.0";
    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--no-docker", "--exclude"])
        .arg(keg)
        .arg("--root")
        .arg(fixture.path())
        .output()
        .expect("Homebrew exclusion scan must run");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value =
        serde_json::from_slice(&output.stdout).expect("scan must emit one JSON document");
    assert!(
        document["payload"]["findings"]
            .as_array()
            .expect("findings must be an array")
            .iter()
            .all(|finding| {
                finding["subject"]["normalized_path"] != "/opt/homebrew/Cellar/example/1.0"
            })
    );
    assert!(
        document["payload"]["coverage_gaps"]
            .as_array()
            .expect("coverage gaps must be an array")
            .iter()
            .any(|gap| gap["region"] == "homebrew" && gap["reason"] == "excluded_by_user")
    );
}

#[test]
fn doctor_reports_homebrew_user_exclusion() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let output = cargo_bin_cmd!("sizetrail")
        .args([
            "doctor",
            "--json",
            "--no-xcode",
            "--no-homebrew",
            "--no-docker",
            "--root",
        ])
        .arg(fixture.path())
        .output()
        .expect("doctor must run");

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout).expect("doctor must emit JSON");
    assert_eq!(document["homebrew"]["status"], "excluded_by_user");
}

fn write_default_docker_raw(root: &std::path::Path) -> std::path::PathBuf {
    let disk = root.join("Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw");
    write_fixture(&disk, "");
    disk
}

#[test]
fn no_docker_is_an_explicit_successful_exclusion() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let output = cargo_bin_cmd!("sizetrail")
        .args([
            "scan",
            "--json",
            "--no-xcode",
            "--no-homebrew",
            "--no-docker",
            "--root",
        ])
        .arg(fixture.path())
        .output()
        .expect("excluded Docker scan must run");

    assert_eq!(output.status.code(), Some(0));
    let document: Value =
        serde_json::from_slice(&output.stdout).expect("scan must emit one JSON document");
    assert!(
        document["payload"]["regions"]
            .as_array()
            .expect("regions must be an array")
            .iter()
            .any(|region| region["id"] == "docker" && region["status"] == "excluded_by_user")
    );
}

#[test]
fn doctor_reports_docker_user_exclusion() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let output = cargo_bin_cmd!("sizetrail")
        .args([
            "doctor",
            "--json",
            "--no-xcode",
            "--no-homebrew",
            "--no-docker",
            "--root",
        ])
        .arg(fixture.path())
        .output()
        .expect("doctor must run");

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout).expect("doctor must emit JSON");
    assert_eq!(document["docker"]["status"], "excluded_by_user");
}

#[test]
fn docker_virtual_disk_is_measured_without_the_daemon() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    write_default_docker_raw(fixture.path());
    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--no-homebrew", "--root"])
        .arg(fixture.path())
        .output()
        .expect("Docker disk scan must run");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value =
        serde_json::from_slice(&output.stdout).expect("scan must emit one JSON document");
    assert_eq!(
        document["payload"]["regions"]
            .as_array()
            .expect("regions")
            .iter()
            .find(|region| region["id"] == "docker")
            .expect("docker region")["status"],
        "not_present"
    );
    assert!(
        document["payload"]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding["rule_id"] == "docker.virtual_disk")
    );
}

#[test]
fn exact_docker_disk_exclusion_is_applied_before_inventory() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let disk = write_default_docker_raw(fixture.path());
    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--no-homebrew", "--exclude"])
        .arg(&disk)
        .arg("--root")
        .arg(fixture.path())
        .output()
        .expect("Docker exclusion scan must run");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value =
        serde_json::from_slice(&output.stdout).expect("scan must emit one JSON document");
    assert!(
        document["payload"]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .all(|finding| finding["rule_id"] != "docker.virtual_disk")
    );
    assert!(
        document["payload"]["coverage_gaps"]
            .as_array()
            .expect("coverage gaps")
            .iter()
            .any(|gap| gap["region"] == "docker" && gap["reason"] == "excluded_by_user")
    );
}

#[test]
fn exact_custom_docker_data_folder_exclusion_is_applied_before_inventory() {
    let temporary = tempfile::tempdir().expect("fixture container must be created");
    let home = temporary.path().join("home");
    std::fs::create_dir(&home).expect("fixture HOME must be created");
    let home = std::fs::canonicalize(home).expect("fixture HOME must canonicalize");
    let data_folder = temporary.path().join("DockerData");
    write_fixture(&data_folder.join("Docker.raw"), "");
    let data_folder = std::fs::canonicalize(&data_folder).expect("data folder must canonicalize");
    let physical_disk = data_folder.join("Docker.raw");
    write_fixture(
        &home.join("Library/Group Containers/group.com.docker/settings-store.json"),
        &serde_json::json!({"DataFolder": data_folder}).to_string(),
    );
    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--no-homebrew", "--exclude"])
        .arg(&physical_disk)
        .arg("--root")
        .arg(&home)
        .output()
        .expect("custom DataFolder exclusion scan must run");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value =
        serde_json::from_slice(&output.stdout).expect("scan must emit one JSON document");
    assert!(
        document["payload"]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .all(|finding| finding["rule_id"] != "docker.virtual_disk")
    );
    assert!(
        document["payload"]["coverage_gaps"]
            .as_array()
            .expect("coverage gaps")
            .iter()
            .any(|gap| gap["region"] == "docker" && gap["reason"] == "excluded_by_user")
    );
}

#[test]
fn live_explain_rescans_only_the_docker_owner() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    write_default_docker_raw(fixture.path());
    let scan = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--no-homebrew", "--root"])
        .arg(fixture.path())
        .output()
        .expect("Docker disk scan must run");
    let document: Value =
        serde_json::from_slice(&scan.stdout).expect("scan must emit one JSON document");
    let id = document["payload"]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| finding["rule_id"] == "docker.virtual_disk")
        .and_then(|finding| finding["id"].as_str())
        .expect("virtual disk finding must have an id");

    let explain = cargo_bin_cmd!("sizetrail")
        .args(["explain", id, "--json", "--root"])
        .arg(fixture.path())
        .env("SIZETRAIL_NO_XCODE_PROBE", "1")
        .output()
        .expect("live explain must run");

    assert!(explain.status.success());
    let explanation: Value =
        serde_json::from_slice(&explain.stdout).expect("explain must emit JSON");
    assert_eq!(explanation["provenance"], "live");
    assert_eq!(explanation["finding"]["id"], id);
    assert_eq!(explanation["finding"]["rule_id"], "docker.virtual_disk");
}

#[test]
fn live_explain_path_rejects_a_docker_object_set() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let report = fixture.path().join("report.json");
    std::fs::write(
        &report,
        r#"{"schema_version":"1.0.0","environment":{"generated_at_unix_seconds":1800000000},"payload":{"findings":[{"id":"f1:docker:0123456789abcdef","subject":{"kind":"toolchain_object_set","object_set_id":"docker.images"}}]}}"#,
    )
    .expect("object-set report must be written");
    let output = cargo_bin_cmd!("sizetrail")
        .args(["explain", "f1:docker:0123456789abcdef", "--path", "--from"])
        .arg(&report)
        .output()
        .expect("object-set explain must run");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("finding has no filesystem path"));
}
