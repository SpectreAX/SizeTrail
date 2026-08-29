#![allow(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};

#[test]
fn hosted_support_claims_are_one_source_with_no_paid_large_runner() {
    let source = std::fs::read_to_string("ci/platforms.json")
        .expect("platform matrix source must be readable");
    let document: serde_json::Value =
        serde_json::from_str(&source).expect("platform matrix source must be JSON");
    let lanes = document["runtime_lanes"]
        .as_array()
        .expect("runtime lanes must be an array");
    assert!(
        document["release"]
            .as_str()
            .expect("release must be text")
            .starts_with(&format!("v{} ", env!("CARGO_PKG_VERSION")))
    );
    let mut runners = BTreeSet::new();
    let mut required = BTreeMap::<&str, BTreeSet<&str>>::new();
    for lane in lanes {
        let runner = lane["runner"].as_str().expect("runner must be text");
        assert!(runners.insert(runner), "duplicate runner lane: {runner}");
        assert!(!runner.ends_with("-large"), "paid larger runner entered CI");
        if lane["required"] == true {
            required
                .entry(lane["os"].as_str().expect("OS must be text"))
                .or_default()
                .insert(lane["arch"].as_str().expect("architecture must be text"));
        }
    }
    assert_eq!(
        required,
        BTreeMap::from([
            ("macOS 15", BTreeSet::from(["arm64", "x86_64"])),
            ("macOS 26", BTreeSet::from(["arm64", "x86_64"])),
        ])
    );
    assert!(runners.contains("xcode-27"));

    let workflow =
        std::fs::read_to_string(".github/workflows/ci.yml").expect("CI workflow must be readable");
    assert!(workflow.contains("ci/platforms.json"));
    assert!(workflow.contains("fromJSON(needs.matrix.outputs.runtime)"));
}

/// Q46: the claim and quantitative-documentation gates scan `docs/`, so notes only fall under them
/// if they live in the repository. Auto-generated notes exist solely on GitHub, where no gate can
/// read them, and a gate that the release path does not use is decoration (§9.0).
#[test]
fn release_notes_are_a_gated_repository_file_for_this_version() {
    let workflow = std::fs::read_to_string(".github/workflows/release.yml")
        .expect("release workflow must be readable");

    assert!(
        !workflow.contains("--generate-notes"),
        "auto-generated notes are public prose no gate can read"
    );
    assert!(
        workflow.contains("--notes-file"),
        "the release must publish notes from the gated repository file"
    );
    assert!(
        workflow.contains("docs/release-notes/"),
        "notes must live under a directory the claim gate already scans"
    );

    let notes = std::path::Path::new("docs/release-notes")
        .join(format!("v{}.md", env!("CARGO_PKG_VERSION")));
    assert!(
        notes.is_file(),
        "{} must exist before this version can be released",
        notes.display()
    );
    assert!(
        !std::fs::read_to_string(&notes)
            .expect("notes must be readable")
            .trim()
            .is_empty(),
        "empty notes would satisfy the gate while telling users nothing"
    );
}
