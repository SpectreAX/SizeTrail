use std::collections::BTreeSet;
use std::path::Path;

use sizetrail::rules::{COMPILED_ADAPTER_IDS, RuleError, builtin_rules, default_selected};

#[derive(serde::Deserialize)]
struct RuleFixture {
    rule_id: String,
    mechanism: String,
    recoverability: String,
    sensitivity: String,
    expected_path: String,
}

#[test]
fn every_builtin_rule_has_evidence_paths_adapter_and_fixture() {
    let rules = builtin_rules().expect("compiled rules must parse and validate");
    assert!(!rules.is_empty());

    let ids = rules
        .iter()
        .map(|rule| rule.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), rules.len());
    for rule in &rules {
        assert!(!rule.evidence.trim().is_empty());
        assert!(!rule.paths.is_empty());
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
        assert!(
            rule.paths
                .iter()
                .any(|pattern| fixture_path_matches(pattern, &fixture.expected_path))
        );
        assert_eq!(
            rule.selection_override.is_some(),
            rule.override_reason.is_some(),
            "selection override and reason must appear together for {}",
            rule.id
        );
    }
}

fn fixture_path_matches(pattern: &str, path: &str) -> bool {
    let Some((prefix, suffix)) = pattern.split_once('*') else {
        return pattern == path;
    };
    path.starts_with(prefix) && path.ends_with(suffix)
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
