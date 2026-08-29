use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::adapters::{
    AdapterDegradedReason, AdapterId, AdapterState, Inventory, InventoryGap, InventoryGapReason,
    InventoryIdentity, InventoryItem, InventoryStage, ToolchainAdapter,
};
use crate::fsx::{Root, RootError};
use crate::model::{
    Advice, AdviceImpact, CommandAdvice, Finding, RevealAdvice, finding_id, normalize_findings,
    normalized_report_path,
};
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
    pub fn physical_path_for_reported(&self, root: &Root, path: &Path) -> Option<PathBuf> {
        path.strip_prefix(&self.reported_prefix)
            .ok()
            .map(|relative| root.path().join(relative))
    }

    pub(crate) fn normalized_prefix_path(
        &self,
        prefix_root: Option<&Root>,
        path: &Path,
    ) -> Option<String> {
        let physical_prefix = prefix_root.map_or(self.prefix.as_path(), Root::path);
        let relative = path.strip_prefix(physical_prefix).ok()?;
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
    prefix_root: Option<&'a Root>,
    prefix_gap: Option<InventoryGapReason>,
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
            prefix_root: Some(prefix_root),
            prefix_gap: None,
            layout,
            excludes,
            home_volume_has_snapshots,
            prefix_volume_has_snapshots,
        }
    }

    #[must_use]
    pub const fn without_prefix(
        home_root: &'a Root,
        layout: &'a Layout,
        excludes: &'a [PathBuf],
        home_volume_has_snapshots: Result<bool, Option<i32>>,
        prefix_gap: InventoryGapReason,
    ) -> Self {
        Self {
            home_root,
            prefix_root: None,
            prefix_gap: Some(prefix_gap),
            layout,
            excludes,
            home_volume_has_snapshots,
            prefix_volume_has_snapshots: Ok(false),
        }
    }
}

impl ToolchainAdapter for HomebrewAdapter<'_> {
    fn id(&self) -> AdapterId {
        AdapterId::new("homebrew")
    }

    fn probe(&self, ctx: &mut PolicyCtx<'_>) -> AdapterState {
        let Some(root) = self.prefix_root else {
            return unknown_version();
        };
        let repository = self
            .layout
            .repository
            .strip_prefix(&self.layout.prefix)
            .map_or_else(
                |_| self.layout.repository.clone(),
                |relative| root.path().join(relative),
            );
        probe_version_in_root(root, &repository, ctx)
    }

    fn inventory(&self, _ctx: &mut PolicyCtx<'_>, state: &AdapterState) -> Inventory {
        if matches!(state, AdapterState::NotPresent) {
            return Inventory::default();
        }
        let mut inventory = self.inventory_stores();
        if let AdapterState::Degraded { reason, .. } = state {
            inventory
                .gaps
                .push(InventoryGap::diagnostic("homebrew", degraded_gap(*reason)));
        }
        inventory
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
            let mut finding = Finding {
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
            };
            finding.advice = self.advise(&finding);
            findings.push(finding);
        }
        normalize_findings(&mut findings);
        Ok(findings)
    }

    fn advise(&self, finding: &Finding) -> Vec<Advice> {
        if finding.rule_id.starts_with("homebrew.cache_") {
            vec![Advice::Command(CommandAdvice {
                display_command: "HOMEBREW_NO_AUTOREMOVE=1 brew cleanup".to_owned(),
                impact: AdviceImpact::Destructive,
                explanation: "Homebrew cleanup deletes data and may otherwise run autoremove. Its `-n` mode is not a reliable preview because it omits unreferenced downloads and cache-database cleanup.".to_owned(),
                reliable_preview_available: false,
            })]
        } else {
            vec![Advice::Reveal(RevealAdvice {
                normalized_path: finding.normalized_path.clone(),
                recovery_semantics: finding.evidence.clone(),
            })]
        }
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
        if let Some(reason) = self.prefix_gap {
            inventory.gaps.push(InventoryGap {
                region: "homebrew.prefix",
                path: Some(self.layout.prefix.clone()),
                reason,
                stage: Some(InventoryStage::RootInitialization),
                errno: None,
            });
        }
        let snapshot_states = std::iter::once(("homebrew.home", self.home_volume_has_snapshots))
            .chain(
                self.prefix_root
                    .map(|_| ("homebrew.prefix", self.prefix_volume_has_snapshots)),
            );
        for (region, snapshot_state) in snapshot_states {
            if let Err(errno) = snapshot_state {
                inventory.gaps.push(InventoryGap {
                    region,
                    path: None,
                    reason: InventoryGapReason::VolumeSnapshotStateUnavailable,
                    stage: Some(InventoryStage::VolumeSnapshots),
                    errno,
                });
            }
        }
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
                    self.prefix_root.map_or_else(
                        || Ok(Vec::new()),
                        |prefix_root| {
                            expand_pattern(prefix_root, prefix_root.path(), relative, self.excludes)
                                .map(|paths| {
                                    paths
                                        .into_iter()
                                        .map(|path| (prefix_root, path, false))
                                        .collect::<Vec<_>>()
                                })
                        },
                    )
                } else if let (Some(cellar), Some(relative)) =
                    (&self.layout.cellar, pattern.strip_prefix("$CELLAR/"))
                {
                    self.prefix_root.map_or_else(
                        || Ok(Vec::new()),
                        |prefix_root| {
                            let cellar = cellar.strip_prefix(&self.layout.prefix).map_or_else(
                                |_| cellar.to_path_buf(),
                                |path| prefix_root.path().join(path),
                            );
                            expand_pattern(prefix_root, &cellar, relative, self.excludes).map(
                                |paths| {
                                    paths
                                        .into_iter()
                                        .map(|path| (prefix_root, path, false))
                                        .collect::<Vec<_>>()
                                },
                            )
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
                if stores.is_empty()
                    && rule.id.starts_with("homebrew.cache_")
                    && pattern.starts_with("~/")
                {
                    inventory.gaps.push(InventoryGap {
                        region: "homebrew.cache",
                        path: pattern
                            .strip_prefix("~/")
                            .map(|relative| self.home_root.path().join(relative)),
                        reason: InventoryGapReason::AbsentOrChanged,
                        stage: Some(InventoryStage::RuleEvaluation),
                        errno: None,
                    });
                }
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
            self.layout.normalized_prefix_path(Some(root), &path)
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
        if rule_id == "homebrew.caskroom" {
            match cask_artifact_outside_prefix(root, &path, &self.layout.reported_prefix) {
                Ok(true) => inventory.gaps.push(InventoryGap {
                    region: "homebrew.caskroom",
                    path: Some(path),
                    reason: InventoryGapReason::CaskArtifactOutsidePrefix,
                    stage: Some(InventoryStage::RuleEvaluation),
                    errno: None,
                }),
                Ok(false) => {}
                Err(error) => {
                    inventory
                        .gaps
                        .push(io_gap(&path, InventoryStage::RuleEvaluation, &error))
                }
            }
        }
    }
}

pub fn probe_version(repository: &Path, _ctx: &mut PolicyCtx<'_>) -> AdapterState {
    Root::open(repository)
        .ok()
        .and_then(|root| read_version(&root, root.path()))
        .map_or_else(unknown_version, |version| AdapterState::Ready { version })
}

fn probe_version_in_root(root: &Root, repository: &Path, _ctx: &mut PolicyCtx<'_>) -> AdapterState {
    read_version(root, repository)
        .map_or_else(unknown_version, |version| AdapterState::Ready { version })
}

fn unknown_version() -> AdapterState {
    AdapterState::Degraded {
        observed_version: None,
        reason: AdapterDegradedReason::UnknownVersion,
    }
}

fn read_version(root: &Root, repository: &Path) -> Option<String> {
    let git = repository.join(".git");
    let head = read_checked(root, &git.join("HEAD"))?;
    let head = std::str::from_utf8(&head).ok()?;
    let head = head.trim();
    let sha = if let Some(reference) = head.strip_prefix("ref: ") {
        read_reference(root, &git, reference)?
    } else {
        valid_sha(head).then(|| head.to_owned())?
    };
    let version = read_checked(root, &git.join("describe-cache").join(sha))?;
    let version = std::str::from_utf8(&version).ok()?;
    let version = version.trim();
    (!version.is_empty() && !version.chars().any(char::is_whitespace)).then(|| version.to_owned())
}

fn read_reference(root: &Root, git: &Path, reference: &str) -> Option<String> {
    let reference_path = Path::new(reference);
    if !reference.starts_with("refs/")
        || !reference_path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    if let Some(contents) = read_checked(root, &git.join(reference_path))
        .and_then(|contents| String::from_utf8(contents).ok())
    {
        let sha = contents.trim();
        return valid_sha(sha).then(|| sha.to_owned());
    }
    let packed = read_checked(root, &git.join("packed-refs"))?;
    let packed = std::str::from_utf8(&packed).ok()?;
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
    let installed_on_request = read_checked(root, &receipt_path)
        .and_then(|contents| serde_json::from_slice::<serde_json::Value>(&contents).ok())
        .and_then(|receipt| receipt["installed_on_request"].as_bool());
    InventoryIdentity::HomebrewKeg {
        formula,
        version,
        installed_on_request,
    }
}

fn read_checked(root: &Root, path: &Path) -> Option<Vec<u8>> {
    let measured = root.measure_object(path).ok()?;
    if measured.dataless || !std::fs::symlink_metadata(path).ok()?.file_type().is_file() {
        return None;
    }
    std::fs::read(path).ok()
}

fn io_gap(path: &Path, stage: InventoryStage, error: &io::Error) -> InventoryGap {
    let errno = error.raw_os_error();
    InventoryGap {
        region: "homebrew",
        path: Some(path.to_path_buf()),
        reason: match errno {
            Some(2) => InventoryGapReason::AbsentOrChanged,
            Some(13) => InventoryGapReason::AccessDenied,
            Some(1) => InventoryGapReason::PolicyDeniedUnknown,
            _ => InventoryGapReason::TraversalFailed,
        },
        stage: Some(stage),
        errno,
    }
}

fn degraded_gap(reason: AdapterDegradedReason) -> InventoryGapReason {
    match reason {
        AdapterDegradedReason::UnknownVersion => InventoryGapReason::UnknownVersion,
        AdapterDegradedReason::NotReady => InventoryGapReason::NotReady,
        AdapterDegradedReason::Disabled => InventoryGapReason::Disabled,
        AdapterDegradedReason::ProbeFailed | AdapterDegradedReason::InvalidSelection => {
            InventoryGapReason::ProbeFailed
        }
    }
}

fn cask_artifact_outside_prefix(
    root: &Root,
    cask: &Path,
    reported_prefix: &Path,
) -> io::Result<bool> {
    let mut stack = vec![cask.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in root.children(&directory)? {
            match entry.kind {
                crate::fsx::RootEntryKind::Directory => stack.push(entry.path),
                crate::fsx::RootEntryKind::Symlink => {
                    let target = std::fs::read_link(&entry.path)?;
                    let outside = if target.is_absolute() {
                        let Some(target) = normalize_absolute(&target) else {
                            return Ok(true);
                        };
                        !target.starts_with(reported_prefix) && !target.starts_with(root.path())
                    } else {
                        let Some(target) = entry
                            .path
                            .parent()
                            .and_then(|parent| normalize_absolute(&parent.join(target)))
                        else {
                            return Ok(true);
                        };
                        !target.starts_with(root.path())
                    };
                    if outside {
                        return Ok(true);
                    }
                }
                crate::fsx::RootEntryKind::File | crate::fsx::RootEntryKind::Other => {}
            }
        }
    }
    Ok(false)
}

fn normalize_absolute(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(value) => normalized.push(value),
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}
