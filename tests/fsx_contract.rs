#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

#[allow(dead_code)]
mod support;

use std::fs::{self, File};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

use sizetrail::fsx::{CapacityBasis, CapacityValue, Root};
use support::ReadOnlyFixture;

fn physical_root(fixture: &tempfile::TempDir) -> std::path::PathBuf {
    fs::canonicalize(fixture.path()).expect("fixture root must have a physical path")
}

fn run_c_oracle(path: &Path, volume: bool) -> String {
    let build = tempfile::tempdir().expect("oracle build directory must be created");
    let executable = build.path().join("probe_attrs");
    let compiled = Command::new("xcrun")
        .args(["clang", "-Wall", "-Wextra", "-Werror"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("probe_attrs.c"))
        .arg("-o")
        .arg(&executable)
        .status()
        .expect("C oracle compiler must run");
    assert!(compiled.success(), "C oracle must compile");
    let mut command = Command::new(executable);
    if volume {
        command.arg("--volume");
    }
    let output = command.arg(path).output().expect("C oracle must run");
    assert!(output.status.success(), "C oracle must succeed");
    String::from_utf8(output.stdout).expect("C oracle output must be UTF-8")
}

fn c_oracle(path: &Path) -> [Option<u64>; 5] {
    let line = run_c_oracle(path, false);

    let field = |name: &str| {
        line.split_whitespace()
            .find_map(|part| part.strip_prefix(name))
            .map(|value| {
                if let Some(hex) = value.strip_prefix("0x") {
                    u64::from_str_radix(hex, 16).expect("hex oracle value must parse")
                } else {
                    value.parse().expect("decimal oracle value must parse")
                }
            })
    };

    [
        field("alloc="),
        field("data="),
        field("rsrc="),
        field("private="),
        field("extflags="),
    ]
}

fn c_volume_oracle(path: &Path) -> [u64; 8] {
    let line = run_c_oracle(path, true);
    let field = |name: &str| {
        line.split_whitespace()
            .find_map(|part| part.strip_prefix(name))
            .expect("oracle field must exist")
            .parse()
            .expect("oracle field must be an integer")
    };
    [
        field("size="),
        field("free="),
        field("available="),
        field("used="),
        field("block_size="),
        field("blocks="),
        field("blocks_free="),
        field("blocks_available="),
    ]
}

#[test]
fn read_only_wrappers_measure_a_fixture_without_changing_it() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let root_path = physical_root(&fixture);
    let path = root_path.join("artifact.bin");
    fs::write(&path, vec![0x5a; 4096]).expect("fixture file must be written");
    let before = fs::symlink_metadata(&path).expect("baseline metadata must be readable");

    let root = Root::open(&root_path).expect("fixture root must initialize");
    let measured = root
        .measure_object(&path)
        .expect("fixture object must be measurable");
    let capacity = root
        .capacity()
        .expect("fixture capacity must be measurable");
    let _has_snapshots = root
        .volume_has_snapshots()
        .expect("the mounted APFS fixture volume must expose snapshot state");

    assert_eq!(measured.logical_bytes, 4096);
    assert_eq!(measured.identity.fsid, root.identity().fsid);
    assert_eq!(measured.identity.fileid, before.ino());
    assert!(measured.allocated_bytes.is_some());
    assert!(capacity.iter().all(|value| matches!(
        value,
        CapacityValue::Measured { basis, .. }
            if matches!(
                basis,
                CapacityBasis::AttrVolSize
                    | CapacityBasis::AttrVolSpaceUsed
                    | CapacityBasis::AttrVolSpaceFree
                    | CapacityBasis::AttrVolSpaceAvailable
                    | CapacityBasis::StatfsBlocks
                    | CapacityBasis::StatfsBlocksMinusFree
                    | CapacityBasis::StatfsFreeBlocks
                    | CapacityBasis::StatfsAvailableBlocks
                    | CapacityBasis::CoreFoundationImportantUsage
                    | CapacityBasis::CoreFoundationOpportunisticUsage
            )
    ) || matches!(value, CapacityValue::Unmeasurable { .. })));

    let after = fs::symlink_metadata(&path).expect("final metadata must be readable");
    assert_eq!(before.ino(), after.ino());
    assert_eq!(before.len(), after.len());
    assert_eq!(before.modified().ok(), after.modified().ok());
}

#[test]
fn root_lists_children_in_stable_order_without_changing_the_tree() {
    let fixture = ReadOnlyFixture::create().expect("fixture must be created");
    let directory = fixture.home.join("Library/Caches/example");
    std::fs::write(directory.join("z-last"), b"z").expect("fixture file must be created");
    std::fs::write(directory.join("a-first"), b"a").expect("fixture file must be created");
    let before = fixture.snapshot().expect("baseline snapshot must succeed");
    let root = Root::open(&fixture.home).expect("fixture root must initialize");

    let children = root.children(&directory).expect("directory must be listed");

    assert_eq!(
        children
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>(),
        ["a-first", "artifact.bin", "z-last"]
            .map(|name| directory.join(name))
            .to_vec()
    );
    assert_eq!(
        fixture.snapshot().expect("final snapshot must succeed"),
        before
    );
}

#[test]
fn root_refuses_to_list_through_an_intermediate_symlink() {
    let fixture = ReadOnlyFixture::create().expect("fixture must be created");
    let outside = tempfile::tempdir().expect("outside directory must be created");
    let link = fixture.home.join("linked");
    std::os::unix::fs::symlink(outside.path(), &link).expect("symlink must be created");
    let root = Root::open(&fixture.home).expect("fixture root must initialize");

    assert!(root.children(&link).is_err());
}

#[test]
fn exclusion_existence_check_does_not_enter_the_final_path() {
    let fixture = ReadOnlyFixture::create().expect("fixture must be created");
    let outside = tempfile::tempdir().expect("outside directory must be created");
    let link = fixture.home.join("excluded-link");
    std::os::unix::fs::symlink(outside.path(), &link).expect("symlink must be created");
    let before = fixture.snapshot().expect("baseline snapshot must succeed");
    let root = Root::open(&fixture.home).expect("fixture root must initialize");

    assert!(
        root.path_exists_without_descending(&link)
            .expect("parent listing must succeed")
    );
    assert!(
        !root
            .path_exists_without_descending(&fixture.home.join("missing"))
            .expect("missing name must be reported")
    );
    assert_eq!(
        fixture.snapshot().expect("final snapshot must succeed"),
        before
    );
}

#[test]
fn an_intermediate_symlink_cannot_escape_the_root() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let outside = tempfile::tempdir().expect("outside root must be created");
    let root_path = physical_root(&fixture);
    let outside_path = physical_root(&outside);
    let secret = outside_path.join("secret.bin");
    fs::write(&secret, b"outside").expect("outside fixture must be written");
    let link = root_path.join("escape");
    std::os::unix::fs::symlink(&outside_path, &link).expect("escape symlink must be created");

    let root = Root::open(&root_path).expect("fixture root must initialize");
    let error = root
        .measure_object(&link.join("secret.bin"))
        .expect_err("an intermediate symlink must not be followed");
    assert!(matches!(error.raw_os_error(), Some(40 | 62)));
}

#[test]
fn parent_components_cannot_escape_the_root_lexically() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let root_path = physical_root(&fixture);
    let root = Root::open(&root_path).expect("fixture root must initialize");
    let error = root
        .measure_object(&root_path.join("../outside.bin"))
        .expect_err("parent component must be rejected before probing");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn rust_getattrlist_matches_the_c_oracle() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let root_path = physical_root(&fixture);
    let path = root_path.join("oracle.bin");
    fs::write(&path, vec![0xa5; 8192]).expect("oracle fixture must be written");
    File::open(&path)
        .expect("oracle fixture must reopen")
        .sync_all()
        .expect("oracle fixture must be stable");

    let rust = Root::open(&root_path)
        .expect("fixture root must initialize")
        .measure_object(&path)
        .expect("Rust measurement must succeed");
    let c = c_oracle(&path);

    assert_eq!(rust.allocated_bytes, c[0]);
    assert_eq!(rust.data_allocated_bytes, c[1]);
    assert_eq!(rust.resource_fork_allocated_bytes, c[2]);
    assert_eq!(rust.private_bytes, c[3]);
    assert_eq!(rust.extended_flags, c[4]);
}

#[test]
fn rust_volume_layout_matches_the_c_oracle() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let root_path = physical_root(&fixture);
    let c = c_volume_oracle(&root_path);
    let rust = Root::open(&root_path)
        .expect("fixture root must initialize")
        .capacity()
        .expect("Rust capacity must succeed");
    let measured = |kind| {
        rust.iter().find_map(|value| match value {
            CapacityValue::Measured {
                kind: measured,
                bytes,
                ..
            } if *measured == kind => Some(*bytes),
            _ => None,
        })
    };

    assert_eq!(
        measured(sizetrail::fsx::CapacityKind::VolumeSize),
        Some(c[0])
    );
    for (kind, oracle) in [
        (sizetrail::fsx::CapacityKind::VolumeFree, c[1]),
        (sizetrail::fsx::CapacityKind::AvailableNormal, c[2]),
        (sizetrail::fsx::CapacityKind::VolumeUsed, c[3]),
    ] {
        let rust = measured(kind).expect("capacity must be measured");
        assert!(
            rust.abs_diff(oracle) < 1024 * 1024 * 1024,
            "volatile volume value differs from the C oracle by at least 1 GiB"
        );
    }
    let container = c[5]
        .checked_sub(c[6])
        .and_then(|blocks| blocks.checked_mul(c[4]))
        .expect("C statfs container arithmetic must fit");
    let rust_container = measured(sizetrail::fsx::CapacityKind::ContainerAllocated)
        .expect("APFS shared container must be measured");
    assert!(
        rust_container.abs_diff(container) < 1024 * 1024 * 1024,
        "Rust statfs layout differs from the C oracle by at least 1 GiB"
    );
}
