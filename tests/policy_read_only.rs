#![allow(clippy::disallowed_methods)]

mod support;

use assert_cmd::cargo::cargo_bin_cmd;
use support::ReadOnlyFixture;

#[test]
fn scan_does_not_change_the_fixture_inside_or_outside_root() {
    let fixture = ReadOnlyFixture::create().expect("fixture must be created");
    let before = fixture.snapshot().expect("baseline snapshot must succeed");

    cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--root"])
        .arg(&fixture.home)
        .assert()
        .success();

    let after = fixture.snapshot().expect("final snapshot must succeed");
    assert_eq!(before, after);
}
