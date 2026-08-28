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
