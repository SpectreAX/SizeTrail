use std::collections::BTreeMap;
use std::io;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

use crate::adapters::store::{excluded, object_observations};
use crate::adapters::{
    AdapterDegradedReason, AdapterId, AdapterState, Inventory, InventoryGap, InventoryGapReason,
    InventoryIdentity, InventoryItem, InventoryStage, ToolchainAdapter,
};
use crate::fsx::{Root, RootError};
use crate::model::{
    Advice, AdviceImpact, CommandAdvice, Finding, FindingSubject, Measurement, MeasurementBasis,
    MeasurementCoverage, MeasurementCoverageStatus, MeasurementPlane, MeasurementQuantity,
    MeasurementScope, MeasurementScopeKind, MeasurementValue, RevealAdvice, finding_id,
    normalize_findings, normalized_report_path, rounded_bytes,
};
use crate::policy::{
    DOCKER_CONTEXT_INSPECT, DOCKER_SYSTEM_DF, DOCKER_VERSION, PolicyCtx, PolicyError, ProbeId,
};
use crate::rules::builtin_rules;

pub const CONTEXT_INSPECT: ProbeId = DOCKER_CONTEXT_INSPECT;
pub const VERSION: ProbeId = DOCKER_VERSION;
pub const SYSTEM_DF: ProbeId = DOCKER_SYSTEM_DF;

const VERIFIED_VERSIONS: &[(&str, &str, &str, &str, &str)] = &[(
    "29.7.2",
    "1.55",
    "29.7.2",
    "1.55",
    "Docker Desktop 4.88.1 (237512)",
)];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DockerUsageRow {
    pub object_set_id: &'static str,
    pub total_count: u64,
    pub active_count: u64,
    pub size: String,
    pub reclaimable: String,
}

pub struct DockerAdapter<'a> {
    home_root: &'a Root,
    excludes: &'a [PathBuf],
}

impl<'a> DockerAdapter<'a> {
    #[must_use]
    pub const fn new(home_root: &'a Root, excludes: &'a [PathBuf]) -> Self {
        Self {
            home_root,
            excludes,
        }
    }

    #[must_use]
    pub fn discover_data_folder(home_root: &'a Root) -> Option<PathBuf> {
        Self::new(home_root, &[])
            .configured_data_folder()
            .ok()
            .flatten()
    }
}

impl ToolchainAdapter for DockerAdapter<'_> {
    fn id(&self) -> AdapterId {
        AdapterId::new("docker")
    }

    fn probe(&self, ctx: &mut PolicyCtx<'_>) -> AdapterState {
        probe(ctx, self.home_root.path())
    }

    fn inventory(&self, ctx: &mut PolicyCtx<'_>, state: &AdapterState) -> Inventory {
        let mut inventory = self.disk_inventory(state);
        if matches!(state, AdapterState::Ready { .. }) {
            self.append_daemon_inventory(ctx, state, &mut inventory);
        }
        inventory
    }

    fn classify(&self, inventory: &Inventory) -> Result<Vec<Finding>, InventoryGapReason> {
        let rules = builtin_rules().map_err(|_| InventoryGapReason::RuleSetInvalid)?;
        let rules = rules
            .into_iter()
            .filter(|rule| rule.adapter == "docker")
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
                id: finding_id("docker", &item.rule_id, &subject_key)
                    .map_err(|_| InventoryGapReason::RuleSetInvalid)?,
                adapter_id: "docker".to_owned(),
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
            "docker.images" => vec![Advice::Command(CommandAdvice {
                display_command: "docker --context desktop-linux image prune".to_owned(),
                impact: AdviceImpact::Destructive,
                explanation: "This vendor command removes dangling unused images. The vendor does not provide a reliable preview.".to_owned(),
                reliable_preview_available: false,
            })],
            "docker.build_cache" => vec![Advice::Command(CommandAdvice {
                display_command: "docker --context desktop-linux builder prune".to_owned(),
                impact: AdviceImpact::Destructive,
                explanation: "This vendor command removes unused BuildKit cache that a later build can reconstruct. The vendor does not provide a reliable preview.".to_owned(),
                reliable_preview_available: false,
            })],
            "docker.containers" => vec![Advice::Command(CommandAdvice {
                display_command: "docker --context desktop-linux ps -a".to_owned(),
                impact: AdviceImpact::Inspect,
                explanation: "Inspect the daemon container list. SizeTrail does not suggest a prune or file-delete command because containers hold writable user state.".to_owned(),
                reliable_preview_available: true,
            })],
            "docker.volumes" => vec![Advice::Command(CommandAdvice {
                display_command: "docker --context desktop-linux system prune --volumes".to_owned(),
                impact: AdviceImpact::Destructive,
                explanation: "This vendor command deletes stopped containers, unused networks, unused images, unused build cache, and unused anonymous volumes. It is not a recommended one-click next step. The vendor does not provide a reliable preview.".to_owned(),
                reliable_preview_available: false,
            })],
            _ => finding
                .subject
                .filesystem_path()
                .map_or_else(Vec::new, |path| {
                    vec![Advice::Reveal(RevealAdvice {
                        normalized_path: path.to_owned(),
                        recovery_semantics: "This host VM disk is not a safe deletion target. Deleting it destroys images, containers, and volumes together. SizeTrail only reveals the path.".to_owned(),
                    })]
                }),
        }
    }
}

impl DockerAdapter<'_> {
    fn disk_inventory(&self, state: &AdapterState) -> Inventory {
        let mut inventory = Inventory::default();
        let configured = match self.configured_data_folder() {
            Ok(configured) => configured,
            Err(gap) => {
                inventory.gaps.push(gap);
                return inventory;
            }
        };

        if let Some(folder) = configured {
            if excluded(&folder, self.excludes) {
                return inventory;
            }
            match Root::open(&folder) {
                Ok(root) => self.measure_candidates(&root, root.path(), state, &mut inventory),
                Err(error) => inventory.gaps.push(root_gap(folder, error)),
            }
            return inventory;
        }

        let data = self
            .home_root
            .path()
            .join("Library/Containers/com.docker.docker/Data/vms/0/data");
        match self.home_root.path_exists_without_descending(&data) {
            Ok(true) => {
                let before = (inventory.items.len(), inventory.gaps.len());
                self.measure_candidates(self.home_root, &data, state, &mut inventory);
                if before == (inventory.items.len(), inventory.gaps.len()) {
                    self.measure_legacy_driver_image(&mut inventory);
                }
            }
            Ok(false) => self.measure_legacy_driver_image(&mut inventory),
            Err(error) => {
                inventory
                    .gaps
                    .push(io_gap(data, InventoryStage::DockerDiskImage, &error))
            }
        }
        inventory
    }

    fn append_daemon_inventory(
        &self,
        ctx: &mut PolicyCtx<'_>,
        state: &AdapterState,
        inventory: &mut Inventory,
    ) {
        match system_df(ctx, state) {
            Ok(rows) => {
                let mut items = Vec::with_capacity(rows.len());
                for row in &rows {
                    match daemon_item(row) {
                        Ok(item) => items.push(item),
                        Err(reason) => {
                            inventory.gaps.push(InventoryGap {
                                region: "docker.daemon_inventory",
                                path: None,
                                reason,
                                stage: Some(InventoryStage::DockerSystemDf),
                                errno: None,
                            });
                            return;
                        }
                    }
                }
                inventory.items.extend(items);
                inventory.gaps.push(InventoryGap {
                    region: "docker.daemon_inventory",
                    path: None,
                    reason: InventoryGapReason::DaemonInventoryExcludesInactiveStore,
                    stage: Some(InventoryStage::DockerSystemDf),
                    errno: None,
                });
            }
            Err(reason) => inventory.gaps.push(InventoryGap {
                region: "docker.daemon_inventory",
                path: None,
                reason,
                stage: Some(InventoryStage::DockerSystemDf),
                errno: None,
            }),
        }
    }

    fn configured_data_folder(&self) -> Result<Option<PathBuf>, InventoryGap> {
        let settings = self
            .home_root
            .path()
            .join("Library/Group Containers/group.com.docker/settings-store.json");
        if let Some(contents) = self.read_setting(&settings)? {
            return parse_data_folder(&contents, "DataFolder")
                .map(Some)
                .ok_or(InventoryGap {
                    region: "docker.virtual_disk",
                    path: Some(settings),
                    reason: InventoryGapReason::InvalidToolOutput,
                    stage: Some(InventoryStage::DockerSettings),
                    errno: None,
                });
        }

        let legacy = self
            .home_root
            .path()
            .join("Library/Group Containers/group.com.docker/settings.json");
        if let Some(contents) = self.read_setting(&legacy)? {
            return parse_data_folder(&contents, "dataFolder")
                .map(Some)
                .ok_or(InventoryGap {
                    region: "docker.virtual_disk",
                    path: Some(legacy),
                    reason: InventoryGapReason::InvalidToolOutput,
                    stage: Some(InventoryStage::DockerSettings),
                    errno: None,
                });
        }
        Ok(None)
    }

    fn read_setting(&self, path: &Path) -> Result<Option<Vec<u8>>, InventoryGap> {
        if excluded(path, self.excludes) {
            return Ok(None);
        }
        match self.home_root.path_exists_without_descending(path) {
            Ok(false) => return Ok(None),
            Ok(true) => {}
            Err(error) => {
                return Err(io_gap(
                    path.to_path_buf(),
                    InventoryStage::DockerSettings,
                    &error,
                ));
            }
        }
        let measured = self
            .home_root
            .measure_object(path)
            .map_err(|error| io_gap(path.to_path_buf(), InventoryStage::DockerSettings, &error))?;
        if measured.dataless
            || !std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
        {
            return Err(InventoryGap {
                region: "docker.virtual_disk",
                path: Some(path.to_path_buf()),
                reason: InventoryGapReason::PolicyDeniedUnknown,
                stage: Some(InventoryStage::DockerSettings),
                errno: None,
            });
        }
        std::fs::read(path)
            .map(Some)
            .map_err(|error| io_gap(path.to_path_buf(), InventoryStage::DockerSettings, &error))
    }

    fn measure_candidates(
        &self,
        root: &Root,
        data_folder: &Path,
        state: &AdapterState,
        inventory: &mut Inventory,
    ) {
        let raw = data_folder.join("Docker.raw");
        let qcow2 = data_folder.join("Docker.qcow2");
        let candidates = [raw, qcow2]
            .into_iter()
            .filter(|path| !excluded(path, self.excludes))
            .filter_map(|path| match root.path_exists_without_descending(&path) {
                Ok(true) => Some(Ok(path)),
                Ok(false) => None,
                Err(error) => Some(Err(io_gap(path, InventoryStage::DockerDiskImage, &error))),
            })
            .collect::<Result<Vec<_>, _>>();
        let candidates = match candidates {
            Ok(candidates) => candidates,
            Err(gap) => {
                inventory.gaps.push(gap);
                return;
            }
        };
        let selected = match candidates.as_slice() {
            [] => return,
            [only] => only,
            [raw, _] if matches!(state, AdapterState::Ready { .. }) => raw,
            _ => {
                inventory.gaps.push(InventoryGap {
                    region: "docker.virtual_disk",
                    path: Some(data_folder.to_path_buf()),
                    reason: InventoryGapReason::AmbiguousDiskImage,
                    stage: Some(InventoryStage::DockerDiskImage),
                    errno: None,
                });
                return;
            }
        };
        match self.measure_disk(root, selected) {
            Ok(item) => inventory.items.push(item),
            Err(gap) => inventory.gaps.push(gap),
        }
    }

    fn measure_legacy_driver_image(&self, inventory: &mut Inventory) {
        let path = self.home_root.path().join(
            "Library/Containers/com.docker.docker/Data/com.docker.driver.amd64-linux/Docker.qcow2",
        );
        if excluded(&path, self.excludes) {
            return;
        }
        match self.home_root.path_exists_without_descending(&path) {
            Ok(true) => match self.measure_disk(self.home_root, &path) {
                Ok(item) => inventory.items.push(item),
                Err(gap) => inventory.gaps.push(gap),
            },
            Ok(false) => {}
            Err(error) => {
                inventory
                    .gaps
                    .push(io_gap(path, InventoryStage::DockerDiskImage, &error))
            }
        }
    }

    fn measure_disk(&self, root: &Root, path: &Path) -> Result<InventoryItem, InventoryGap> {
        let measured = root
            .measure_object(path)
            .map_err(|error| io_gap(path.to_path_buf(), InventoryStage::DockerDiskImage, &error))?;
        if measured.dataless
            || !std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
        {
            return Err(InventoryGap {
                region: "docker.virtual_disk",
                path: Some(path.to_path_buf()),
                reason: InventoryGapReason::PolicyDeniedUnknown,
                stage: Some(InventoryStage::DockerDiskImage),
                errno: None,
            });
        }
        let normalized_path =
            normalized_report_path(self.home_root.path(), path).map_err(|_| InventoryGap {
                region: "docker.virtual_disk",
                path: Some(path.to_path_buf()),
                reason: InventoryGapReason::TraversalFailed,
                stage: Some(InventoryStage::DockerDiskImage),
                errno: None,
            })?;
        let scope = MeasurementScope {
            kind: MeasurementScopeKind::ToolchainStore,
            id: normalized_path.clone(),
        };
        let complete = MeasurementCoverage {
            status: MeasurementCoverageStatus::Complete,
            gap_ids: Vec::new(),
        };
        let allocated = measured.allocated_bytes;
        Ok(InventoryItem {
            rule_id: "docker.virtual_disk".to_owned(),
            subject: FindingSubject::FilesystemPath { normalized_path },
            path: Some(path.to_path_buf()),
            measurements: vec![
                Measurement {
                    plane: MeasurementPlane::ToolchainAttribution,
                    quantity: MeasurementQuantity::DiskImageLogicalLimit,
                    basis: MeasurementBasis::LogicalSize,
                    scope: scope.clone(),
                    coverage: complete,
                    value: MeasurementValue::ExactBytes {
                        bytes: measured.logical_bytes,
                    },
                },
                Measurement {
                    plane: MeasurementPlane::ToolchainAttribution,
                    quantity: MeasurementQuantity::HostAllocatedFootprint,
                    basis: MeasurementBasis::AllocatedFootprint,
                    scope,
                    coverage: MeasurementCoverage {
                        status: if allocated.is_some() {
                            MeasurementCoverageStatus::Complete
                        } else {
                            MeasurementCoverageStatus::Unmeasurable
                        },
                        gap_ids: if allocated.is_some() {
                            Vec::new()
                        } else {
                            vec!["allocated_size_unmeasurable".to_owned()]
                        },
                    },
                    value: allocated.map_or(MeasurementValue::Unmeasurable, |bytes| {
                        MeasurementValue::ExactBytes { bytes }
                    }),
                },
            ],
            observations: object_observations(&measured),
            identity: InventoryIdentity::Path,
        })
    }
}

fn daemon_item(row: &DockerUsageRow) -> Result<InventoryItem, InventoryGapReason> {
    let used = rounded_bytes(&row.size).map_err(|_| InventoryGapReason::InvalidToolOutput)?;
    let reclaimable =
        rounded_bytes(&row.reclaimable).map_err(|_| InventoryGapReason::InvalidToolOutput)?;
    let scope = MeasurementScope {
        kind: MeasurementScopeKind::ObjectSet,
        id: row.object_set_id.to_owned(),
    };
    let complete = MeasurementCoverage {
        status: MeasurementCoverageStatus::Complete,
        gap_ids: Vec::new(),
    };
    Ok(InventoryItem {
        rule_id: row.object_set_id.to_owned(),
        subject: FindingSubject::ToolchainObjectSet {
            object_set_id: row.object_set_id.to_owned(),
        },
        path: None,
        measurements: vec![
            daemon_count(
                MeasurementQuantity::ObjectCount,
                row.total_count,
                scope.clone(),
                complete.clone(),
            ),
            daemon_count(
                MeasurementQuantity::ActiveObjectCount,
                row.active_count,
                scope.clone(),
                complete.clone(),
            ),
            daemon_rounded(
                MeasurementQuantity::DaemonUsed,
                used,
                scope.clone(),
                complete.clone(),
            ),
            daemon_rounded(
                MeasurementQuantity::DaemonReclaimable,
                reclaimable,
                scope,
                complete,
            ),
        ],
        observations: Vec::new(),
        identity: InventoryIdentity::Path,
    })
}

fn daemon_count(
    quantity: MeasurementQuantity,
    count: u64,
    scope: MeasurementScope,
    coverage: MeasurementCoverage,
) -> Measurement {
    Measurement {
        plane: MeasurementPlane::ToolchainAttribution,
        quantity,
        basis: MeasurementBasis::DockerSystemDf,
        scope,
        coverage,
        value: MeasurementValue::ExactCount { count },
    }
}

fn daemon_rounded(
    quantity: MeasurementQuantity,
    value: MeasurementValue,
    scope: MeasurementScope,
    coverage: MeasurementCoverage,
) -> Measurement {
    Measurement {
        plane: MeasurementPlane::ToolchainAttribution,
        quantity,
        basis: MeasurementBasis::DockerSystemDf,
        scope,
        coverage,
        value,
    }
}

fn parse_data_folder(contents: &[u8], key: &str) -> Option<PathBuf> {
    let document = serde_json::from_slice::<serde_json::Value>(contents).ok()?;
    let path = PathBuf::from(document.get(key)?.as_str()?);
    (path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir)))
    .then_some(path)
}

fn root_gap(path: PathBuf, error: RootError) -> InventoryGap {
    InventoryGap {
        region: "docker.virtual_disk",
        path: Some(path),
        reason: match error {
            RootError::CloudRootExcluded | RootError::ReadPolicyVerificationFailed => {
                InventoryGapReason::PolicyDeniedUnknown
            }
            RootError::PathUnresolvable => InventoryGapReason::AbsentOrChanged,
            RootError::NotNormalizedAbsolute
            | RootError::PathNotEncodable
            | RootError::IdentityUnavailable
            | RootError::SymlinkTraversalRejected => InventoryGapReason::TraversalFailed,
        },
        stage: Some(InventoryStage::RootInitialization),
        errno: None,
    }
}

fn io_gap(path: PathBuf, stage: InventoryStage, error: &io::Error) -> InventoryGap {
    let errno = error.raw_os_error();
    InventoryGap {
        region: "docker.virtual_disk",
        path: Some(path),
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

pub fn probe(ctx: &mut PolicyCtx<'_>, home: &Path) -> AdapterState {
    let context = match ctx.run(CONTEXT_INSPECT) {
        Ok(output) if output.success => output.stdout,
        Ok(_) => {
            return AdapterState::Degraded {
                observed_version: None,
                reason: AdapterDegradedReason::InvalidSelection,
            };
        }
        Err(PolicyError::InvocationFailed(_)) => return AdapterState::NotPresent,
        Err(error) => return degraded_from_policy(error, None),
    };
    if !verified_context(&context, home) {
        return AdapterState::Degraded {
            observed_version: None,
            reason: AdapterDegradedReason::InvalidSelection,
        };
    }

    let output = match ctx.run(VERSION) {
        Ok(output) if output.success => output.stdout,
        Ok(output) => {
            return AdapterState::Degraded {
                observed_version: observed_version(&output.stdout),
                reason: AdapterDegradedReason::ProbeFailed,
            };
        }
        Err(error) => return degraded_from_policy(error, None),
    };
    let Some(version) = parse_verified_version(&output) else {
        return AdapterState::Degraded {
            observed_version: observed_version(&output),
            reason: AdapterDegradedReason::UnknownVersion,
        };
    };
    AdapterState::Ready { version }
}

pub fn system_df(
    ctx: &mut PolicyCtx<'_>,
    state: &AdapterState,
) -> Result<Vec<DockerUsageRow>, InventoryGapReason> {
    match state {
        AdapterState::Ready { .. } => {}
        AdapterState::NotPresent => return Err(InventoryGapReason::AbsentOrChanged),
        AdapterState::Degraded { reason, .. } => return Err(gap_from_degraded(*reason)),
    }
    let output = ctx.run(SYSTEM_DF).map_err(gap_from_policy)?;
    if !output.success {
        return Err(InventoryGapReason::ProbeFailed);
    }
    parse_system_df(&output.stdout)
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContextRecord {
    name: String,
    endpoints: ContextEndpoints,
}

#[derive(Deserialize)]
struct ContextEndpoints {
    docker: ContextDockerEndpoint,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContextDockerEndpoint {
    host: String,
    #[serde(rename = "SkipTLSVerify")]
    skip_tls_verify: bool,
}

fn verified_context(output: &[u8], home: &Path) -> bool {
    let Ok(contexts) = serde_json::from_slice::<Vec<ContextRecord>>(output) else {
        return false;
    };
    let Ok([context]) = <Vec<ContextRecord> as TryInto<[ContextRecord; 1]>>::try_into(contexts)
    else {
        return false;
    };
    context.name == "desktop-linux"
        && !context.endpoints.docker.skip_tls_verify
        && context.endpoints.docker.host
            == format!("unix://{}", home.join(".docker/run/docker.sock").display())
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VersionDocument {
    client: VersionPeer,
    server: VersionPeer,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VersionPeer {
    version: String,
    api_version: String,
    #[serde(default)]
    platform: Option<VersionPlatform>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VersionPlatform {
    name: String,
}

fn parse_verified_version(output: &[u8]) -> Option<String> {
    let version = serde_json::from_slice::<VersionDocument>(output).ok()?;
    let platform = version.server.platform?.name;
    VERIFIED_VERSIONS
        .contains(&(
            version.client.version.as_str(),
            version.client.api_version.as_str(),
            version.server.version.as_str(),
            version.server.api_version.as_str(),
            platform.as_str(),
        ))
        .then(|| {
            format!(
                "client {} api {}; server {} api {}; {platform}",
                version.client.version,
                version.client.api_version,
                version.server.version,
                version.server.api_version
            )
        })
}

fn observed_version(output: &[u8]) -> Option<String> {
    let version = serde_json::from_slice::<VersionDocument>(output).ok()?;
    Some(format!(
        "client {} api {}; server {} api {}; {}",
        version.client.version,
        version.client.api_version,
        version.server.version,
        version.server.api_version,
        version.server.platform?.name
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawUsageRow {
    r#type: String,
    total_count: String,
    active: String,
    size: String,
    reclaimable: String,
}

fn parse_system_df(output: &[u8]) -> Result<Vec<DockerUsageRow>, InventoryGapReason> {
    let text = std::str::from_utf8(output).map_err(|_| InventoryGapReason::InvalidToolOutput)?;
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        let raw = serde_json::from_str::<RawUsageRow>(line)
            .map_err(|_| InventoryGapReason::InvalidToolOutput)?;
        let total_count = raw
            .total_count
            .parse::<u64>()
            .map_err(|_| InventoryGapReason::InvalidToolOutput)?;
        let active_count = raw
            .active
            .parse::<u64>()
            .map_err(|_| InventoryGapReason::InvalidToolOutput)?;
        rounded_bytes(&raw.size).map_err(|_| InventoryGapReason::InvalidToolOutput)?;
        rounded_bytes(&raw.reclaimable).map_err(|_| InventoryGapReason::InvalidToolOutput)?;
        let object_set_id = match raw.r#type.as_str() {
            "Images" => "docker.images",
            "Containers" => "docker.containers",
            "Local Volumes" => "docker.volumes",
            "Build Cache" => "docker.build_cache",
            _ => return Err(InventoryGapReason::InvalidToolOutput),
        };
        if rows
            .insert(
                object_set_id,
                DockerUsageRow {
                    object_set_id,
                    total_count,
                    active_count,
                    size: raw.size,
                    reclaimable: raw.reclaimable,
                },
            )
            .is_some()
        {
            return Err(InventoryGapReason::InvalidToolOutput);
        }
    }
    [
        "docker.images",
        "docker.containers",
        "docker.volumes",
        "docker.build_cache",
    ]
    .into_iter()
    .map(|id| rows.remove(id).ok_or(InventoryGapReason::InvalidToolOutput))
    .collect::<Result<Vec<_>, _>>()
    .and_then(|ordered| {
        if rows.is_empty() {
            Ok(ordered)
        } else {
            Err(InventoryGapReason::InvalidToolOutput)
        }
    })
}

fn degraded_from_policy(error: PolicyError, observed_version: Option<String>) -> AdapterState {
    let reason = match error {
        PolicyError::Disabled(_) => AdapterDegradedReason::Disabled,
        PolicyError::UndeclaredProbe(_)
        | PolicyError::CallLimitExceeded(_)
        | PolicyError::InvocationFailed(_)
        | PolicyError::TimedOut(_) => AdapterDegradedReason::ProbeFailed,
    };
    AdapterState::Degraded {
        observed_version,
        reason,
    }
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

fn gap_from_policy(error: PolicyError) -> InventoryGapReason {
    match error {
        PolicyError::Disabled(_) => InventoryGapReason::Disabled,
        PolicyError::TimedOut(_) => InventoryGapReason::TimedOut,
        PolicyError::UndeclaredProbe(_)
        | PolicyError::CallLimitExceeded(_)
        | PolicyError::InvocationFailed(_) => InventoryGapReason::ProbeFailed,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        CONTEXT_INSPECT, SYSTEM_DF, VERSION, parse_system_df, probe, system_df, verified_context,
    };
    use crate::adapters::{AdapterDegradedReason, AdapterState, InventoryGapReason};
    use crate::policy::{PolicyCtx, ProbePolicy, ReadOnlyCommand};

    const CONTEXT: &str = include_str!("../../tests/fixtures/docker/context-desktop-linux.json");
    const VERSION_JSON: &str = include_str!("../../tests/fixtures/docker/version-verified.json");
    const SYSTEM_DF_JSON: &str = include_str!("../../tests/fixtures/docker/system-df.ndjson");

    const CONTEXT_ARGS: &[&str] = &["%s", CONTEXT];
    const VERSION_ARGS: &[&str] = &["%s", VERSION_JSON];
    const SYSTEM_DF_ARGS: &[&str] = &["%s", SYSTEM_DF_JSON];

    fn policies(
        context: &'static [&'static str],
        version: &'static [&'static str],
        df: &'static [&'static str],
    ) -> [ProbePolicy; 3] {
        [
            printf_policy(CONTEXT_INSPECT, context),
            printf_policy(VERSION, version),
            printf_policy(SYSTEM_DF, df),
        ]
    }

    const fn printf_policy(
        id: crate::policy::ProbeId,
        arguments: &'static [&'static str],
    ) -> ProbePolicy {
        ProbePolicy {
            id,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_DOCKER_PROBE_FIXTURE",
            known_side_effects: &[],
            command: ReadOnlyCommand {
                program: "/usr/bin/printf",
                arguments,
                environment: &[],
                remove_environment: &[],
                timeout_millis: 10_000,
            },
        }
    }

    #[test]
    fn verified_local_context_and_version_allow_one_summary_call() {
        let policies = policies(CONTEXT_ARGS, VERSION_ARGS, SYSTEM_DF_ARGS);
        let mut ctx = PolicyCtx::for_test(&policies);
        let state = probe(&mut ctx, Path::new("/Users/fixture"));

        assert!(matches!(state, AdapterState::Ready { .. }));
        let rows = system_df(&mut ctx, &state).expect("verified summary must parse");
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].object_set_id, "docker.images");
        assert_eq!(rows[3].object_set_id, "docker.build_cache");
        assert_eq!(ctx.count(CONTEXT_INSPECT), 1);
        assert_eq!(ctx.count(VERSION), 1);
        assert_eq!(ctx.count(SYSTEM_DF), 1);
    }

    #[test]
    fn remote_context_stops_before_any_daemon_connection() {
        const REMOTE: &str =
            r#"[{"Name":"desktop-linux","Endpoints":{"docker":{"Host":"ssh://builder.example"}}}]"#;
        const REMOTE_ARGS: &[&str] = &["%s", REMOTE];
        let policies = policies(REMOTE_ARGS, VERSION_ARGS, SYSTEM_DF_ARGS);
        let mut ctx = PolicyCtx::for_test(&policies);
        let state = probe(&mut ctx, Path::new("/Users/fixture"));

        assert_eq!(
            state,
            AdapterState::Degraded {
                observed_version: None,
                reason: AdapterDegradedReason::InvalidSelection,
            }
        );
        assert!(system_df(&mut ctx, &state).is_err());
        assert_eq!(ctx.count(CONTEXT_INSPECT), 1);
        assert_eq!(ctx.count(VERSION), 0);
        assert_eq!(ctx.count(SYSTEM_DF), 0);
    }

    #[test]
    fn context_identity_rejects_every_nonlocal_or_tls_variant() {
        for (host, skip_tls_verify) in [
            ("tcp://127.0.0.1:2375", false),
            ("ssh://builder.example", false),
            ("unix:///var/run/docker.sock", false),
            ("unix:///Users/other/.docker/run/docker.sock", false),
            ("unix:///Users/fixture/.docker/run/docker.sock", true),
        ] {
            let context = serde_json::to_vec(&serde_json::json!([{
                "Name": "desktop-linux",
                "Endpoints": {"docker": {"Host": host, "SkipTLSVerify": skip_tls_verify}}
            }]))
            .expect("context fixture must serialize");
            assert!(!verified_context(&context, Path::new("/Users/fixture")));
        }
    }

    #[test]
    fn unknown_version_never_runs_system_df() {
        const UNKNOWN: &str = r#"{"Client":{"Version":"99.0.0","ApiVersion":"9.99"},"Server":{"Platform":{"Name":"Docker Desktop 99.0.0"},"Version":"99.0.0","ApiVersion":"9.99"}}"#;
        const UNKNOWN_ARGS: &[&str] = &["%s", UNKNOWN];
        let policies = policies(CONTEXT_ARGS, UNKNOWN_ARGS, SYSTEM_DF_ARGS);
        let mut ctx = PolicyCtx::for_test(&policies);
        let state = probe(&mut ctx, Path::new("/Users/fixture"));

        assert!(matches!(
            state,
            AdapterState::Degraded {
                reason: AdapterDegradedReason::UnknownVersion,
                ..
            }
        ));
        assert!(system_df(&mut ctx, &state).is_err());
        assert_eq!(ctx.count(CONTEXT_INSPECT), 1);
        assert_eq!(ctx.count(VERSION), 1);
        assert_eq!(ctx.count(SYSTEM_DF), 0);
    }

    #[test]
    fn malformed_or_nonzero_summary_is_a_typed_gap_without_retry() {
        const MALFORMED_ARGS: &[&str] = &["%s", "not-json\n"];
        let malformed = policies(CONTEXT_ARGS, VERSION_ARGS, MALFORMED_ARGS);
        let mut malformed_ctx = PolicyCtx::for_test(&malformed);
        let state = probe(&mut malformed_ctx, Path::new("/Users/fixture"));
        assert!(system_df(&mut malformed_ctx, &state).is_err());
        assert_eq!(malformed_ctx.count(SYSTEM_DF), 1);

        let mut nonzero = policies(CONTEXT_ARGS, VERSION_ARGS, SYSTEM_DF_ARGS);
        nonzero[2].command.program = "/usr/bin/false";
        nonzero[2].command.arguments = &[];
        let mut nonzero_ctx = PolicyCtx::for_test(&nonzero);
        let state = probe(&mut nonzero_ctx, Path::new("/Users/fixture"));
        assert!(system_df(&mut nonzero_ctx, &state).is_err());
        assert_eq!(nonzero_ctx.count(SYSTEM_DF), 1);
    }

    #[test]
    fn disabled_or_timed_out_probes_are_typed_and_never_retried() {
        let mut disabled = policies(CONTEXT_ARGS, VERSION_ARGS, SYSTEM_DF_ARGS);
        disabled[0].disable_env = "PATH";
        let mut disabled_ctx = PolicyCtx::for_test(&disabled);
        assert_eq!(
            probe(&mut disabled_ctx, Path::new("/Users/fixture")),
            AdapterState::Degraded {
                observed_version: None,
                reason: AdapterDegradedReason::Disabled,
            }
        );
        assert_eq!(disabled_ctx.count(CONTEXT_INSPECT), 0);
        assert_eq!(disabled_ctx.count(VERSION), 0);
        assert_eq!(disabled_ctx.count(SYSTEM_DF), 0);

        let mut timed_out = policies(CONTEXT_ARGS, VERSION_ARGS, SYSTEM_DF_ARGS);
        timed_out[2].command.program = "/bin/sleep";
        timed_out[2].command.arguments = &["1"];
        timed_out[2].command.timeout_millis = 1;
        let mut timed_out_ctx = PolicyCtx::for_test(&timed_out);
        let state = probe(&mut timed_out_ctx, Path::new("/Users/fixture"));
        assert_eq!(
            system_df(&mut timed_out_ctx, &state),
            Err(InventoryGapReason::TimedOut)
        );
        assert_eq!(timed_out_ctx.count(SYSTEM_DF), 1);
    }

    #[test]
    fn summary_requires_the_exact_four_typed_rows_and_valid_vendor_numbers() {
        let missing = SYSTEM_DF_JSON
            .lines()
            .take(3)
            .collect::<Vec<_>>()
            .join("\n");
        let duplicate = format!(
            "{SYSTEM_DF_JSON}{}\n",
            SYSTEM_DF_JSON.lines().next().expect("images fixture row")
        );
        let unknown = SYSTEM_DF_JSON.replace("Build Cache", "Networks");
        let negative = SYSTEM_DF_JSON.replacen("\"11\"", "\"-1\"", 1);
        let overflow = SYSTEM_DF_JSON.replacen("\"11\"", "\"18446744073709551616\"", 1);
        let extra = format!("{SYSTEM_DF_JSON}docker diagnostic text\n");

        for invalid in [missing, duplicate, unknown, negative, overflow, extra] {
            assert_eq!(
                parse_system_df(invalid.as_bytes()),
                Err(InventoryGapReason::InvalidToolOutput)
            );
        }
        assert_eq!(
            parse_system_df(&[0xff]),
            Err(InventoryGapReason::InvalidToolOutput)
        );
    }
}
