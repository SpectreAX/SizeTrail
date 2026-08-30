use serde::{Deserialize, Serialize};

const BUILTIN_HOMEBREW: &str = include_str!("builtin/homebrew.toml");
const BUILTIN_XCODE: &str = include_str!("builtin/xcode.toml");

pub const COMPILED_ADAPTER_IDS: &[&str] = &["homebrew", "xcode"];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct RuleTable {
    rule: Vec<Rule>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub adapter: String,
    pub title: String,
    pub description: String,
    pub subjects: Vec<RuleSubjectPattern>,
    pub os: String,
    pub mechanism: Mechanism,
    pub recoverability: Recoverability,
    pub sensitivity: Sensitivity,
    pub evidence: String,
    pub fixture_id: String,
    pub preconditions: Preconditions,
    pub selection_override: Option<bool>,
    pub override_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleSubjectPattern {
    FilesystemPath { pattern: String },
    ToolchainObjectSet { object_set_id: String },
}

impl Rule {
    pub fn filesystem_patterns(&self) -> impl Iterator<Item = &str> {
        self.subjects.iter().filter_map(|subject| match subject {
            RuleSubjectPattern::FilesystemPath { pattern } => Some(pattern.as_str()),
            RuleSubjectPattern::ToolchainObjectSet { .. } => None,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mechanism {
    Generated,
    UserAdjacent,
    UserOwned,
    VendorManaged,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Recoverability {
    TrashRestore,
    RebuildTimeCost,
    RedownloadBandwidth,
    RequiresExternalDevice,
    Unrecoverable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Low,
    Medium,
    High,
}

impl Mechanism {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::UserAdjacent => "user_adjacent",
            Self::UserOwned => "user_owned",
            Self::VendorManaged => "vendor_managed",
        }
    }
}

impl Recoverability {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TrashRestore => "trash_restore",
            Self::RebuildTimeCost => "rebuild_time_cost",
            Self::RedownloadBandwidth => "redownload_bandwidth",
            Self::RequiresExternalDevice => "requires_external_device",
            Self::Unrecoverable => "unrecoverable",
        }
    }
}

impl Sensitivity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preconditions {
    #[serde(default)]
    pub process_not_running: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuleError {
    UnknownField,
    Invalid,
}

pub fn builtin_rules() -> Result<Vec<Rule>, RuleError> {
    let mut rules = parse(BUILTIN_HOMEBREW)?;
    rules.extend(parse(BUILTIN_XCODE)?);
    Ok(rules)
}

pub fn parse(source: &str) -> Result<Vec<Rule>, RuleError> {
    let table = toml::from_str::<RuleTable>(source).map_err(|error| {
        if error.to_string().contains("unknown field") {
            RuleError::UnknownField
        } else {
            RuleError::Invalid
        }
    })?;
    if table.rule.is_empty() || table.rule.iter().any(invalid) {
        return Err(RuleError::Invalid);
    }
    Ok(table.rule)
}

#[must_use]
pub fn default_selected(rule: &Rule, preconditions_met: bool) -> bool {
    rule.selection_override.unwrap_or(
        preconditions_met
            && rule.sensitivity == Sensitivity::Low
            && matches!(
                rule.recoverability,
                Recoverability::RebuildTimeCost | Recoverability::RedownloadBandwidth
            ),
    )
}

fn invalid(rule: &Rule) -> bool {
    rule.id.trim().is_empty()
        || rule.adapter.trim().is_empty()
        || rule.title.trim().is_empty()
        || rule.description.trim().is_empty()
        || rule.subjects.is_empty()
        || rule.subjects.iter().any(|subject| match subject {
            RuleSubjectPattern::FilesystemPath { pattern } => pattern.trim().is_empty(),
            RuleSubjectPattern::ToolchainObjectSet { object_set_id } => {
                object_set_id.trim().is_empty()
            }
        })
        || rule.os != ">=13.0"
        || rule.evidence.trim().is_empty()
        || rule.fixture_id.trim().is_empty()
        || !COMPILED_ADAPTER_IDS.contains(&rule.adapter.as_str())
        || rule.selection_override.is_some() != rule.override_reason.is_some()
        || rule
            .override_reason
            .as_ref()
            .is_some_and(|reason| reason.trim().is_empty())
}
