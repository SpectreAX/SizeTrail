#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fs;
use std::path::Path;

use sizetrail::adapters::{AdapterDegradedReason, AdapterState, homebrew};
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
