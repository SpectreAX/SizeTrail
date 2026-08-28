#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

mod support;

use assert_cmd::cargo::cargo_bin_cmd;
use std::process::Command;
use std::sync::Mutex;
use support::{HighValueEntrySnapshot, ReadOnlyFixture};

static REAL_PATH_SNAPSHOT_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn scan_does_not_change_the_fixture_inside_or_outside_root() {
    let _real_path_guard = REAL_PATH_SNAPSHOT_LOCK
        .lock()
        .expect("real-path snapshot lock must be available");
    let fixture = ReadOnlyFixture::create().expect("fixture must be created");
    let before = fixture.snapshot().expect("baseline snapshot must succeed");
    let real_home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let high_value_before = HighValueEntrySnapshot::capture(real_home.as_deref())
        .expect("high-value baseline must succeed");

    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--root"])
        .arg(&fixture.home)
        .envs(fixture.environment())
        .output()
        .expect("full scan must run");
    assert!(matches!(output.status.code(), Some(0 | 3)));
    assert!(serde_json::from_slice::<serde_json::Value>(&output.stdout).is_ok());

    let after = fixture.snapshot().expect("final snapshot must succeed");
    let high_value_after = HighValueEntrySnapshot::capture(real_home.as_deref())
        .expect("high-value final snapshot must succeed");
    assert_eq!(before, after);
    assert!(
        high_value_after
            .new_entries_since(&high_value_before)
            .is_empty(),
        "scan created an entry in a high-value real path"
    );
}

#[test]
fn harness_detects_a_home_derived_write() {
    let fixture = ReadOnlyFixture::create().expect("fixture must be created");
    let before = fixture.snapshot().expect("baseline snapshot must succeed");

    let status = Command::new("/bin/sh")
        .args(["-c", "printf mutation > \"$HOME/.sizetrail-mutation\""])
        .envs(fixture.environment())
        .status()
        .expect("mutation subprocess must run");
    assert!(status.success());

    let after = fixture.snapshot().expect("final snapshot must succeed");
    assert_ne!(before, after, "HOME-derived mutation escaped the snapshot");
}

#[test]
fn high_value_fallback_detects_a_hard_coded_tmp_write() {
    let _real_path_guard = REAL_PATH_SNAPSHOT_LOCK
        .lock()
        .expect("real-path snapshot lock must be available");
    let before = HighValueEntrySnapshot::capture(None).expect("fallback baseline must succeed");
    let probe = std::path::PathBuf::from(format!(
        "/tmp/sizetrail-hard-coded-mutation-{}",
        std::process::id()
    ));
    std::fs::write(&probe, b"mutation").expect("hard-coded mutation must be written");

    let after =
        HighValueEntrySnapshot::capture(None).expect("fallback final snapshot must succeed");
    let new_entries = after.new_entries_since(&before);
    std::fs::remove_file(&probe).expect("hard-coded mutation must be removed");

    assert!(
        new_entries.contains(&probe),
        "hard-coded /tmp mutation escaped the fallback snapshot"
    );
}
