use std::collections::BTreeSet;
use std::path::Path;

use sizetrail::rules::{
    COMPILED_ADAPTER_IDS, RuleError, RuleSubjectPattern, builtin_rules, default_selected,
};

#[derive(serde::Deserialize)]
struct RuleFixture {
    rule_id: String,
    mechanism: String,
    recoverability: String,
    sensitivity: String,
    expected_path: String,
}

#[test]
fn every_builtin_rule_has_evidence_subjects_adapter_and_fixture() {
    let rules = builtin_rules().expect("compiled rules must parse and validate");
    assert!(!rules.is_empty());

    let ids = rules
        .iter()
        .map(|rule| rule.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), rules.len());
    for rule in &rules {
        assert!(!rule.evidence.trim().is_empty());
        assert!(!rule.subjects.is_empty());
        assert!(COMPILED_ADAPTER_IDS.contains(&rule.adapter.as_str()));
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/rules")
            .join(format!("{}.json", rule.fixture_id));
        let fixture: RuleFixture = serde_json::from_slice(
            &std::fs::read(&path).unwrap_or_else(|_| panic!("missing fixture for {}", rule.id)),
        )
        .unwrap_or_else(|_| panic!("invalid fixture for {}", rule.id));
        assert_eq!(fixture.rule_id, rule.id);
        assert_eq!(fixture.mechanism, rule.mechanism.as_str());
        assert_eq!(fixture.recoverability, rule.recoverability.as_str());
        assert_eq!(fixture.sensitivity, rule.sensitivity.as_str());
        assert!(rule.subjects.iter().any(|subject| match subject {
            RuleSubjectPattern::FilesystemPath { pattern } => {
                fixture_path_matches(pattern, &fixture.expected_path)
            }
            RuleSubjectPattern::ToolchainObjectSet { .. } => false,
        }));
        assert_eq!(
            rule.selection_override.is_some(),
            rule.override_reason.is_some(),
            "selection override and reason must appear together for {}",
            rule.id
        );
    }
}

#[test]
fn toolchain_object_set_rules_are_typed_and_never_accept_commands() {
    let source = r#"
[[rule]]
id = "xcode.fixture_objects"
adapter = "xcode"
title = "Fixture objects"
description = "Typed toolchain objects."
subjects = [{ kind = "toolchain_object_set", object_set_id = "xcode.fixture_objects" }]
os = ">=13.0"
mechanism = "vendor_managed"
recoverability = "redownload_bandwidth"
sensitivity = "medium"
evidence = "The fixture exercises a typed non-filesystem subject."
fixture_id = "xcode-fixture-objects"
preconditions = { process_not_running = [] }
"#;

    let rules = sizetrail::rules::parse(source).expect("typed object-set rule must parse");
    assert_eq!(
        rules[0].subjects,
        [RuleSubjectPattern::ToolchainObjectSet {
            object_set_id: "xcode.fixture_objects".to_owned()
        }]
    );
    assert_eq!(
        sizetrail::rules::parse(&format!("{source}\ncommand = \"docker image prune\"\n")),
        Err(RuleError::UnknownField)
    );
}

fn fixture_path_matches(pattern: &str, path: &str) -> bool {
    let Some((prefix, _)) = pattern.split_once('*') else {
        return pattern == path;
    };
    let Some(mut remaining) = path.strip_prefix(prefix) else {
        return false;
    };
    let parts = pattern.split('*').skip(1).collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        let last = index + 1 == parts.len();
        if last && !pattern.ends_with('*') {
            return remaining.ends_with(part);
        }
        let Some(position) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[position + part.len()..];
    }
    true
}

#[test]
fn selection_is_derived_from_orthogonal_fields() {
    let rules = builtin_rules().expect("compiled rules must parse and validate");
    let derived = rules
        .iter()
        .find(|rule| rule.id == "xcode.derived_data_build")
        .expect("derived data rule must exist");
    let archives = rules
        .iter()
        .find(|rule| rule.id == "xcode.archives")
        .expect("archives rule must exist");

    assert!(default_selected(derived, true));
    assert!(!default_selected(derived, false));
    assert!(!default_selected(archives, true));
}

#[test]
fn arbitrary_commands_are_not_part_of_the_rule_schema() {
    let source = include_str!("../src/rules/builtin/xcode.toml");
    let mutated = format!("{source}\ncommand = \"rm -rf /\"\n");

    assert_eq!(
        sizetrail::rules::parse(&mutated),
        Err(RuleError::UnknownField)
    );
}

#[test]
fn homebrew_rules_cover_only_the_eight_decided_store_classes() {
    let rules = builtin_rules().expect("compiled rules must parse and validate");
    let homebrew = rules
        .iter()
        .filter(|rule| rule.adapter == "homebrew")
        .collect::<Vec<_>>();
    assert_eq!(COMPILED_ADAPTER_IDS, ["docker", "homebrew", "xcode"]);
    assert_eq!(
        homebrew
            .iter()
            .map(|rule| rule.id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "homebrew.cache_api",
            "homebrew.cache_bootsnap",
            "homebrew.cache_build_tools",
            "homebrew.cache_downloads",
            "homebrew.caskroom",
            "homebrew.cellar",
            "homebrew.logs",
            "homebrew.taps",
        ])
    );
    let logs = homebrew
        .iter()
        .find(|rule| rule.id == "homebrew.logs")
        .expect("logs rule must exist");
    assert!(logs.evidence.contains("user state"));
    assert!(logs.evidence.contains("cannot be regenerated"));
    let cellar = homebrew
        .iter()
        .find(|rule| rule.id == "homebrew.cellar")
        .expect("Cellar rule must exist");
    assert!(cellar.evidence.contains("installed software"));
    assert!(!cellar.evidence.contains("reclaim"));
}
