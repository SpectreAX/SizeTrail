use std::path::Path;

use sizetrail::model::{
    Advice, AdviceImpact, CommandAdvice, Finding, FindingIdError, FindingSubject, Measurement,
    ObservationKind, ObservationRelation, ObservationScope, RevealAdvice, SignalId,
    SignalObservation, finding_id, normalize_findings, normalized_report_path,
};

#[test]
fn finding_ids_are_order_independent_and_home_relative() {
    let first_home = Path::new("/Users/first");
    let second_home = Path::new("/Users/second");
    let first_path = first_home.join("Library/Developer/Xcode/DerivedData/App/Build");
    let second_path = second_home.join("Library/Developer/Xcode/DerivedData/App/Build");
    let first = normalized_report_path(first_home, &first_path).expect("path must normalize");
    let second = normalized_report_path(second_home, &second_path).expect("path must normalize");

    assert_eq!(first, "~/Library/Developer/Xcode/DerivedData/App/Build");
    assert_eq!(first, second);
    assert_eq!(
        finding_id("xcode", "xcode.derived_data_build", &first),
        finding_id("xcode", "xcode.derived_data_build", &second)
    );
    assert!(
        finding_id("xcode", "xcode.derived_data_build", &first)
            .expect("id inputs are valid")
            .starts_with("f1:xcode:")
    );
}

#[test]
fn finding_id_set_does_not_depend_on_discovery_order() {
    let paths = [
        "~/Library/Developer/Xcode/Archives/A",
        "~/Library/Developer/Xcode/Archives/B",
    ];
    let mut forward = paths
        .iter()
        .map(|path| finding_id("xcode", "xcode.archives", path).expect("valid id"))
        .collect::<Vec<_>>();
    let mut reverse = paths
        .iter()
        .rev()
        .map(|path| finding_id("xcode", "xcode.archives", path).expect("valid id"))
        .collect::<Vec<_>>();
    forward.sort();
    reverse.sort();

    assert_eq!(forward, reverse);
}

#[test]
fn structured_signals_have_a_deterministic_lossy_summary_order() {
    let compressed = SignalObservation {
        observation: ObservationKind::Direct,
        signal: SignalId::FilesystemCompressed,
        relation: ObservationRelation::TestedWidthCorrelate,
        scope: ObservationScope::Object,
    };
    let sharing = SignalObservation {
        observation: ObservationKind::Direct,
        signal: SignalId::MayShareBlocks,
        relation: ObservationRelation::PossibleWidthExplanation,
        scope: ObservationScope::Inode,
    };
    let mut findings = vec![Finding {
        id: "f1:xcode:bbbbbbbbbbbbbbbb".to_owned(),
        adapter_id: "xcode".to_owned(),
        rule_id: "xcode.derived_data_build".to_owned(),
        title: "Fixture".to_owned(),
        summary: String::new(),
        subject: FindingSubject::FilesystemPath {
            normalized_path: "~/Library/Developer/Xcode/DerivedData/App/Build".to_owned(),
        },
        mechanism: "generated".to_owned(),
        recoverability: "rebuild_time_cost".to_owned(),
        sensitivity: "low".to_owned(),
        evidence: "fixture evidence".to_owned(),
        unexplained_private_gap: true,
        measurements: Vec::<Measurement>::new(),
        observations: vec![sharing, compressed],
        advice: Vec::new(),
    }];

    normalize_findings(&mut findings);

    assert_eq!(
        findings[0].observations[0].signal,
        SignalId::FilesystemCompressed
    );
    assert_eq!(
        findings[0].summary,
        "compressed storage makes the private floor uninformative"
    );
    assert_eq!(findings[0].observations.len(), 2);
}

#[test]
fn filesystem_and_toolchain_subjects_have_disjoint_canonical_keys() {
    let path = FindingSubject::FilesystemPath {
        normalized_path: "~/Library/Containers/com.docker.docker".to_owned(),
    };
    let object_set = FindingSubject::ToolchainObjectSet {
        object_set_id: "docker.images".to_owned(),
    };

    assert_eq!(
        path.canonical_key().expect("path subject must be valid"),
        "~/Library/Containers/com.docker.docker"
    );
    assert_eq!(
        object_set
            .canonical_key()
            .expect("object-set subject must be valid"),
        "object_set:docker.images"
    );
    assert_eq!(
        path.filesystem_path(),
        Some("~/Library/Containers/com.docker.docker")
    );
    assert_eq!(object_set.filesystem_path(), None);
    assert_ne!(
        finding_id(
            "docker",
            "docker.images",
            &path.canonical_key().expect("path key")
        ),
        finding_id(
            "docker",
            "docker.images",
            &object_set.canonical_key().expect("object-set key")
        )
    );

    let serialized = serde_json::to_value(object_set).expect("subject must serialize");
    assert_eq!(serialized["kind"], "toolchain_object_set");
    assert_eq!(serialized["object_set_id"], "docker.images");
}

#[test]
fn finding_id_inputs_reject_ambiguous_components() {
    assert_eq!(
        finding_id("xcode:other", "rule", "~/path"),
        Err(FindingIdError::InvalidAdapterId)
    );
    assert_eq!(
        finding_id("xcode", "rule", "../path"),
        Err(FindingIdError::InvalidNormalizedPath)
    );
}

#[test]
fn advice_commands_remain_render_only_and_avoid_unsafe_convenience_flags() {
    let advice = Advice::Command(CommandAdvice {
        display_command: "xcrun simctl delete unavailable".to_owned(),
        impact: AdviceImpact::Destructive,
        explanation: "This removes simulator devices whose runtimes are unavailable.".to_owned(),
        reliable_preview_available: false,
    });
    let reveal = Advice::Reveal(RevealAdvice {
        normalized_path: "~/Library/Developer/Xcode/Archives".to_owned(),
        recovery_semantics: "Finder Trash is the user's recovery boundary.".to_owned(),
    });
    let rendered = serde_json::to_string(&[advice, reveal]).expect("advice must serialize");

    for forbidden in ["--force", "--yes", "|", "sudo "] {
        assert!(!rendered.contains(forbidden));
    }
}
