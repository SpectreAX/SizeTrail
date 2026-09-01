#![allow(clippy::disallowed_methods)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sizetrail::adapters::docker::{self, DockerAdapter};
use sizetrail::adapters::{
    AdapterDegradedReason, AdapterState, InventoryGapReason, ToolchainAdapter,
};
use sizetrail::fsx::Root;
use sizetrail::model::{
    Advice, AdviceImpact, FindingSubject, MeasurementQuantity, MeasurementValue,
};
use sizetrail::policy::{PolicyCtx, ProbePolicy, ReadOnlyCommand};

#[test]
fn default_raw_reports_only_host_backing_file_quantities() {
    let fixture = fixture_home();
    let disk = fixture
        .path
        .join("Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw");
    sparse_file(&disk, 8 * 1024 * 1024);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let mut ctx = PolicyCtx::for_scan();

    let inventory = adapter.inventory(&mut ctx, &AdapterState::NotPresent);
    let item = inventory
        .items
        .first()
        .expect("default data.img.raw finding");
    let quantities = item
        .measurements
        .iter()
        .map(|measurement| measurement.quantity)
        .collect::<Vec<_>>();

    assert_eq!(item.rule_id, "docker.virtual_disk");
    assert_eq!(
        item.subject.filesystem_path(),
        Some("~/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw")
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
            if reveal.normalized_path.ends_with("data.img.raw")
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
    let disk = data_folder.join("data.img.raw");
    sparse_file(&disk, 4 * 1024 * 1024);
    let physical_data_folder =
        std::fs::canonicalize(&data_folder).expect("data folder must canonicalize");
    let physical_disk = physical_data_folder.join("data.img.raw");
    let vmconfig = home.join(".orbstack/vmconfig.json");
    write_json(&vmconfig, &serde_json::json!({"data_dir": data_folder}));
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
fn custom_data_dir_sparse_image_remains_measurable() {
    let fixture = fixture_home();
    let data_folder = fixture.path.join("CustomOrbData");
    let disk = data_folder.join("data.img");
    sparse_file(&disk, 2 * 1024 * 1024);
    let vmconfig = fixture.path.join(".orbstack/vmconfig.json");
    write_json(&vmconfig, &serde_json::json!({"data_dir": data_folder}));
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let mut ctx = PolicyCtx::for_scan();

    let inventory = adapter.inventory(&mut ctx, &AdapterState::NotPresent);

    assert_eq!(inventory.items.len(), 1);
    assert_eq!(
        inventory.items[0].subject.filesystem_path(),
        Some("~/CustomOrbData/data.img")
    );

    let findings = adapter
        .classify(&inventory)
        .expect("a measured custom disk must classify");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule_id, "docker.virtual_disk");
    assert!(
        matches!(
            findings[0].advice.as_slice(),
            [Advice::Reveal(reveal)]
            if reveal.normalized_path.ends_with("data.img")
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
        .join("Library/Group Containers/HUAQ24HBR6.dev.orbstack/data");
    sparse_file(&data.join("data.img.raw"), 8 * 1024 * 1024);
    sparse_file(&data.join("data.img"), 2 * 1024 * 1024);
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
    let policies = fixture_policies(SYSTEM_DF_ARGS);
    let mut ready_ctx = PolicyCtx::for_test(&policies);
    let selected = adapter.inventory(&mut ready_ctx, &verified);
    let disk = selected
        .items
        .iter()
        .find(|item| item.rule_id == "docker.virtual_disk")
        .expect("verified version still selects one host disk");
    assert!(
        disk.subject
            .filesystem_path()
            .is_some_and(|path| path.ends_with("/data.img.raw"))
    );
    assert_eq!(ready_ctx.count(docker::SYSTEM_DF), 1);
}

#[test]
fn swap_and_unlisted_images_are_never_measured() {
    let fixture = fixture_home();
    let data = fixture
        .path
        .join("Library/Group Containers/HUAQ24HBR6.dev.orbstack/data");
    sparse_file(&data.join("swap.img"), 2 * 1024 * 1024);
    sparse_file(&data.join("rootfs.img"), 2 * 1024 * 1024);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let mut ctx = PolicyCtx::for_scan();

    let inventory = adapter.inventory(&mut ctx, &AdapterState::NotPresent);

    assert!(inventory.items.is_empty());
    assert!(inventory.gaps.is_empty());
}

#[test]
fn malformed_current_settings_do_not_fall_through_to_a_guessed_default() {
    let fixture = fixture_home();
    let disk = fixture
        .path
        .join("Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw");
    sparse_file(&disk, 8 * 1024 * 1024);
    let vmconfig = fixture.path.join(".orbstack/vmconfig.json");
    std::fs::create_dir_all(vmconfig.parent().expect("vmconfig parent"))
        .expect("vmconfig parent must be created");
    std::fs::write(&vmconfig, b"not-json").expect("invalid vmconfig fixture must be written");
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

#[test]
fn empty_vmconfig_uses_the_default_orbstack_disk() {
    let fixture = fixture_home();
    let disk = fixture
        .path
        .join("Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw");
    sparse_file(&disk, 8 * 1024 * 1024);
    write_json(
        &fixture.path.join(".orbstack/vmconfig.json"),
        &serde_json::json!({}),
    );
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let mut ctx = PolicyCtx::for_scan();

    let inventory = adapter.inventory(&mut ctx, &AdapterState::NotPresent);

    assert_eq!(inventory.items.len(), 1);
    assert_eq!(
        inventory.items[0].subject.filesystem_path(),
        Some("~/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw")
    );
    assert!(inventory.gaps.is_empty());
}

#[test]
fn ready_summary_emits_four_object_sets_without_summing_or_subtracting() {
    let fixture = fixture_home();
    let disk = fixture
        .path
        .join("Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw");
    sparse_file(&disk, 8 * 1024 * 1024);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let policies = fixture_policies(SYSTEM_DF_ARGS);
    let mut ctx = PolicyCtx::for_test(&policies);
    let state = AdapterState::Ready {
        version: "verified fixture".to_owned(),
    };

    let inventory = adapter.inventory(&mut ctx, &state);
    let findings = adapter
        .classify(&inventory)
        .expect("disk plus daemon rows must classify");

    assert_eq!(ctx.count(docker::SYSTEM_DF), 1);
    assert_eq!(
        inventory
            .items
            .iter()
            .map(|item| item.rule_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "docker.virtual_disk",
            "docker.images",
            "docker.containers",
            "docker.volumes",
            "docker.build_cache",
        ])
    );
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.rule_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "docker.virtual_disk",
            "docker.images",
            "docker.containers",
            "docker.volumes",
            "docker.build_cache",
        ])
    );
    assert!(
        inventory
            .gaps
            .iter()
            .all(|gap| gap.reason != InventoryGapReason::DaemonInventoryExcludesInactiveStore)
    );
    for finding in findings
        .iter()
        .filter(|finding| finding.rule_id != "docker.virtual_disk")
    {
        assert!(
            matches!(
                finding.subject,
                FindingSubject::ToolchainObjectSet { ref object_set_id }
                if object_set_id == &finding.rule_id
            ),
            "subject: {:?}",
            finding.subject
        );
        assert_eq!(
            finding
                .measurements
                .iter()
                .map(|measurement| measurement.quantity)
                .collect::<Vec<_>>(),
            [
                MeasurementQuantity::ObjectCount,
                MeasurementQuantity::ActiveObjectCount,
                MeasurementQuantity::DaemonUsed,
                MeasurementQuantity::DaemonReclaimable,
            ]
        );
        assert!(finding.measurements.iter().all(|measurement| {
            matches!(
                measurement.scope.kind,
                sizetrail::model::MeasurementScopeKind::ObjectSet
            ) && measurement.scope.id == finding.rule_id
                && matches!(
                    measurement.basis,
                    sizetrail::model::MeasurementBasis::DockerSystemDf
                )
                && !matches!(measurement.value, MeasurementValue::IntervalBytes { .. })
                && !matches!(measurement.value, MeasurementValue::ExactBytes { .. })
        }));
    }
}

#[test]
fn docker_advice_keeps_host_disk_and_user_state_from_being_treated_as_cleanup() {
    let fixture = fixture_home();
    let disk = fixture
        .path
        .join("Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw");
    sparse_file(&disk, 8 * 1024 * 1024);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let policies = fixture_policies(SYSTEM_DF_ARGS);
    let mut ctx = PolicyCtx::for_test(&policies);
    let findings = adapter
        .classify(&adapter.inventory(
            &mut ctx,
            &AdapterState::Ready {
                version: "verified fixture".to_owned(),
            },
        ))
        .expect("classified Docker findings must exist");
    let rendered = serde_json::to_string(
        &findings
            .iter()
            .flat_map(|finding| &finding.advice)
            .collect::<Vec<_>>(),
    )
    .expect("advice must serialize");
    for forbidden in ["--force", "--yes", "|", "sudo "] {
        assert!(
            !rendered.contains(forbidden),
            "compiled advice contains forbidden input: {forbidden}"
        );
    }

    let disk = findings
        .iter()
        .find(|finding| finding.rule_id == "docker.virtual_disk")
        .expect("virtual disk finding");
    assert!(
        matches!(
            disk.advice.as_slice(),
            [Advice::Reveal(reveal)]
            if reveal.normalized_path.ends_with("data.img.raw")
                && reveal.recovery_semantics.contains("safe deletion target")
        ),
        "disk advice: {:?}",
        disk.advice
    );

    let images = findings
        .iter()
        .find(|finding| finding.rule_id == "docker.images")
        .expect("images finding");
    assert!(matches!(
        images.advice.as_slice(),
        [Advice::Command(command)]
        if command.display_command == "docker --context orbstack image prune"
            && matches!(command.impact, AdviceImpact::Destructive)
            && !command.reliable_preview_available
            && command.explanation.contains("does not provide a reliable preview")
    ));

    let cache = findings
        .iter()
        .find(|finding| finding.rule_id == "docker.build_cache")
        .expect("build cache finding");
    assert!(matches!(
        cache.advice.as_slice(),
        [Advice::Command(command)]
        if command.display_command == "docker --context orbstack builder prune"
            && matches!(command.impact, AdviceImpact::Destructive)
            && !command.reliable_preview_available
    ));

    let containers = findings
        .iter()
        .find(|finding| finding.rule_id == "docker.containers")
        .expect("containers finding");
    assert!(matches!(
        containers.advice.as_slice(),
        [Advice::Command(command)]
        if command.display_command == "docker --context orbstack ps -a"
            && matches!(command.impact, AdviceImpact::Inspect)
            && command.reliable_preview_available
            && command.explanation.contains("does not suggest a prune")
    ));

    let volumes = findings
        .iter()
        .find(|finding| finding.rule_id == "docker.volumes")
        .expect("volumes finding");
    assert!(
        matches!(
            volumes.advice.as_slice(),
            [Advice::Command(command)]
            if command.display_command == "docker --context orbstack system prune --volumes"
                && matches!(command.impact, AdviceImpact::Destructive)
                && !command.reliable_preview_available
                && command.explanation.contains("stopped containers")
                && command.explanation.contains("anonymous volumes")
                && command.explanation.contains("not a recommended one-click next step")
        ),
        "volumes advice: {:?}",
        volumes.advice
    );

    assert!(
        findings
            .iter()
            .filter(|finding| finding.rule_id != "docker.virtual_disk")
            .all(|finding| finding
                .advice
                .iter()
                .all(|advice| !matches!(advice, Advice::Reveal(_))))
    );
}

#[test]
fn path_exclude_does_not_drop_daemon_object_sets() {
    let fixture = fixture_home();
    let disk = fixture
        .path
        .join("Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw");
    sparse_file(&disk, 8 * 1024 * 1024);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let excludes = [disk];
    let adapter = DockerAdapter::new(&root, &excludes);
    let policies = fixture_policies(SYSTEM_DF_ARGS);
    let mut ctx = PolicyCtx::for_test(&policies);
    let inventory = adapter.inventory(
        &mut ctx,
        &AdapterState::Ready {
            version: "verified fixture".to_owned(),
        },
    );

    assert_eq!(ctx.count(docker::SYSTEM_DF), 1);
    assert!(
        inventory
            .items
            .iter()
            .all(|item| item.rule_id != "docker.virtual_disk")
    );
    assert_eq!(
        inventory
            .items
            .iter()
            .map(|item| item.rule_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "docker.images",
            "docker.containers",
            "docker.volumes",
            "docker.build_cache",
        ])
    );
}

#[test]
#[ignore = "records a runner-specific fixture benchmark for publication"]
fn docker_inventory_fixture_benchmark() {
    let fixture = fixture_home();
    let disk = fixture
        .path
        .join("Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw");
    sparse_file(&disk, 8 * 1024 * 1024);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let policies = fixture_policies(SYSTEM_DF_ARGS);
    let state = AdapterState::Ready {
        version: "verified fixture".to_owned(),
    };
    let mut samples = Vec::new();
    for _ in 0..5 {
        let mut ctx = PolicyCtx::for_test(&policies);
        let started = std::time::Instant::now();
        let inventory = adapter.inventory(&mut ctx, &state);
        let elapsed = started.elapsed().as_nanos();
        assert!(
            inventory
                .items
                .iter()
                .any(|item| item.rule_id == "docker.virtual_disk")
        );
        assert_eq!(
            inventory
                .items
                .iter()
                .filter(|item| item.rule_id != "docker.virtual_disk")
                .count(),
            4
        );
        samples.push(elapsed);
    }
    samples.sort_unstable();
    println!(
        "SIZETRAIL_BENCHMARK_JSON={}",
        serde_json::json!({
            "adapter": "docker",
            "scope": "temp_home_sparse_raw_with_stubbed_system_df",
            "iterations": samples.len(),
            "median_wall_nanoseconds": samples[samples.len() / 2],
            "all_wall_nanoseconds": samples,
        })
    );
}

#[test]
fn not_present_never_runs_system_df() {
    let fixture = fixture_home();
    let disk = fixture
        .path
        .join("Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw");
    sparse_file(&disk, 8 * 1024 * 1024);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    let policies = fixture_policies(SYSTEM_DF_ARGS);
    let mut ctx = PolicyCtx::for_test(&policies);

    let inventory = adapter.inventory(&mut ctx, &AdapterState::NotPresent);

    assert_eq!(ctx.count(docker::SYSTEM_DF), 0);
    assert!(
        inventory
            .items
            .iter()
            .all(|item| item.rule_id == "docker.virtual_disk")
    );
    assert!(
        inventory
            .gaps
            .iter()
            .all(|gap| gap.reason != InventoryGapReason::DaemonInventoryExcludesInactiveStore)
    );
}

#[test]
fn malformed_ready_summary_keeps_the_host_disk_and_does_not_claim_inactive_store() {
    let fixture = fixture_home();
    let disk = fixture
        .path
        .join("Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw");
    sparse_file(&disk, 8 * 1024 * 1024);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = DockerAdapter::new(&root, &[]);
    const MALFORMED_ARGS: &[&str] = &["%s", "not-json\n"];
    let policies = fixture_policies(MALFORMED_ARGS);
    let mut ctx = PolicyCtx::for_test(&policies);
    let state = AdapterState::Ready {
        version: "verified fixture".to_owned(),
    };

    let inventory = adapter.inventory(&mut ctx, &state);

    assert_eq!(ctx.count(docker::SYSTEM_DF), 1);
    assert_eq!(inventory.items.len(), 1);
    assert_eq!(inventory.items[0].rule_id, "docker.virtual_disk");
    assert!(
        inventory
            .gaps
            .iter()
            .any(|gap| gap.reason == InventoryGapReason::InvalidToolOutput)
    );
    assert!(
        inventory
            .gaps
            .iter()
            .all(|gap| gap.reason != InventoryGapReason::DaemonInventoryExcludesInactiveStore)
    );
}

const CONTEXT: &str = include_str!("fixtures/docker/context-orbstack.json");
const VERSION_JSON: &str = include_str!("fixtures/docker/version-verified.json");
const SYSTEM_DF_JSON: &str = include_str!("fixtures/docker/system-df.ndjson");
const CONTEXT_ARGS: &[&str] = &["%s", CONTEXT];
const VERSION_ARGS: &[&str] = &["%s", VERSION_JSON];
const SYSTEM_DF_ARGS: &[&str] = &["%s", SYSTEM_DF_JSON];

fn fixture_policies(df: &'static [&'static str]) -> [ProbePolicy; 3] {
    [
        printf_policy(docker::CONTEXT_INSPECT, CONTEXT_ARGS),
        printf_policy(docker::VERSION, VERSION_ARGS),
        printf_policy(docker::SYSTEM_DF, df),
    ]
}

const fn printf_policy(
    id: sizetrail::policy::ProbeId,
    arguments: &'static [&'static str],
) -> ProbePolicy {
    ProbePolicy {
        id,
        max_calls_per_scan: 1,
        disable_env: "SIZETRAIL_NO_DOCKER_PROBE_FIXTURE",
        known_side_effects: &[],
        command: ReadOnlyCommand {
            program: "/usr/bin/printf",
            arguments,
            environment: &[],
            remove_environment: &[],
            timeout_millis: 10_000,
        },
    }
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
