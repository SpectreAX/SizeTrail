#![allow(clippy::disallowed_methods)]

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

use sizetrail::fsx::{CapacityBasis, CapacityValue, Root};

fn c_oracle(path: &Path) -> [Option<u64>; 5] {
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
    let output = Command::new(executable)
        .arg(path)
        .output()
        .expect("C oracle must run");
    assert!(output.status.success(), "C oracle must succeed");
    let line = String::from_utf8(output.stdout).expect("C oracle output must be UTF-8");

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

#[test]
fn read_only_wrappers_measure_a_fixture_without_changing_it() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let path = fixture.path().join("artifact.bin");
    fs::write(&path, vec![0x5a; 4096]).expect("fixture file must be written");
    let before = fs::symlink_metadata(&path).expect("baseline metadata must be readable");

    let root = Root::open(fixture.path()).expect("fixture root must initialize");
    let measured = root
        .measure_object(&path)
        .expect("fixture object must be measurable");
    let capacity = root.capacity().expect("fixture capacity must be measurable");

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
            )
    ) || matches!(value, CapacityValue::Unmeasurable { .. })));

    let after = fs::symlink_metadata(&path).expect("final metadata must be readable");
    assert_eq!(before.ino(), after.ino());
    assert_eq!(before.len(), after.len());
    assert_eq!(before.modified().ok(), after.modified().ok());
}

#[test]
fn rust_getattrlist_matches_the_c_oracle() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    let path = fixture.path().join("oracle.bin");
    fs::write(&path, vec![0xa5; 8192]).expect("oracle fixture must be written");

    let rust = Root::open(fixture.path())
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
