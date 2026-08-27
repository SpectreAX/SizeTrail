#![allow(clippy::disallowed_methods)]

mod support;

use assert_cmd::cargo::cargo_bin_cmd;
use sizetrail::policy::{InvocationTracker, ProbeId, ProbePolicy};
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

#[test]
fn registry_caps_declared_probes_and_rejects_undeclared_probes_before_invocation() {
    const DECLARED_ID: ProbeId = ProbeId::new("fixture.read_only");
    const UNDECLARED_ID: ProbeId = ProbeId::new("fixture.undeclared");
    const POLICIES: &[ProbePolicy] = &[ProbePolicy {
        id: DECLARED_ID,
        max_calls_per_scan: 1,
        disable_env: "SIZETRAIL_NO_FIXTURE_PROBE",
    }];

    let mut tracker = InvocationTracker::new(POLICIES);
    let mut actual_calls = 0;

    tracker
        .invoke(DECLARED_ID, || actual_calls += 1)
        .expect("the declared call is within its limit");
    assert!(tracker.invoke(DECLARED_ID, || actual_calls += 1).is_err());
    assert!(tracker.invoke(UNDECLARED_ID, || actual_calls += 1).is_err());

    assert_eq!(actual_calls, 1);
    assert_eq!(tracker.count(DECLARED_ID), 1);
    assert_eq!(tracker.count(UNDECLARED_ID), 0);
}
