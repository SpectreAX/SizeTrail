use std::collections::BTreeMap;
use std::io;
use std::path::{Component, PathBuf};

use crate::adapters::store::{excluded, measure_store_as};
use crate::adapters::{
    AdapterDegradedReason, AdapterId, AdapterState, Inventory, InventoryGap, InventoryGapReason,
    InventoryIdentity, InventoryItem, InventoryStage, ToolchainAdapter,
};
use crate::fsx::Root;
use crate::model::{
    Advice, AdviceImpact, CommandAdvice, Finding, FindingSubject, finding_id, normalize_findings,
    normalized_report_path,
};
use crate::policy::{GO_HOMEBREW_VERSION, GO_PKG_VERSION, PolicyCtx, PolicyError, ProbeId};
use crate::rules::builtin_rules;

pub const HOMEBREW_VERSION: ProbeId = GO_HOMEBREW_VERSION;
pub const PKG_VERSION: ProbeId = GO_PKG_VERSION;

const VERIFIED_VERSIONS: &[&str] = &["go1.26.6"];
const HOMEBREW_GO: &str = "/opt/homebrew/bin/go";
const PKG_GO: &str = "/usr/local/go/bin/go";
pub const BUILD_CACHE_RELATIVE: &str = "Library/Caches/go-build";
pub const MODULE_CACHE_RELATIVE: &str = "go/pkg/mod";
const GOENV_RELATIVE: &str = "Library/Application Support/go/env";

pub struct GoAdapter<'a> {
    home_root: &'a Root,
    excludes: &'a [PathBuf],
}

impl<'a> GoAdapter<'a> {
    #[must_use]
    pub const fn new(home_root: &'a Root, excludes: &'a [PathBuf]) -> Self {
        Self {
            home_root,
            excludes,
        }
    }

    #[must_use]
    pub fn discover_cache_roots(home_root: &'a Root) -> Vec<PathBuf> {
        match Self::new(home_root, &[]).resolved_caches() {
            Ok(caches) => caches.into_iter().map(|cache| cache.path).collect(),
            Err(_) => vec![
                home_root.path().join(BUILD_CACHE_RELATIVE),
                home_root.path().join(MODULE_CACHE_RELATIVE),
            ],
        }
    }
}

struct CacheRoot {
    rule_id: &'static str,
    path: PathBuf,
}

type GoenvOverrides = (Option<PathBuf>, Option<PathBuf>);

impl ToolchainAdapter for GoAdapter<'_> {
    fn id(&self) -> AdapterId {
        AdapterId::new("go")
    }

    fn probe(&self, ctx: &mut PolicyCtx<'_>) -> AdapterState {
        probe(ctx)
    }

    fn inventory(&self, _ctx: &mut PolicyCtx<'_>, state: &AdapterState) -> Inventory {
        let mut inventory = Inventory::default();
        if let AdapterState::Degraded { reason, .. } = state
            && matches!(
                reason,
                AdapterDegradedReason::Disabled | AdapterDegradedReason::ProbeFailed
            )
        {
            inventory
                .gaps
                .push(InventoryGap::diagnostic("go", gap_from_degraded(*reason)));
        }
        let caches = match self.resolved_caches() {
            Ok(caches) => caches,
            Err(gap) => {
                inventory.gaps.push(gap);
                return inventory;
            }
        };
        let snapshots = self.home_root.volume_has_snapshots().unwrap_or(false);
        for cache in caches {
            if excluded(&cache.path, self.excludes) {
                continue;
            }
            match self.measure_cache(&cache, snapshots) {
                Ok(Some(item)) => inventory.items.push(item),
                Ok(None) => {}
                Err(gap) => inventory.gaps.push(gap),
            }
        }
        inventory
    }

    fn classify(&self, inventory: &Inventory) -> Result<Vec<Finding>, InventoryGapReason> {
        let rules = builtin_rules().map_err(|_| InventoryGapReason::RuleSetInvalid)?;
        let rules = rules
            .into_iter()
            .filter(|rule| rule.adapter == "go")
            .map(|rule| (rule.id.clone(), rule))
            .collect::<BTreeMap<_, _>>();
        let mut findings = Vec::with_capacity(inventory.items.len());
        for item in &inventory.items {
            let rule = rules
                .get(&item.rule_id)
                .ok_or(InventoryGapReason::RuleSetInvalid)?;
            let subject_key = item
                .subject
                .canonical_key()
                .map_err(|_| InventoryGapReason::RuleSetInvalid)?;
            let mut finding = Finding {
                id: finding_id("go", &item.rule_id, &subject_key)
                    .map_err(|_| InventoryGapReason::RuleSetInvalid)?,
                adapter_id: "go".to_owned(),
                rule_id: item.rule_id.clone(),
                title: rule.title.clone(),
                summary: String::new(),
                subject: item.subject.clone(),
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
        match finding.rule_id.as_str() {
            "go.build_cache" => vec![Advice::Command(CommandAdvice {
                display_command: "go clean -cache".to_owned(),
                impact: AdviceImpact::Destructive,
                explanation: "This vendor command deletes the Go build cache. SizeTrail does not run it, and the vendor does not provide a reliable preview.".to_owned(),
                reliable_preview_available: false,
            })],
            "go.module_cache" => vec![Advice::Command(CommandAdvice {
                display_command: "go clean -modcache".to_owned(),
                impact: AdviceImpact::Destructive,
                explanation: "This vendor command deletes downloaded modules from the module cache. SizeTrail does not run it, and the vendor does not provide a reliable preview.".to_owned(),
                reliable_preview_available: false,
            })],
            _ => Vec::new(),
        }
    }
}

impl GoAdapter<'_> {
    fn resolved_caches(&self) -> Result<Vec<CacheRoot>, InventoryGap> {
        let mut build = self.home_root.path().join(BUILD_CACHE_RELATIVE);
        let mut module = self.home_root.path().join(MODULE_CACHE_RELATIVE);
        if let Some(overrides) = self.read_goenv()? {
            let (custom_build, custom_module) = overrides;
            if let Some(path) = custom_build {
                build = path;
            }
            if let Some(path) = custom_module {
                module = path;
            }
        }
        Ok(vec![
            CacheRoot {
                rule_id: "go.build_cache",
                path: build,
            },
            CacheRoot {
                rule_id: "go.module_cache",
                path: module,
            },
        ])
    }

    fn read_goenv(&self) -> Result<Option<GoenvOverrides>, InventoryGap> {
        let path = self.home_root.path().join(GOENV_RELATIVE);
        if excluded(&path, self.excludes) {
            return Ok(None);
        }
        match self.home_root.path_exists_without_descending(&path) {
            Ok(false) => return Ok(None),
            Ok(true) => {}
            Err(error) => return Err(io_gap(path, &error)),
        }
        let measured = self
            .home_root
            .measure_object(&path)
            .map_err(|error| io_gap(path.clone(), &error))?;
        if measured.dataless
            || !std::fs::symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.file_type().is_file())
        {
            return Err(InventoryGap {
                region: "go",
                path: Some(path),
                reason: InventoryGapReason::PolicyDeniedUnknown,
                stage: Some(InventoryStage::ToolchainProbe),
                errno: None,
            });
        }
        let contents = std::fs::read(&path).map_err(|error| io_gap(path.clone(), &error))?;
        parse_goenv(&contents).map(Some).ok_or(InventoryGap {
            region: "go",
            path: Some(path),
            reason: InventoryGapReason::InvalidToolOutput,
            stage: Some(InventoryStage::ToolchainProbe),
            errno: None,
        })
    }

    fn measure_cache(
        &self,
        cache: &CacheRoot,
        snapshots: bool,
    ) -> Result<Option<InventoryItem>, InventoryGap> {
        let exists = if cache.path.starts_with(self.home_root.path()) {
            self.home_root
                .path_exists_without_descending(&cache.path)
                .map_err(|error| io_gap(cache.path.clone(), &error))?
        } else {
            match Root::open(&cache.path) {
                Ok(_) => true,
                Err(_) => std::fs::symlink_metadata(&cache.path).is_ok(),
            }
        };
        if !exists {
            return Ok(None);
        }
        let (root, scope) = if cache.path.starts_with(self.home_root.path()) {
            let scope =
                normalized_report_path(self.home_root.path(), &cache.path).map_err(|_| {
                    InventoryGap {
                        region: cache.rule_id,
                        path: Some(cache.path.clone()),
                        reason: InventoryGapReason::TraversalFailed,
                        stage: Some(InventoryStage::NormalizePath),
                        errno: None,
                    }
                })?;
            (self.home_root, scope)
        } else {
            let opened = Root::open(&cache.path).map_err(|_| InventoryGap {
                region: cache.rule_id,
                path: Some(cache.path.clone()),
                reason: InventoryGapReason::TraversalFailed,
                stage: Some(InventoryStage::RootInitialization),
                errno: None,
            })?;
            let scope = cache.path.to_string_lossy().into_owned();
            return measure_opened(opened, cache, snapshots, scope);
        };
        let (measurements, observations) =
            measure_store_as(root, &cache.path, self.excludes, snapshots, scope.clone())
                .map_err(|error| io_gap(cache.path.clone(), &error))?;
        Ok(Some(InventoryItem {
            rule_id: cache.rule_id.to_owned(),
            subject: FindingSubject::FilesystemPath {
                normalized_path: scope,
            },
            path: Some(cache.path.clone()),
            measurements,
            observations,
            identity: InventoryIdentity::Path,
        }))
    }
}

fn measure_opened(
    root: Root,
    cache: &CacheRoot,
    snapshots: bool,
    scope: String,
) -> Result<Option<InventoryItem>, InventoryGap> {
    let (measurements, observations) =
        measure_store_as(&root, root.path(), &[], snapshots, scope.clone())
            .map_err(|error| io_gap(cache.path.clone(), &error))?;
    Ok(Some(InventoryItem {
        rule_id: cache.rule_id.to_owned(),
        subject: FindingSubject::FilesystemPath {
            normalized_path: scope,
        },
        path: Some(cache.path.clone()),
        measurements,
        observations,
        identity: InventoryIdentity::Path,
    }))
}

fn parse_goenv(contents: &[u8]) -> Option<GoenvOverrides> {
    let text = std::str::from_utf8(contents).ok()?;
    let mut build = None;
    let mut module = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        if !matches!(key, "GOCACHE" | "GOMODCACHE") {
            continue;
        }
        let path = PathBuf::from(value);
        if !path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        {
            return None;
        }
        match key {
            "GOCACHE" => build = Some(path),
            "GOMODCACHE" => module = Some(path),
            _ => {}
        }
    }
    Some((build, module))
}

fn io_gap(path: PathBuf, error: &io::Error) -> InventoryGap {
    let errno = error.raw_os_error();
    InventoryGap {
        region: "go",
        path: Some(path),
        reason: match errno {
            Some(2) => InventoryGapReason::AbsentOrChanged,
            Some(13) => InventoryGapReason::AccessDenied,
            Some(1) => InventoryGapReason::PolicyDeniedUnknown,
            _ => InventoryGapReason::TraversalFailed,
        },
        stage: Some(InventoryStage::MeasureObject),
        errno,
    }
}

pub fn probe(ctx: &mut PolicyCtx<'_>) -> AdapterState {
    let id = if go_exists(HOMEBREW_GO) {
        HOMEBREW_VERSION
    } else if go_exists(PKG_GO) {
        PKG_VERSION
    } else {
        return AdapterState::NotPresent;
    };
    match ctx.run(id) {
        Ok(output) if output.success => parse_verified_version(&output.stdout).map_or_else(
            || AdapterState::Degraded {
                observed_version: observed_version(&output.stdout),
                reason: AdapterDegradedReason::UnknownVersion,
            },
            |version| AdapterState::Ready { version },
        ),
        Ok(output) => AdapterState::Degraded {
            observed_version: observed_version(&output.stdout),
            reason: AdapterDegradedReason::ProbeFailed,
        },
        Err(PolicyError::InvocationFailed(_)) => AdapterState::NotPresent,
        Err(PolicyError::Disabled(_)) => AdapterState::Degraded {
            observed_version: None,
            reason: AdapterDegradedReason::Disabled,
        },
        Err(_) => AdapterState::Degraded {
            observed_version: None,
            reason: AdapterDegradedReason::ProbeFailed,
        },
    }
}

fn go_exists(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.file_type().is_dir())
}

fn parse_verified_version(output: &[u8]) -> Option<String> {
    let version = observed_version(output)?;
    VERIFIED_VERSIONS
        .contains(&version.as_str())
        .then_some(version)
}

fn observed_version(output: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(output).ok()?.trim();
    let rest = text.strip_prefix("go version ")?;
    rest.split_whitespace().next().map(ToOwned::to_owned)
}

const fn gap_from_degraded(reason: AdapterDegradedReason) -> InventoryGapReason {
    match reason {
        AdapterDegradedReason::UnknownVersion => InventoryGapReason::UnknownVersion,
        AdapterDegradedReason::Disabled => InventoryGapReason::Disabled,
        AdapterDegradedReason::InvalidSelection => InventoryGapReason::InvalidToolOutput,
        AdapterDegradedReason::ProbeFailed | AdapterDegradedReason::NotReady => {
            InventoryGapReason::ProbeFailed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{observed_version, parse_goenv, parse_verified_version};
    use std::path::PathBuf;

    #[test]
    fn version_line_accepts_only_the_verified_token() {
        assert_eq!(
            parse_verified_version(b"go version go1.26.6 darwin/arm64\n"),
            Some("go1.26.6".to_owned())
        );
        assert_eq!(
            parse_verified_version(b"go version go1.25.0 darwin/arm64\n"),
            None
        );
        assert_eq!(
            observed_version(b"go version go1.25.0 darwin/amd64\n").as_deref(),
            Some("go1.25.0")
        );
    }

    #[test]
    fn goenv_overrides_only_absolute_cache_keys() {
        let parsed = parse_goenv(
            b"GOROOT=/opt/homebrew/opt/go/libexec\nGOCACHE=/tmp/gocache\nGOMODCACHE=/tmp/gomod\n",
        )
        .expect("valid goenv");
        assert_eq!(parsed.0, Some(PathBuf::from("/tmp/gocache")));
        assert_eq!(parsed.1, Some(PathBuf::from("/tmp/gomod")));
        assert!(parse_goenv(b"GOCACHE=relative\n").is_none());
    }
}
