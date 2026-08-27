#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use sizetrail::fsx::Root;
use sizetrail::model::{ExtentKind, ExtentObservation, StorageSignal, estimate_disposition};

const MIB: usize = 1024 * 1024;
const UF_COMPRESSED: u32 = 0x0000_0020;

fn root_and_path(name: &str) -> (tempfile::TempDir, PathBuf) {
    let fixture = tempfile::tempdir().expect("APFS fixture root must be created");
    let root = fs::canonicalize(fixture.path()).expect("fixture root must have a physical path");
    let path = root.join(name);
    (fixture, path)
}

fn command(program: &str, arguments: &[&Path]) -> bool {
    Command::new(program)
        .args(arguments)
        .status()
        .is_ok_and(|status| status.success())
}

fn report_gap(fixture: &str, detail: &str) {
    eprintln!("SIZETRAIL_P2_COVERAGE_GAP fixture={fixture} detail={detail}");
}

#[test]
fn clone_allocated_footprints_are_counted_per_directory_entry() {
    let (_fixture, original) = root_and_path("clone-source.bin");
    let clone = original
        .parent()
        .expect("clone source has a parent")
        .join("clone-copy.bin");
    fs::write(&original, vec![0x5a; 20 * MIB]).expect("clone source must be written");
    if !command("/bin/cp", &[Path::new("-c"), &original, &clone]) {
        report_gap("clone", "cp -c could not construct an APFS clone");
        return;
    }

    let root = Root::open(original.parent().expect("clone has a parent"))
        .expect("APFS root must initialize");
    let first = root
        .measure_object(&original)
        .expect("clone source must be measurable");
    let second = root
        .measure_object(&clone)
        .expect("clone copy must be measurable");
    let first_blocks = fs::metadata(&original)
        .expect("source metadata must be readable")
        .blocks();
    let second_blocks = fs::metadata(&clone)
        .expect("clone metadata must be readable")
        .blocks();

    assert_eq!(first.allocated_bytes, second.allocated_bytes);
    assert!(first.allocated_bytes.is_some_and(|bytes| bytes > 0));
    assert_eq!(first_blocks, second_blocks);
    assert!(first_blocks > 0, "st_blocks must report both clone entries");
}

#[test]
fn resource_fork_can_have_allocated_bytes_with_zero_private_floor() {
    let (_fixture, base) = root_and_path("resource-only.bin");
    File::create(&base).expect("empty data fork must be created");
    let resource = base.join("..namedfork/rsrc");
    let mut fork = File::create(resource).expect("resource fork must be created");
    let bytes: Vec<u8> = (0..2 * MIB).map(|index| (index % 251) as u8).collect();
    fork.write_all(&bytes)
        .expect("resource fork must be written");
    fork.sync_all()
        .expect("resource fork fixture must be stable");

    let measured = Root::open(base.parent().expect("resource file has a parent"))
        .expect("APFS root must initialize")
        .measure_object(&base)
        .expect("resource-only file must be measurable");

    assert!(
        measured
            .resource_fork_allocated_bytes
            .is_some_and(|bytes| bytes > 0)
    );
    assert!(measured.allocated_bytes.is_some_and(|bytes| bytes > 0));
    assert_eq!(measured.private_bytes, Some(0));
    assert_eq!(measured.extended_flags, Some(0));
}

#[test]
fn hfs_compression_keeps_the_private_floor_uninformative() {
    let fixture = tempfile::tempdir().expect("compression fixture root must be created");
    let root = fs::canonicalize(fixture.path()).expect("fixture root must have a physical path");
    let source = root.join("source");
    let output = root.join("output");
    let archive = root.join("fixture.cpio");
    fs::create_dir(&source).expect("compression source directory must be created");
    fs::create_dir(&output).expect("compression output directory must be created");
    fs::write(source.join("sample.bin"), vec![0x41; 512 * 1024])
        .expect("compressible source must be written");

    let packed = command("/usr/bin/ditto", &[Path::new("-c"), &source, &archive]);
    let unpacked = packed
        && command(
            "/usr/bin/ditto",
            &[
                Path::new("-x"),
                Path::new("--hfsCompression"),
                &archive,
                &output,
            ],
        );
    if !unpacked {
        report_gap(
            "hfs_compression",
            "CPIO plus ditto --hfsCompression construction was unavailable",
        );
        return;
    }

    let sample = output.join("sample.bin");
    let measured = Root::open(&root)
        .expect("APFS root must initialize")
        .measure_object(&sample)
        .expect("compressed file must be measurable");
    if measured.private_bytes != Some(0) {
        report_gap(
            "hfs_compression",
            "runner filesystem did not preserve HFS compression",
        );
        return;
    }
    assert!(measured.allocated_bytes.is_some_and(|bytes| bytes > 0));
    assert_eq!(measured.resource_fork_allocated_bytes, Some(0));
    assert_eq!(measured.extended_flags, Some(0));
    assert_ne!(measured.bsd_flags & UF_COMPRESSED, 0);
}

#[test]
fn an_incomplete_hardlink_set_has_no_private_floor() {
    let (_fixture, first_path) = root_and_path("hardlink-first.bin");
    let second_path = first_path
        .parent()
        .expect("hardlink source has a parent")
        .join("hardlink-second.bin");
    fs::write(&first_path, vec![0x37; MIB]).expect("hardlink source must be written");
    fs::hard_link(&first_path, &second_path).expect("second hardlink must be created");
    let measured = Root::open(first_path.parent().expect("hardlink has a parent"))
        .expect("APFS root must initialize")
        .measure_object(&first_path)
        .expect("hardlink must be measurable");
    assert_eq!(measured.link_count, 2);

    let estimate = estimate_disposition(
        &[ExtentObservation {
            identity: measured.identity,
            kind: ExtentKind::FileForks,
            link_count: measured.link_count,
            covered_link_count: 1,
            allocated_bytes: measured.allocated_bytes,
            private_bytes: measured.private_bytes,
            signals: Vec::new(),
        }],
        true,
    )
    .expect("hardlink estimate must fit");
    assert_eq!(estimate.floor_bytes, 0);
}

#[test]
fn sparse_is_a_logical_allocation_gap_not_a_private_gap_explanation() {
    let (_fixture, path) = root_and_path("sparse.bin");
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .expect("sparse fixture must be created");
    file.set_len((8 * MIB) as u64)
        .expect("sparse logical length must be set");
    file.sync_all().expect("sparse fixture must be stable");

    let measured = Root::open(path.parent().expect("sparse file has a parent"))
        .expect("APFS root must initialize")
        .measure_object(&path)
        .expect("sparse file must be measurable");
    assert_eq!(measured.logical_bytes, 8 * MIB as u64);
    assert!(
        measured
            .allocated_bytes
            .is_some_and(|allocated| allocated < measured.logical_bytes)
    );

    let observation = ExtentObservation {
        identity: measured.identity,
        kind: ExtentKind::FileForks,
        link_count: 1,
        covered_link_count: 1,
        allocated_bytes: measured.allocated_bytes,
        private_bytes: measured.private_bytes,
        signals: vec![StorageSignal::Sparse],
    };
    let without_signal = ExtentObservation {
        signals: Vec::new(),
        ..observation.clone()
    };
    assert_eq!(
        estimate_disposition(&[observation], true).expect("estimate must fit"),
        estimate_disposition(&[without_signal], true).expect("estimate must fit")
    );
}
