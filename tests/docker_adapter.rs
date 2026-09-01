#![allow(clippy::disallowed_methods)]

use std::path::{Path, PathBuf};

use sizetrail::adapters::docker::DockerAdapter;
use sizetrail::adapters::{
    AdapterDegradedReason, AdapterState, InventoryGapReason, ToolchainAdapter,
};
use sizetrail::fsx::Root;
use sizetrail::model::{Advice, MeasurementQuantity, MeasurementValue};
use sizetrail::policy::PolicyCtx;

#[test]
fn default_raw_reports_only_host_backing_file_quantities() {
    let fixture = fixture_home();
    let disk = fixture
        .path
        .join("Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw");
    sparse_file(&disk, 8 * 1024 * 1024);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let mut ctx = PolicyCtx::for_scan();

    let inventory = adapter.inventory(&mut ctx, &AdapterState::NotPresent);
    let item = inventory.items.first().expect("default Docker.raw finding");
    let quantities = item
        .measurements
        .iter()
        .map(|measurement| measurement.quantity)
        .collect::<Vec<_>>();

    assert_eq!(item.rule_id, "docker.virtual_disk");
    assert_eq!(
        item.subject.filesystem_path(),
        Some("~/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw")
    );
    assert_eq!(
        quantities,
        [
            MeasurementQuantity::DiskImageLogicalLimit,
            MeasurementQuantity::HostAllocatedFootprint,
        ]
    );
    assert!(
        item.measurements.iter().all(|measurement| !matches!(
            measurement.value,
            MeasurementValue::IntervalBytes { .. }
        ))
    );

    let findings = adapter
        .classify(&inventory)
        .expect("a measured disk must classify");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "docker.virtual_disk");
    assert_eq!(findings[0].adapter_id, "docker");
    assert!(
        findings[0].measurements.iter().all(|measurement| !matches!(
            measurement.value,
            MeasurementValue::IntervalBytes { .. }
        ))
    );
    assert!(
        matches!(
            findings[0].advice.as_slice(),
            [Advice::Reveal(reveal)]
            if reveal.normalized_path.ends_with("Docker.raw")
                && reveal.recovery_semantics.contains("safe deletion target")
        ),
        "advice: {:?}",
        findings[0].advice
    );
}

#[test]
fn current_setting_opens_custom_data_folder_as_an_independent_root() {
    let temporary = tempfile::tempdir().expect("fixture container must be created");
    let home = temporary.path().join("home");
    let data_folder = temporary.path().join("DockerData");
    std::fs::create_dir(&home).expect("fixture HOME must be created");
    let home = std::fs::canonicalize(home).expect("fixture HOME must canonicalize");
    let disk = data_folder.join("Docker.raw");
    sparse_file(&disk, 4 * 1024 * 1024);
    let physical_data_folder =
        std::fs::canonicalize(&data_folder).expect("data folder must canonicalize");
    let physical_disk = physical_data_folder.join("Docker.raw");
    let settings = home.join("Library/Group Containers/group.com.docker/settings-store.json");
    write_json(&settings, &serde_json::json!({"DataFolder": data_folder}));
    let root = Root::open(&home).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let mut ctx = PolicyCtx::for_scan();

    let inventory = adapter.inventory(&mut ctx, &AdapterState::NotPresent);

    assert_eq!(inventory.items.len(), 1);
    assert_eq!(
        inventory.items[0].subject.filesystem_path(),
        Some(physical_disk.to_string_lossy().as_ref())
    );
    assert!(inventory.gaps.is_empty());
}

#[test]
fn legacy_setting_and_qcow2_remain_measurable() {
    let fixture = fixture_home();
    let data_folder = fixture.path.join("LegacyDockerData");
    let disk = data_folder.join("Docker.qcow2");
    sparse_file(&disk, 2 * 1024 * 1024);
    let settings = fixture
        .path
        .join("Library/Group Containers/group.com.docker/settings.json");
    write_json(&settings, &serde_json::json!({"dataFolder": data_folder}));
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let mut ctx = PolicyCtx::for_scan();

    let inventory = adapter.inventory(&mut ctx, &AdapterState::NotPresent);

    assert_eq!(inventory.items.len(), 1);
    assert_eq!(
        inventory.items[0].subject.filesystem_path(),
        Some("~/LegacyDockerData/Docker.qcow2")
    );

    let findings = adapter
        .classify(&inventory)
        .expect("a measured legacy disk must classify");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "docker.virtual_disk");
    assert!(
        matches!(
            findings[0].advice.as_slice(),
            [Advice::Reveal(reveal)]
            if reveal.normalized_path.ends_with("Docker.qcow2")
                && reveal.recovery_semantics.contains("safe deletion target")
        ),
        "advice: {:?}",
        findings[0].advice
    );
}

#[test]
fn ambiguous_images_are_never_summed_without_a_verified_version() {
    let fixture = fixture_home();
    let data = fixture
        .path
        .join("Library/Containers/com.docker.docker/Data/vms/0/data");
    sparse_file(&data.join("Docker.raw"), 8 * 1024 * 1024);
    sparse_file(&data.join("Docker.qcow2"), 2 * 1024 * 1024);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let mut ctx = PolicyCtx::for_scan();
    let unknown = AdapterState::Degraded {
        observed_version: None,
        reason: AdapterDegradedReason::UnknownVersion,
    };

    let inventory = adapter.inventory(&mut ctx, &unknown);

    assert!(inventory.items.is_empty());
    assert_eq!(inventory.gaps.len(), 1);
    assert_eq!(
        inventory.gaps[0].reason,
        InventoryGapReason::AmbiguousDiskImage
    );

    let verified = AdapterState::Ready {
        version: "verified fixture".to_owned(),
    };
    let selected = adapter.inventory(&mut ctx, &verified);
    assert_eq!(selected.items.len(), 1);
    assert!(
        selected.items[0]
            .subject
            .filesystem_path()
            .is_some_and(|path| path.ends_with("/Docker.raw"))
    );
}

#[test]
fn oldest_driver_layout_is_used_when_the_new_data_directory_is_empty() {
    let fixture = fixture_home();
    std::fs::create_dir_all(
        fixture
            .path
            .join("Library/Containers/com.docker.docker/Data/vms/0/data"),
    )
    .expect("empty modern data folder must be created");
    let legacy = fixture.path.join(
        "Library/Containers/com.docker.docker/Data/com.docker.driver.amd64-linux/Docker.qcow2",
    );
    sparse_file(&legacy, 2 * 1024 * 1024);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let mut ctx = PolicyCtx::for_scan();

    let inventory = adapter.inventory(&mut ctx, &AdapterState::NotPresent);

    assert_eq!(inventory.items.len(), 1);
    assert_eq!(
        inventory.items[0].subject.filesystem_path(),
        Some(
            "~/Library/Containers/com.docker.docker/Data/com.docker.driver.amd64-linux/Docker.qcow2"
        )
    );
}

#[test]
fn malformed_current_settings_do_not_fall_through_to_a_guessed_default() {
    let fixture = fixture_home();
    let disk = fixture
        .path
        .join("Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw");
    sparse_file(&disk, 8 * 1024 * 1024);
    let settings = fixture
        .path
        .join("Library/Group Containers/group.com.docker/settings-store.json");
    std::fs::create_dir_all(settings.parent().expect("settings parent"))
        .expect("settings parent must be created");
    std::fs::write(&settings, b"not-json").expect("invalid settings fixture must be written");
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let mut ctx = PolicyCtx::for_scan();

    let inventory = adapter.inventory(&mut ctx, &AdapterState::NotPresent);

    assert!(inventory.items.is_empty());
    assert_eq!(inventory.gaps.len(), 1);
    assert_eq!(
        inventory.gaps[0].reason,
        InventoryGapReason::InvalidToolOutput
    );
}

struct FixtureHome {
    _temporary: tempfile::TempDir,
    path: PathBuf,
}

fn fixture_home() -> FixtureHome {
    let temporary = tempfile::tempdir().expect("fixture HOME must be created");
    let path = std::fs::canonicalize(temporary.path()).expect("fixture HOME must canonicalize");
    FixtureHome {
        _temporary: temporary,
        path,
    }
}

fn sparse_file(path: &Path, logical_bytes: u64) {
    std::fs::create_dir_all(path.parent().expect("disk path must have a parent"))
        .expect("disk parent must be created");
    std::fs::File::create(path)
        .expect("disk image fixture must be created")
        .set_len(logical_bytes)
        .expect("disk image fixture must be sparse");
}

fn write_json(path: &Path, value: &serde_json::Value) {
    std::fs::create_dir_all(path.parent().expect("settings path must have a parent"))
        .expect("settings parent must be created");
    std::fs::write(
        path,
        serde_json::to_vec(value).expect("settings fixture must serialize"),
    )
    .expect("settings fixture must be written");
}
