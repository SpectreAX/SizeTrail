use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::adapters::{
    AdapterDegradedReason, AdapterId, AdapterState, Inventory, InventoryGap, InventoryGapReason,
    InventoryIdentity, InventoryItem, InventoryStage, ToolchainAdapter,
};
use crate::fsx::{Root, RootError};
use crate::model::{Advice, Finding, finding_id, normalize_findings, normalized_report_path};
use crate::policy::PolicyCtx;
use crate::rules::builtin_rules;

use super::store::{expand_home_pattern, expand_pattern, measure_store_as};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    pub prefix: PathBuf,
    pub repository: PathBuf,
    pub cellar: Option<PathBuf>,
    reported_prefix: PathBuf,
}

pub fn discover_layout(sandbox_root: Option<&Path>) -> Option<Layout> {
    [
        (Path::new("/opt/homebrew"), Path::new("/opt/homebrew")),
        (Path::new("/usr/local"), Path::new("/usr/local/Homebrew")),
    ]
    .into_iter()
    .find_map(|(prefix, repository)| {
        let prefix = under_sandbox(sandbox_root, prefix);
        if std::fs::symlink_metadata(prefix.join("bin/brew")).is_err()
            || (std::fs::symlink_metadata(prefix.join("Cellar")).is_err()
                && std::fs::symlink_metadata(prefix.join("Caskroom")).is_err())
        {
            return None;
        }
        let reported_prefix = prefix_for_report(prefix.as_path(), sandbox_root);
        let repository = under_sandbox(sandbox_root, repository);
        let cellar = if std::fs::symlink_metadata(prefix.join("Cellar")).is_ok() {
            Some(prefix.join("Cellar"))
        } else if std::fs::symlink_metadata(repository.join("Cellar")).is_ok() {
            Some(repository.join("Cellar"))
        } else {
            None
        };
        Some(Layout {
            prefix,
            repository,
            cellar,
            reported_prefix,
        })
    })
}

pub fn open_prefix_root(layout: &Layout) -> Result<Root, RootError> {
    Root::open(&layout.prefix)
}

impl Layout {
    fn normalized_prefix_path(&self, prefix_root: &Root, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(prefix_root.path()).ok()?;
        Some(
            self.reported_prefix
                .join(relative)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

pub struct HomebrewAdapter<'a> {
    home_root: &'a Root,
    prefix_root: &'a Root,
    layout: &'a Layout,
    excludes: &'a [PathBuf],
    home_volume_has_snapshots: Result<bool, Option<i32>>,
    prefix_volume_has_snapshots: Result<bool, Option<i32>>,
}

impl<'a> HomebrewAdapter<'a> {
    #[must_use]
    pub const fn new(
        home_root: &'a Root,
        prefix_root: &'a Root,
        layout: &'a Layout,
        excludes: &'a [PathBuf],
        home_volume_has_snapshots: Result<bool, Option<i32>>,
        prefix_volume_has_snapshots: Result<bool, Option<i32>>,
    ) -> Self {
        Self {
            home_root,
            prefix_root,
            layout,
            excludes,
            home_volume_has_snapshots,
            prefix_volume_has_snapshots,
        }
    }
}

impl ToolchainAdapter for HomebrewAdapter<'_> {
    fn id(&self) -> AdapterId {
        AdapterId::new("homebrew")
    }

    fn probe(&self, ctx: &mut PolicyCtx<'_>) -> AdapterState {
        probe_version(&self.layout.repository, ctx)
    }

    fn inventory(&self, _ctx: &mut PolicyCtx<'_>, state: &AdapterState) -> Inventory {
        if matches!(state, AdapterState::NotPresent) {
            return Inventory::default();
        }
        self.inventory_stores()
    }

    fn classify(&self, inventory: &Inventory) -> Result<Vec<Finding>, InventoryGapReason> {
        let rules = builtin_rules().map_err(|_| InventoryGapReason::RuleSetInvalid)?;
        let rules = rules
            .into_iter()
            .filter(|rule| rule.adapter == "homebrew")
            .map(|rule| (rule.id.clone(), rule))
            .collect::<BTreeMap<_, _>>();
        let mut findings = Vec::with_capacity(inventory.items.len());
        for item in &inventory.items {
            let rule = rules
                .get(&item.rule_id)
                .ok_or(InventoryGapReason::RuleSetInvalid)?;
            findings.push(Finding {
                id: finding_id("homebrew", &item.rule_id, &item.normalized_path)
                    .map_err(|_| InventoryGapReason::RuleSetInvalid)?,
                adapter_id: "homebrew".to_owned(),
                rule_id: item.rule_id.clone(),
                title: rule.title.clone(),
                summary: String::new(),
                normalized_path: item.normalized_path.clone(),
                mechanism: rule.mechanism.as_str().to_owned(),
                recoverability: rule.recoverability.as_str().to_owned(),
                sensitivity: rule.sensitivity.as_str().to_owned(),
                evidence: rule.evidence.clone(),
                unexplained_private_gap: true,
                measurements: item.measurements.clone(),
                observations: item.observations.clone(),
                advice: Vec::new(),
            });
        }
        normalize_findings(&mut findings);
        Ok(findings)
    }

    fn advise(&self, _finding: &Finding) -> Vec<Advice> {
        Vec::new()
    }
}

impl HomebrewAdapter<'_> {
    fn inventory_stores(&self) -> Inventory {
        let Ok(rules) = builtin_rules() else {
            return Inventory {
                gaps: vec![InventoryGap::diagnostic(
                    "homebrew",
                    InventoryGapReason::RuleSetInvalid,
                )],
                ..Inventory::default()
            };
        };
        let mut inventory = Inventory::default();
        for rule in rules.iter().filter(|rule| rule.adapter == "homebrew") {
            for pattern in &rule.paths {
                let expansion = if pattern.starts_with("~/") {
                    expand_home_pattern(self.home_root, pattern, self.excludes).map(|paths| {
                        paths
                            .into_iter()
                            .map(|path| (self.home_root, path, true))
                            .collect::<Vec<_>>()
                    })
                } else if let Some(relative) = pattern.strip_prefix("$PREFIX/") {
                    expand_pattern(
                        self.prefix_root,
                        self.prefix_root.path(),
                        relative,
                        self.excludes,
                    )
                    .map(|paths| {
                        paths
                            .into_iter()
                            .map(|path| (self.prefix_root, path, false))
                            .collect::<Vec<_>>()
                    })
                } else if let (Some(cellar), Some(relative)) =
                    (&self.layout.cellar, pattern.strip_prefix("$CELLAR/"))
                {
                    let cellar = cellar.strip_prefix(&self.layout.prefix).map_or_else(
                        |_| cellar.to_path_buf(),
                        |path| self.prefix_root.path().join(path),
                    );
                    expand_pattern(self.prefix_root, &cellar, relative, self.excludes).map(
                        |paths| {
                            paths
                                .into_iter()
                                .map(|path| (self.prefix_root, path, false))
                                .collect::<Vec<_>>()
                        },
                    )
                } else {
                    Ok(Vec::new())
                };
                let stores = match expansion {
                    Ok(stores) => stores,
                    Err((path, error)) => {
                        inventory
                            .gaps
                            .push(io_gap(&path, InventoryStage::ListDirectory, &error));
                        continue;
                    }
                };
                for (root, path, home_side) in stores {
                    self.measure_rule_store(
                        rule.id.as_str(),
                        root,
                        path,
                        home_side,
                        &mut inventory,
                    );
                }
            }
        }
        inventory.items.sort_by(|left, right| {
            left.rule_id
                .cmp(&right.rule_id)
                .then_with(|| left.normalized_path.cmp(&right.normalized_path))
        });
        inventory
    }

    fn measure_rule_store(
        &self,
        rule_id: &str,
        root: &Root,
        path: PathBuf,
        home_side: bool,
        inventory: &mut Inventory,
    ) {
        let normalized_path = if home_side {
            normalized_report_path(self.home_root.path(), &path).ok()
        } else {
            self.layout.normalized_prefix_path(self.prefix_root, &path)
        };
        let Some(normalized_path) = normalized_path else {
            inventory.gaps.push(InventoryGap {
                region: "homebrew",
                path: Some(path),
                reason: InventoryGapReason::TraversalFailed,
                stage: Some(InventoryStage::NormalizePath),
                errno: None,
            });
            return;
        };
        let snapshots = if home_side {
            self.home_volume_has_snapshots.unwrap_or(false)
        } else {
            self.prefix_volume_has_snapshots.unwrap_or(false)
        };
        let measured = measure_store_as(
            root,
            &path,
            self.excludes,
            snapshots,
            normalized_path.clone(),
        );
        let (measurements, observations) = match measured {
            Ok(measured) => measured,
            Err(error) => {
                inventory
                    .gaps
                    .push(io_gap(&path, InventoryStage::MeasureObject, &error));
                return;
            }
        };
        inventory.items.push(InventoryItem {
            rule_id: rule_id.to_owned(),
            normalized_path,
            path: Some(path.clone()),
            measurements,
            observations,
            identity: if rule_id == "homebrew.cellar" {
                keg_identity(root, &path)
            } else {
                InventoryIdentity::Path
            },
        });
    }
}

pub fn probe_version(repository: &Path, _ctx: &mut PolicyCtx<'_>) -> AdapterState {
    read_version(repository).map_or_else(
        || AdapterState::Degraded {
            observed_version: None,
            reason: AdapterDegradedReason::UnknownVersion,
        },
        |version| AdapterState::Ready { version },
    )
}

fn read_version(repository: &Path) -> Option<String> {
    let git = repository.join(".git");
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    let sha = if let Some(reference) = head.strip_prefix("ref: ") {
        read_reference(&git, reference)?
    } else {
        valid_sha(head).then(|| head.to_owned())?
    };
    let version = std::fs::read_to_string(git.join("describe-cache").join(sha)).ok()?;
    let version = version.trim();
    (!version.is_empty() && !version.chars().any(char::is_whitespace)).then(|| version.to_owned())
}

fn read_reference(git: &Path, reference: &str) -> Option<String> {
    let reference_path = Path::new(reference);
    if !reference.starts_with("refs/")
        || !reference_path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    if let Ok(contents) = std::fs::read_to_string(git.join(reference_path)) {
        let sha = contents.trim();
        return valid_sha(sha).then(|| sha.to_owned());
    }
    let packed = std::fs::read_to_string(git.join("packed-refs")).ok()?;
    packed.lines().find_map(|line| {
        let (sha, candidate) = line.split_once(' ')?;
        (candidate == reference && valid_sha(sha)).then(|| sha.to_owned())
    })
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn under_sandbox(sandbox_root: Option<&Path>, absolute: &Path) -> PathBuf {
    sandbox_root.map_or_else(
        || absolute.to_path_buf(),
        |root| root.join(absolute.strip_prefix(Path::new("/")).unwrap_or(absolute)),
    )
}

fn prefix_for_report(physical_prefix: &Path, sandbox_root: Option<&Path>) -> PathBuf {
    if sandbox_root.is_none() {
        return physical_prefix.to_path_buf();
    }
    if physical_prefix.ends_with("opt/homebrew") {
        PathBuf::from("/opt/homebrew")
    } else {
        PathBuf::from("/usr/local")
    }
}

fn keg_identity(root: &Root, path: &Path) -> InventoryIdentity {
    let version = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let formula = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let receipt_path = path.join("INSTALL_RECEIPT.json");
    let installed_on_request = root
        .measure_object(&receipt_path)
        .ok()
        .filter(|_| {
            std::fs::symlink_metadata(&receipt_path)
                .is_ok_and(|metadata| metadata.file_type().is_file())
        })
        .and_then(|_| std::fs::read(receipt_path).ok())
        .and_then(|contents| serde_json::from_slice::<serde_json::Value>(&contents).ok())
        .and_then(|receipt| receipt["installed_on_request"].as_bool());
    InventoryIdentity::HomebrewKeg {
        formula,
        version,
        installed_on_request,
    }
}

fn io_gap(path: &Path, stage: InventoryStage, error: &io::Error) -> InventoryGap {
    InventoryGap {
        region: "homebrew",
        path: Some(path.to_path_buf()),
        reason: InventoryGapReason::TraversalFailed,
        stage: Some(stage),
        errno: error.raw_os_error(),
    }
}
