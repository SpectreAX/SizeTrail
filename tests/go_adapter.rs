#![allow(clippy::disallowed_methods)]

use std::path::{Path, PathBuf};

use sizetrail::adapters::go::{BUILD_CACHE_RELATIVE, GoAdapter, MODULE_CACHE_RELATIVE};
use sizetrail::adapters::{
    AdapterDegradedReason, AdapterState, InventoryGapReason, ToolchainAdapter,
};
use sizetrail::fsx::Root;
use sizetrail::model::{Advice, AdviceImpact};

#[test]
fn default_caches_are_independent_roots() {
    let fixture = fixture_home();
    let build = write_cache(&fixture.path, BUILD_CACHE_RELATIVE);
    let module = write_cache(&fixture.path, MODULE_CACHE_RELATIVE);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = GoAdapter::new(&root, &[]);
    let inventory = adapter.inventory(
        &mut unused_ctx(),
        &AdapterState::Ready {
            version: "go1.26.6".to_owned(),
        },
    );

    let build_item = inventory
        .items
        .iter()
        .find(|item| item.rule_id == "go.build_cache")
        .expect("build cache");
    let module_item = inventory
        .items
        .iter()
        .find(|item| item.rule_id == "go.module_cache")
        .expect("module cache");
    assert_eq!(
        build_item.subject.filesystem_path(),
        Some("~/Library/Caches/go-build")
    );
    assert_eq!(module_item.subject.filesystem_path(), Some("~/go/pkg/mod"));
    assert_eq!(inventory.items.len(), 2);
    assert!(
        inventory.gaps.is_empty(),
        "default caches must not emit a combined remainder: {:?}",
        inventory.gaps
    );
    assert_ne!(build, module);

    let findings = adapter
        .classify(&inventory)
        .expect("default caches must classify");
    assert_eq!(findings.len(), 2);
    let rendered = serde_json::to_string(&findings).expect("findings must serialize");
    assert!(!rendered.contains("go clean -r"));
    assert!(!rendered.contains("--force"));
    assert!(!rendered.contains("--yes"));
    assert!(!rendered.contains('|'));
}

#[test]
fn goenv_overrides_measure_an_external_root() {
    let fixture = fixture_home();
    let custom_build_dir = tempfile::tempdir().expect("external build cache must be created");
    let custom_module_dir = tempfile::tempdir().expect("external module cache must be created");
    let custom_build = std::fs::canonicalize(custom_build_dir.path())
        .expect("external build cache must canonicalize");
    let custom_module = std::fs::canonicalize(custom_module_dir.path())
        .expect("external module cache must canonicalize");
    write_cache(&custom_build, "");
    write_cache(&custom_module, "");
    write_goenv(
        &fixture.path,
        &format!(
            "GOCACHE={}\nGOMODCACHE={}\n",
            custom_build.display(),
            custom_module.display()
        ),
    );
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = GoAdapter::new(&root, &[]);
    let inventory = adapter.inventory(&mut unused_ctx(), &AdapterState::NotPresent);

    assert_eq!(inventory.items.len(), 2);
    let paths = inventory
        .items
        .iter()
        .filter_map(|item| item.subject.filesystem_path().map(ToOwned::to_owned))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        paths,
        std::collections::BTreeSet::from([
            custom_build.to_string_lossy().into_owned(),
            custom_module.to_string_lossy().into_owned(),
        ])
    );
}

#[test]
fn exclusion_inside_an_external_cache_is_applied_before_inventory() {
    let fixture = fixture_home();
    let custom_build_dir = tempfile::tempdir().expect("external build cache must be created");
    let custom_build = std::fs::canonicalize(custom_build_dir.path())
        .expect("external build cache must canonicalize");
    write_cache(&custom_build, "");
    let excluded = custom_build.join("private-subtree");
    std::fs::create_dir(&excluded).expect("excluded subtree must be created");
    std::fs::write(excluded.join("must-not-be-measured"), b"excluded")
        .expect("excluded object must be written");
    write_goenv(
        &fixture.path,
        &format!("GOCACHE={}\n", custom_build.display()),
    );
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = GoAdapter::new(&root, std::slice::from_ref(&excluded));
    let inventory = adapter.inventory(&mut unused_ctx(), &AdapterState::NotPresent);

    let build = inventory
        .items
        .iter()
        .find(|item| item.rule_id == "go.build_cache")
        .expect("external build cache");
    let encoded = serde_json::to_value(&build.measurements).expect("measurements must serialize");
    assert!(
        encoded
            .as_array()
            .expect("measurements")
            .iter()
            .any(|measurement| {
                measurement["quantity"] == "logical_size" && measurement["value"]["bytes"] == 6
            }),
        "excluded descendants must not contribute to an external cache: {encoded}"
    );
}

#[test]
fn unknown_version_still_measures_default_caches() {
    let fixture = fixture_home();
    write_cache(&fixture.path, BUILD_CACHE_RELATIVE);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = GoAdapter::new(&root, &[]);
    let inventory = adapter.inventory(
        &mut unused_ctx(),
        &AdapterState::Degraded {
            observed_version: Some("go1.25.0".to_owned()),
            reason: AdapterDegradedReason::UnknownVersion,
        },
    );

    assert!(
        inventory
            .items
            .iter()
            .any(|item| item.rule_id == "go.build_cache"),
        "unknown version must not block static measurement: {:?}",
        inventory.gaps
    );
    assert!(
        inventory
            .gaps
            .iter()
            .all(|gap| gap.reason != InventoryGapReason::UnknownVersion)
    );
}

#[test]
fn malformed_goenv_is_a_typed_gap_and_does_not_guess() {
    let fixture = fixture_home();
    write_cache(&fixture.path, BUILD_CACHE_RELATIVE);
    write_goenv(&fixture.path, "GOCACHE=relative\n");
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = GoAdapter::new(&root, &[]);
    let inventory = adapter.inventory(&mut unused_ctx(), &AdapterState::NotPresent);

    assert!(inventory.items.is_empty());
    assert!(
        inventory
            .gaps
            .iter()
            .any(|gap| gap.reason == InventoryGapReason::InvalidToolOutput)
    );
}

#[test]
fn advice_renders_only_the_vendor_clean_commands() {
    let fixture = fixture_home();
    write_cache(&fixture.path, BUILD_CACHE_RELATIVE);
    write_cache(&fixture.path, MODULE_CACHE_RELATIVE);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = GoAdapter::new(&root, &[]);
    let findings = adapter
        .classify(&adapter.inventory(
            &mut unused_ctx(),
            &AdapterState::Ready {
                version: "go1.26.6".to_owned(),
            },
        ))
        .expect("classified caches");

    let commands = findings
        .iter()
        .flat_map(|finding| &finding.advice)
        .map(|advice| match advice {
            Advice::Command(command) => {
                assert!(matches!(command.impact, AdviceImpact::Destructive));
                assert!(!command.reliable_preview_available);
                command.display_command.clone()
            }
            other => panic!("Go advice must be a vendor command, got {other:?}"),
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        commands,
        std::collections::BTreeSet::from([
            "go clean -cache".to_owned(),
            "go clean -modcache".to_owned()
        ])
    );
}

#[test]
fn excluding_one_cache_leaves_the_other() {
    let fixture = fixture_home();
    let build = write_cache(&fixture.path, BUILD_CACHE_RELATIVE);
    write_cache(&fixture.path, MODULE_CACHE_RELATIVE);
    let root = Root::open(&fixture.path).expect("fixture HOME must initialize");
    let adapter = GoAdapter::new(&root, std::slice::from_ref(&build));
    let inventory = adapter.inventory(&mut unused_ctx(), &AdapterState::NotPresent);

    assert!(
        inventory
            .items
            .iter()
            .all(|item| item.rule_id != "go.build_cache")
    );
    assert!(
        inventory
            .items
            .iter()
            .any(|item| item.rule_id == "go.module_cache")
    );
}

fn unused_ctx() -> sizetrail::policy::PolicyCtx<'static> {
    sizetrail::policy::PolicyCtx::for_scan()
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

fn write_cache(root: &Path, relative: &str) -> PathBuf {
    let path = if relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative)
    };
    std::fs::create_dir_all(&path).expect("cache directory must be created");
    std::fs::write(path.join("cached-object"), b"object").expect("cache object must be written");
    path
}

fn write_goenv(home: &Path, contents: &str) {
    let path = home.join("Library/Application Support/go/env");
    std::fs::create_dir_all(path.parent().expect("GOENV parent")).expect("GOENV parent");
    std::fs::write(path, contents).expect("GOENV must be written");
}
