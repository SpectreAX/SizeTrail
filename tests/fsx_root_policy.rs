#![allow(clippy::disallowed_methods)]

use std::fs;
use std::path::{Path, PathBuf};

use sizetrail::fsx::{Root, RootError};

fn symlinked_temp_root() -> PathBuf {
    let root = Path::new("/tmp");
    assert!(
        fs::symlink_metadata(root)
            .expect("/tmp must exist")
            .is_symlink(),
        "this test requires the stock macOS layout where /tmp is a symlink"
    );
    root.to_path_buf()
}

fn make_dir_under(base: &Path, name: &str) -> PathBuf {
    let path = base.join(name);
    fs::create_dir_all(&path).expect("fixture directory must be created");
    path
}

#[test]
fn a_root_reached_through_a_symlinked_ancestor_is_accepted() {
    let base = symlinked_temp_root();
    let given = make_dir_under(&base, "sizetrail-root-policy-accept");

    let root = Root::open(&given).expect("a root under /tmp must open after canonicalization");

    assert_eq!(
        root.path(),
        fs::canonicalize(&given)
            .expect("fixture must have a physical path")
            .as_path(),
        "the root must retain its physical path, not the symlinked spelling"
    );
    fs::remove_dir_all(&given).expect("fixture cleanup");
}

#[test]
fn symlinked_and_physical_spellings_of_one_root_share_an_identity() {
    let base = symlinked_temp_root();
    let given = make_dir_under(&base, "sizetrail-root-policy-identity");
    let physical = fs::canonicalize(&given).expect("fixture must have a physical path");

    let through_symlink = Root::open(&given).expect("symlinked spelling must open");
    let through_physical = Root::open(&physical).expect("physical spelling must open");

    assert_eq!(through_symlink.identity(), through_physical.identity());
    fs::remove_dir_all(&given).expect("fixture cleanup");
}

#[test]
fn capacity_is_measurable_for_a_root_given_through_a_symlink() {
    let base = symlinked_temp_root();
    let given = make_dir_under(&base, "sizetrail-root-policy-capacity");

    let root = Root::open(&given).expect("symlinked spelling must open");
    let values = root.capacity().expect("capacity must be queryable");

    assert!(
        !values.is_empty(),
        "a measurable root must produce capacity values"
    );
    fs::remove_dir_all(&given).expect("fixture cleanup");
}

#[test]
fn objects_below_a_symlinked_root_are_measured_through_the_physical_path() {
    let base = symlinked_temp_root();
    let given = make_dir_under(&base, "sizetrail-root-policy-object");
    fs::write(given.join("payload.bin"), vec![0x11; 4096]).expect("payload must be written");

    let root = Root::open(&given).expect("symlinked spelling must open");
    let measured = root
        .measure_object(&root.path().join("payload.bin"))
        .expect("an object under the physical root must be measurable");

    assert_eq!(measured.logical_bytes, 4096);
    fs::remove_dir_all(&given).expect("fixture cleanup");
}

#[test]
fn a_missing_root_is_reported_as_unresolvable_not_as_a_policy_failure() {
    let error = Root::open(Path::new("/nonexistent-sizetrail-root-policy"))
        .expect_err("a missing root must fail");

    assert_eq!(
        error,
        RootError::PathUnresolvable,
        "a missing root must be distinguishable from an I/O policy failure"
    );
}

#[test]
fn a_relative_root_is_rejected_before_any_filesystem_call() {
    let error = Root::open(Path::new("relative/root")).expect_err("a relative root must fail");

    assert_eq!(error, RootError::NotNormalizedAbsolute);
}

/// Redline 6 requires an I/O policy failure to mark the whole root unknown. That requirement is
/// only auditable if the reason survives into the machine output distinctly, so no other root
/// failure may share its serialized name.
#[test]
fn every_root_failure_serializes_to_a_distinct_reason() {
    let errors = [
        RootError::NotNormalizedAbsolute,
        RootError::CloudRootExcluded,
        RootError::ReadPolicyVerificationFailed,
        RootError::PathUnresolvable,
        RootError::PathNotEncodable,
        RootError::IdentityUnavailable,
        RootError::SymlinkTraversalRejected,
    ];

    let names: Vec<String> = errors
        .iter()
        .map(|error| serde_json::to_string(&error.reason()).expect("a reason must serialize"))
        .collect();
    let unique: std::collections::BTreeSet<&String> = names.iter().collect();

    assert_eq!(
        unique.len(),
        errors.len(),
        "root failure reasons collapsed onto each other: {names:?}"
    );
    assert!(
        names.contains(&"\"read_policy_verification_failed\"".to_owned()),
        "the materialization guard failure must be identifiable in JSON: {names:?}"
    );
}
