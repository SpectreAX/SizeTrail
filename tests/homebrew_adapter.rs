#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use sizetrail::adapters::{
    AdapterDegradedReason, AdapterState, InventoryIdentity, ToolchainAdapter, homebrew,
};
use sizetrail::fsx::Root;
use sizetrail::model::{MeasurementBasis, MeasurementValue, ObservationRelation, SignalId};
use sizetrail::policy::{PolicyCtx, SIDE_EFFECT_REGISTRY};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("fixture parent must be created");
    }
    fs::write(path, contents).expect("fixture file must be written");
}

fn make_installation(root: &Path, prefix: &str, store: &str) {
    write(&root.join(prefix).join("bin/brew"), "fixture");
    fs::create_dir_all(root.join(prefix).join(store)).expect("fixture store must be created");
}

fn exact_bytes(item: &sizetrail::adapters::InventoryItem, basis: MeasurementBasis) -> u64 {
    item.measurements
        .iter()
        .find_map(|measurement| {
            (std::mem::discriminant(&measurement.basis) == std::mem::discriminant(&basis))
                .then_some(&measurement.value)
        })
        .and_then(|value| match value {
            MeasurementValue::ExactBytes { bytes } => Some(*bytes),
            _ => None,
        })
        .expect("the requested exact measurement must exist")
}

fn repository_with_describe_cache() -> tempfile::TempDir {
    let fixture = tempfile::tempdir().expect("fixture repository must be created");
    write(
        &fixture.path().join(format!(".git/describe-cache/{SHA}")),
        "6.0.19\n",
    );
    fixture
}

#[test]
fn homebrew_version_reads_a_loose_ref_without_invoking_any_registered_command() {
    let fixture = repository_with_describe_cache();
    write(
        &fixture.path().join(".git/HEAD"),
        "ref: refs/heads/stable\n",
    );
    write(&fixture.path().join(".git/refs/heads/stable"), SHA);
    let mut ctx = PolicyCtx::for_scan();

    assert_eq!(
        homebrew::probe_version(fixture.path(), &mut ctx),
        AdapterState::Ready {
            version: "6.0.19".to_owned(),
        }
    );
    for policy in SIDE_EFFECT_REGISTRY {
        assert_eq!(
            ctx.count(policy.id),
            0,
            "Homebrew version discovery invoked {}",
            policy.id.as_str()
        );
    }
}

#[test]
fn homebrew_version_accepts_a_detached_head() {
    let fixture = repository_with_describe_cache();
    write(&fixture.path().join(".git/HEAD"), SHA);

    assert_eq!(
        homebrew::probe_version(fixture.path(), &mut PolicyCtx::for_scan()),
        AdapterState::Ready {
            version: "6.0.19".to_owned(),
        }
    );
}

#[test]
fn homebrew_version_falls_back_to_packed_refs() {
    let fixture = repository_with_describe_cache();
    write(
        &fixture.path().join(".git/HEAD"),
        "ref: refs/heads/stable\n",
    );
    write(
        &fixture.path().join(".git/packed-refs"),
        &format!("# pack-refs with: peeled fully-peeled sorted\n{SHA} refs/heads/stable\n"),
    );

    assert_eq!(
        homebrew::probe_version(fixture.path(), &mut PolicyCtx::for_scan()),
        AdapterState::Ready {
            version: "6.0.19".to_owned(),
        }
    );
}

#[test]
fn missing_homebrew_version_metadata_is_an_explicit_degraded_state() {
    let fixture = tempfile::tempdir().expect("fixture repository must be created");
    write(
        &fixture.path().join(".git/HEAD"),
        "ref: refs/heads/stable\n",
    );

    assert_eq!(
        homebrew::probe_version(fixture.path(), &mut PolicyCtx::for_scan()),
        AdapterState::Degraded {
            observed_version: None,
            reason: AdapterDegradedReason::UnknownVersion,
        }
    );
}

#[test]
fn apple_silicon_prefix_is_preferred_and_opened_as_its_own_root() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    make_installation(fixture.path(), "opt/homebrew", "Cellar");
    make_installation(fixture.path(), "usr/local", "Cellar");

    let layout = homebrew::discover_layout(Some(fixture.path()))
        .expect("the Apple Silicon layout must be discovered first");
    assert_eq!(layout.prefix, fixture.path().join("opt/homebrew"));
    assert_eq!(layout.repository, layout.prefix);
    assert_eq!(layout.cellar, Some(layout.prefix.join("Cellar")));

    let prefix_root = homebrew::open_prefix_root(&layout).expect("prefix Root must open");
    assert_eq!(
        prefix_root.path(),
        fs::canonicalize(&layout.prefix)
            .expect("fixture prefix must have a physical representation")
    );
}

#[test]
fn intel_prefix_keeps_the_repository_distinct() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    make_installation(fixture.path(), "usr/local", "Cellar");

    let layout = homebrew::discover_layout(Some(fixture.path()))
        .expect("the Intel layout must be discovered");
    assert_eq!(layout.prefix, fixture.path().join("usr/local"));
    assert_eq!(layout.repository, fixture.path().join("usr/local/Homebrew"));
    assert_eq!(layout.cellar, Some(layout.prefix.join("Cellar")));
}

#[test]
fn cellar_falls_back_to_the_repository_layout() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    make_installation(fixture.path(), "usr/local", "Caskroom");
    fs::create_dir_all(fixture.path().join("usr/local/Homebrew/Cellar"))
        .expect("repository Cellar must be created");

    let layout = homebrew::discover_layout(Some(fixture.path()))
        .expect("Caskroom proves the prefix is present");
    assert_eq!(
        layout.cellar,
        Some(fixture.path().join("usr/local/Homebrew/Cellar"))
    );
}

#[test]
fn a_brew_launcher_without_cellar_or_caskroom_is_not_an_installation() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    write(
        &fixture.path().join("opt/homebrew/bin/brew"),
        "incomplete fixture",
    );

    assert_eq!(homebrew::discover_layout(Some(fixture.path())), None);
}

#[test]
fn home_and_prefix_stores_remain_separate_and_keg_identity_comes_from_directories() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    make_installation(fixture.path(), "opt/homebrew", "Cellar");
    write(
        &fixture
            .path()
            .join("Library/Caches/Homebrew/downloads/source.tar.gz"),
        "download",
    );
    let keg = fixture.path().join("opt/homebrew/Cellar/example/1.2.3");
    write(&keg.join("lib/libexample.1.dylib"), "library-bytes");
    symlink("libexample.1.dylib", keg.join("lib/libexample.dylib"))
        .expect("versioned library link must be created");
    fs::hard_link(
        keg.join("lib/libexample.1.dylib"),
        keg.join("lib/libexample-hardlink.dylib"),
    )
    .expect("hardlink fixture must be created");
    write(
        &fixture
            .path()
            .join("opt/homebrew/Cellar/example/2.0/INSTALL_RECEIPT.json"),
        r#"{"installed_on_request":true,"name":"must-not-be-used"}"#,
    );

    let home_root = Root::open(fixture.path()).expect("HOME Root must open");
    let layout = homebrew::discover_layout(Some(fixture.path())).expect("layout must exist");
    let prefix_root = homebrew::open_prefix_root(&layout).expect("prefix Root must open");
    let adapter = homebrew::HomebrewAdapter::new(
        &home_root,
        &prefix_root,
        &layout,
        &[],
        Ok(false),
        Ok(false),
    );
    let inventory = adapter.inventory(
        &mut PolicyCtx::for_scan(),
        &AdapterState::Ready {
            version: "6.0.19".to_owned(),
        },
    );

    let download = inventory
        .items
        .iter()
        .find(|item| item.rule_id == "homebrew.cache_downloads")
        .expect("HOME-side cache must be measured");
    assert_eq!(
        download.normalized_path,
        "~/Library/Caches/Homebrew/downloads"
    );
    let cellar = inventory
        .items
        .iter()
        .find(|item| item.rule_id == "homebrew.cellar")
        .unwrap_or_else(|| {
            panic!(
                "prefix-side keg must be measured; items={:?}, gaps={:?}",
                inventory.items, inventory.gaps
            )
        });
    assert_eq!(cellar.normalized_path, "/opt/homebrew/Cellar/example/1.2.3");
    assert!(
        inventory
            .items
            .iter()
            .all(|item| item.rule_id != "homebrew.total"),
        "HOME and prefix Roots must never be summed"
    );
    assert!(matches!(
        &cellar.identity,
        InventoryIdentity::HomebrewKeg {
            formula,
            version,
            installed_on_request: None,
        } if formula == "example" && version == "1.2.3"
    ));
    assert!(inventory.items.iter().any(|item| matches!(
        &item.identity,
        InventoryIdentity::HomebrewKeg {
            formula,
            version,
            installed_on_request: Some(true),
        } if formula == "example" && version == "2.0"
    )));

    let target = fs::symlink_metadata(keg.join("lib/libexample.1.dylib"))
        .expect("target metadata must be readable");
    let link = fs::symlink_metadata(keg.join("lib/libexample.dylib"))
        .expect("link metadata must be readable");
    assert_eq!(
        exact_bytes(cellar, MeasurementBasis::LogicalSize),
        target.len() + link.len(),
        "the hardlink is deduplicated and the symlink contributes only itself"
    );
    assert!(cellar.observations.iter().any(|observation| {
        observation.signal == SignalId::MultipleHardlinks
            && observation.relation == ObservationRelation::DeletionScope
    }));
    let interval = cellar
        .measurements
        .iter()
        .find(|measurement| {
            matches!(
                measurement.basis,
                MeasurementBasis::PrivateFloorAllocatedCeiling
            )
        })
        .expect("disposition interval must exist");
    assert!(matches!(
        interval.value,
        MeasurementValue::IntervalBytes { floor_bytes: 0, .. }
    ));
}

#[test]
fn caskroom_symlink_targets_outside_prefix_become_gaps_without_becoming_measurements() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    make_installation(fixture.path(), "opt/homebrew", "Caskroom");
    let cask = fixture.path().join("opt/homebrew/Caskroom/zed/1.15.0");
    fs::create_dir_all(&cask).expect("cask fixture must be created");
    symlink("/Applications/Zed.app", cask.join("Zed.app")).expect("moved app link must be created");

    let home_root = Root::open(fixture.path()).expect("HOME Root must open");
    let layout = homebrew::discover_layout(Some(fixture.path())).expect("layout must exist");
    let prefix_root = homebrew::open_prefix_root(&layout).expect("prefix Root must open");
    let adapter = homebrew::HomebrewAdapter::new(
        &home_root,
        &prefix_root,
        &layout,
        &[],
        Ok(false),
        Ok(false),
    );
    let inventory = adapter.inventory(
        &mut PolicyCtx::for_scan(),
        &AdapterState::Degraded {
            observed_version: None,
            reason: AdapterDegradedReason::UnknownVersion,
        },
    );

    assert!(inventory.gaps.iter().any(|gap| {
        gap.reason == sizetrail::adapters::InventoryGapReason::CaskArtifactOutsidePrefix
            && gap
                .path
                .as_deref()
                .is_some_and(|path| path.ends_with("opt/homebrew/Caskroom/zed"))
    }));
    assert!(
        inventory
            .gaps
            .iter()
            .any(|gap| { gap.reason == sizetrail::adapters::InventoryGapReason::UnknownVersion })
    );
    assert!(
        inventory
            .gaps
            .iter()
            .any(|gap| { gap.reason == sizetrail::adapters::InventoryGapReason::AbsentOrChanged })
    );
    let caskroom = inventory
        .items
        .iter()
        .find(|item| item.rule_id == "homebrew.caskroom")
        .expect("Caskroom itself remains measurable");
    assert_eq!(
        exact_bytes(caskroom, MeasurementBasis::LogicalSize),
        fs::symlink_metadata(cask.join("Zed.app"))
            .expect("link metadata must be readable")
            .len()
    );
    assert!(!caskroom.normalized_path.starts_with("/Applications"));
}

#[test]
fn an_unavailable_prefix_root_keeps_home_measurements_and_declares_the_boundary_gap() {
    let fixture = tempfile::tempdir().expect("fixture root must be created");
    make_installation(fixture.path(), "opt/homebrew", "Cellar");
    write(
        &fixture
            .path()
            .join("Library/Caches/Homebrew/downloads/source.tar.gz"),
        "download",
    );
    let home_root = Root::open(fixture.path()).expect("HOME Root must open");
    let layout = homebrew::discover_layout(Some(fixture.path())).expect("layout must exist");
    let adapter = homebrew::HomebrewAdapter::without_prefix(
        &home_root,
        &layout,
        &[],
        Ok(false),
        sizetrail::adapters::InventoryGapReason::AccessDenied,
    );

    let inventory = adapter.inventory(
        &mut PolicyCtx::for_scan(),
        &AdapterState::Ready {
            version: "6.0.19".to_owned(),
        },
    );

    assert!(
        inventory
            .items
            .iter()
            .any(|item| item.rule_id == "homebrew.cache_downloads")
    );
    assert!(
        inventory
            .items
            .iter()
            .all(|item| !item.normalized_path.starts_with("/opt/homebrew"))
    );
    assert!(inventory.gaps.iter().any(|gap| {
        gap.reason == sizetrail::adapters::InventoryGapReason::AccessDenied
            && gap.stage == Some(sizetrail::adapters::InventoryStage::RootInitialization)
    }));
}
