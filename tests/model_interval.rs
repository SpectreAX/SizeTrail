use sizetrail::model::{
    ExtentKind, ExtentObservation, FileIdentity, StorageSignal, estimate_disposition,
};

fn object(fileid: u64) -> ExtentObservation {
    ExtentObservation {
        identity: FileIdentity {
            fsid: [7, 11],
            fileid,
        },
        kind: ExtentKind::FileForks,
        link_count: 1,
        covered_link_count: 1,
        allocated_bytes: Some(2_048),
        private_bytes: Some(1_024),
        signals: Vec::new(),
    }
}

#[test]
fn incomplete_hardlink_scope_has_zero_floor_and_complete_scope_is_deduplicated() {
    let mut first = object(41);
    first.link_count = 2;
    first.covered_link_count = 1;
    let mut second = first.clone();
    second.covered_link_count = 2;

    let incomplete = estimate_disposition(&[first], true).expect("estimate must fit");
    assert_eq!(incomplete.floor_bytes, 0);
    assert_eq!(incomplete.ceiling_bytes, Some(2_048));

    let complete =
        estimate_disposition(&[second.clone(), second], true).expect("estimate must fit");
    assert_eq!(complete.floor_bytes, 1_024);
    assert_eq!(complete.ceiling_bytes, Some(2_048));
}

#[test]
fn identity_uses_fsid_and_fileid_together() {
    let mut first = object(41);
    let mut second = object(41);
    second.identity.fsid = [8, 11];
    first.private_bytes = Some(100);
    second.private_bytes = Some(200);

    let estimate = estimate_disposition(&[first, second], true).expect("estimate must fit");
    assert_eq!(estimate.floor_bytes, 300);
    assert_eq!(estimate.ceiling_bytes, Some(4_096));
}

#[test]
fn missing_allocated_makes_ceiling_unknown_and_non_file_data_is_excluded() {
    let mut missing = object(1);
    missing.allocated_bytes = None;
    let mut directory = object(2);
    directory.kind = ExtentKind::Directory;
    directory.allocated_bytes = Some(u64::MAX);
    directory.private_bytes = Some(u64::MAX);

    let estimate = estimate_disposition(&[missing, directory], true).expect("estimate must fit");
    assert_eq!(estimate.floor_bytes, 1_024);
    assert_eq!(estimate.ceiling_bytes, None);
    assert!(estimate.has_unmeasurable_objects);
}

#[test]
fn snapshots_may_invalidate_only_the_floor() {
    let estimate = estimate_disposition(&[object(1)], false).expect("estimate must fit");
    assert_eq!(estimate.floor_bytes, 0);
    assert_eq!(estimate.ceiling_bytes, Some(2_048));
}

#[test]
fn negative_sharing_signals_never_collapse_the_interval() {
    let mut measured = object(1);
    measured.private_bytes = Some(0);
    measured.signals = vec![
        StorageSignal::MayShareBlocks(false),
        StorageSignal::VolumeHasSnapshots(false),
    ];

    let estimate = estimate_disposition(&[measured], true).expect("estimate must fit");
    assert_eq!(estimate.floor_bytes, 0);
    assert_eq!(estimate.ceiling_bytes, Some(2_048));
    assert!(estimate.unexplained_private_gap);
}

#[test]
fn signals_are_labels_and_never_arithmetic_inputs() {
    let plain = object(1);
    let mut signaled = plain.clone();
    signaled.signals = vec![
        StorageSignal::ResourceForkAllocated,
        StorageSignal::FilesystemCompressed,
        StorageSignal::Sparse,
        StorageSignal::Purgeable,
    ];

    assert_eq!(
        estimate_disposition(&[plain], true).expect("estimate must fit"),
        estimate_disposition(&[signaled], true).expect("estimate must fit")
    );
}
