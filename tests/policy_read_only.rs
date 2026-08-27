#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

mod support;

use assert_cmd::cargo::cargo_bin_cmd;
use std::process::Command;
use support::ReadOnlyFixture;

#[test]
fn scan_does_not_change_the_fixture_inside_or_outside_root() {
    let fixture = ReadOnlyFixture::create().expect("fixture must be created");
    let before = fixture.snapshot().expect("baseline snapshot must succeed");

    cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--root"])
        .arg(&fixture.home)
        .envs(fixture.environment())
        .assert()
        .success();

    let after = fixture.snapshot().expect("final snapshot must succeed");
    assert_eq!(before, after);
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
