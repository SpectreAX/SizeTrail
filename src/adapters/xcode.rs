use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::adapters::{
    AdapterDegradedReason, AdapterId, AdapterState, Inventory, InventoryGap, InventoryGapReason,
    InventoryIdentity, InventoryItem, InventoryStage, ToolchainAdapter,
};
use crate::fsx::{ObjectMeasurements, Root, RootEntryKind};
use crate::model::{
    Advice, AdviceImpact, CommandAdvice, DispositionAction, ExtentKind, ExtentObservation, Finding,
    Measurement, MeasurementBasis, MeasurementCoverage, MeasurementCoverageStatus,
    MeasurementPlane, MeasurementScope, MeasurementScopeKind, MeasurementValue, ObservationKind,
    ObservationRelation, ObservationScope, RevealAdvice, SignalId, SignalObservation,
    StorageSignal, estimate_disposition, finding_id, normalize_findings, normalized_report_path,
};
use crate::policy::{
    PolicyCtx, PolicyError, ProbeId, XCODE_CORE_SIMULATOR_VERSION, XCODE_FIRST_LAUNCH_STATUS,
    XCODE_SELECT_DEVELOPER_DIR, XCODE_SIMCTL_DEVICES, XCODE_SIMCTL_RUNTIMES,
    XCODE_XCODEBUILD_VERSION,
};
use crate::rules::{Rule, builtin_rules};

pub const SELECT_DEVELOPER_DIR: ProbeId = XCODE_SELECT_DEVELOPER_DIR;
pub const XCODEBUILD_VERSION: ProbeId = XCODE_XCODEBUILD_VERSION;
pub const FIRST_LAUNCH_STATUS: ProbeId = XCODE_FIRST_LAUNCH_STATUS;
pub const CORE_SIMULATOR_VERSION: ProbeId = XCODE_CORE_SIMULATOR_VERSION;
pub const SIMCTL_DEVICES: ProbeId = XCODE_SIMCTL_DEVICES;
pub const SIMCTL_RUNTIMES: ProbeId = XCODE_SIMCTL_RUNTIMES;

pub struct XcodeAdapter<'a> {
    root: &'a Root,
    excludes: &'a [PathBuf],
    volume_has_snapshots: Result<bool, Option<i32>>,
}

impl<'a> XcodeAdapter<'a> {
    #[must_use]
    pub const fn new(
        root: &'a Root,
        excludes: &'a [PathBuf],
        volume_has_snapshots: Result<bool, Option<i32>>,
    ) -> Self {
        Self {
            root,
            excludes,
            volume_has_snapshots,
        }
    }
}

impl ToolchainAdapter for XcodeAdapter<'_> {
    fn id(&self) -> AdapterId {
        AdapterId::new("xcode")
    }

    fn probe(&self, ctx: &mut PolicyCtx<'_>) -> AdapterState {
        probe(ctx)
    }

    fn inventory(&self, ctx: &mut PolicyCtx<'_>, state: &AdapterState) -> Inventory {
        let mut inventory = Inventory::default();
        self.visit_inventory_stages(ctx, state, |mut stage| {
            inventory.items.append(&mut stage.items);
            inventory.gaps.append(&mut stage.gaps);
            inventory.warnings.append(&mut stage.warnings);
        });
        inventory.items.sort_by(|left, right| {
            left.rule_id
                .cmp(&right.rule_id)
                .then_with(|| left.normalized_path.cmp(&right.normalized_path))
        });
        inventory
    }

    fn classify(&self, inventory: &Inventory) -> Result<Vec<Finding>, InventoryGapReason> {
        let rules = builtin_rules().map_err(|_| InventoryGapReason::RuleSetInvalid)?;
        let rules = rules
            .into_iter()
            .map(|rule| (rule.id.clone(), rule))
            .collect::<BTreeMap<_, _>>();
        let mut findings = Vec::with_capacity(inventory.items.len());
        for item in &inventory.items {
            let rule = rules
                .get(&item.rule_id)
                .ok_or(InventoryGapReason::RuleSetInvalid)?;
            let id = finding_id("xcode", &item.rule_id, &item.normalized_path)
                .map_err(|_| InventoryGapReason::RuleSetInvalid)?;
            let mut finding = Finding {
                id,
                adapter_id: "xcode".to_owned(),
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
        match finding.rule_id.as_str() {
            "xcode.simulator_device" => finding
                .normalized_path
                .rsplit('/')
                .next()
                .filter(|udid| valid_udid(udid))
                .map(|udid| {
                    vec![Advice::Command(CommandAdvice {
                        display_command: format!("xcrun simctl delete {udid}"),
                        impact: AdviceImpact::Destructive,
                        explanation: "Apple simctl removes this device and all of its application and test state; no reliable preview is available.".to_owned(),
                        reliable_preview_available: false,
                    })]
                })
                .unwrap_or_default(),
            "xcode.simulator_runtime" => vec![Advice::Command(CommandAdvice {
                display_command: "xcrun simctl runtime list".to_owned(),
                impact: AdviceImpact::Inspect,
                explanation: "Inspect Apple's runtime image manager, then use Xcode Settings > Components for removal; do not delete runtime mount paths directly.".to_owned(),
                reliable_preview_available: true,
            })],
            _ => vec![Advice::Reveal(RevealAdvice {
                normalized_path: finding.normalized_path.clone(),
                recovery_semantics: finding.evidence.clone(),
            })],
        }
    }
}

impl XcodeAdapter<'_> {
    pub(crate) fn visit_inventory_stages(
        &self,
        ctx: &mut PolicyCtx<'_>,
        state: &AdapterState,
        mut visit: impl FnMut(Inventory),
    ) {
        match state {
            AdapterState::NotPresent => return,
            AdapterState::Degraded { reason, .. } => {
                visit(Inventory {
                    gaps: vec![InventoryGap::diagnostic("xcode", degraded_gap(*reason))],
                    ..Inventory::default()
                });
                return;
            }
            AdapterState::Ready { .. } => {}
        }

        let static_inventory = match self.inventory_static() {
            Ok(inventory) => inventory,
            Err(reason) => {
                visit(Inventory {
                    gaps: vec![InventoryGap::diagnostic("xcode", reason)],
                    ..Inventory::default()
                });
                return;
            }
        };
        visit(static_inventory);

        if let Err(reason) = core_simulator_compatible(ctx, state) {
            let mut devices = Inventory {
                gaps: vec![InventoryGap {
                    region: "xcode.simulator_inventory",
                    path: None,
                    reason,
                    stage: Some(InventoryStage::ToolchainProbe),
                    errno: None,
                }],
                ..Inventory::default()
            };
            self.inventory_devices_static(&mut devices);
            visit(devices);
            return;
        }

        let mut devices = Inventory::default();
        self.inventory_devices(ctx, &mut devices);
        visit(devices);

        let mut runtimes = Inventory::default();
        self.inventory_runtimes(ctx, &mut runtimes);
        visit(runtimes);
    }

    fn inventory_static(&self) -> Result<Inventory, InventoryGapReason> {
        let rules = builtin_rules().map_err(|_| InventoryGapReason::RuleSetInvalid)?;
        let mut inventory = Inventory::default();
        let volume_has_snapshots = match self.volume_has_snapshots {
            Ok(value) => value,
            Err(errno) => {
                inventory.gaps.push(InventoryGap {
                    region: "xcode",
                    path: None,
                    reason: InventoryGapReason::VolumeSnapshotStateUnavailable,
                    stage: Some(InventoryStage::VolumeSnapshots),
                    errno,
                });
                false
            }
        };

        for rule in rules.iter().filter(|rule| {
            matches!(
                rule.id.as_str(),
                "xcode.derived_data_build" | "xcode.archives" | "xcode.device_support"
            )
        }) {
            self.expand_rule(rule, volume_has_snapshots, &mut inventory);
        }

        inventory.items.sort_by(|left, right| {
            left.rule_id
                .cmp(&right.rule_id)
                .then_with(|| left.normalized_path.cmp(&right.normalized_path))
        });
        Ok(inventory)
    }

    fn expand_rule(&self, rule: &Rule, volume_has_snapshots: bool, inventory: &mut Inventory) {
        let mut paths = BTreeSet::new();
        for pattern in &rule.paths {
            match expand_home_pattern(self.root, pattern, self.excludes) {
                Ok(matches) => paths.extend(matches),
                Err((path, error)) => inventory.gaps.push(io_gap(
                    "xcode",
                    &path,
                    InventoryStage::ListDirectory,
                    &error,
                )),
            }
        }
        for path in paths {
            match measure_store(self.root, &path, self.excludes, volume_has_snapshots) {
                Ok((measurements, observations)) => {
                    let Ok(normalized_path) = normalized_report_path(self.root.path(), &path)
                    else {
                        inventory.gaps.push(InventoryGap {
                            region: "xcode",
                            path: Some(path),
                            reason: InventoryGapReason::TraversalFailed,
                            stage: Some(InventoryStage::NormalizePath),
                            errno: None,
                        });
                        continue;
                    };
                    inventory.items.push(InventoryItem {
                        rule_id: rule.id.clone(),
                        normalized_path,
                        path: Some(path),
                        measurements,
                        observations,
                        identity: InventoryIdentity::Path,
                    });
                }
                Err(error) => inventory.gaps.push(io_gap(
                    "xcode",
                    &path,
                    InventoryStage::MeasureObject,
                    &error,
                )),
            }
        }
    }

    /// Q45: a device set's bytes are measurable without the version-pinned binary, which is only
    /// needed to enumerate devices and name them. Losing the whole category on a version miss
    /// discards measurable storage, so measure the paths and declare the missing identity.
    fn inventory_devices_static(&self, inventory: &mut Inventory) {
        let Ok(rules) = builtin_rules() else {
            inventory.gaps.push(InventoryGap::diagnostic(
                "xcode",
                InventoryGapReason::RuleSetInvalid,
            ));
            return;
        };
        let Some(rule) = rules
            .iter()
            .find(|rule| rule.id == "xcode.simulator_device")
        else {
            inventory.gaps.push(InventoryGap::diagnostic(
                "xcode",
                InventoryGapReason::RuleSetInvalid,
            ));
            return;
        };

        let measured_before = inventory.items.len();
        self.expand_rule(rule, self.volume_has_snapshots.unwrap_or(false), inventory);
        if inventory.items.len() > measured_before {
            inventory.gaps.push(InventoryGap {
                region: "xcode.simulator_devices",
                path: None,
                reason: InventoryGapReason::SimulatorIdentityUnavailable,
                stage: Some(InventoryStage::ToolchainProbe),
                errno: None,
            });
        }
    }

    fn inventory_devices(&self, ctx: &mut PolicyCtx<'_>, inventory: &mut Inventory) {
        let devices_root = self
            .root
            .path()
            .join("Library/Developer/CoreSimulator/Devices");
        if excluded(&devices_root, self.excludes) {
            return;
        }
        let output = match ctx.run(SIMCTL_DEVICES) {
            Ok(output) if output.success => output,
            Ok(_) => {
                inventory
                    .gaps
                    .push(probe_gap(InventoryGapReason::ProbeFailed));
                return;
            }
            Err(error) => {
                inventory.gaps.push(probe_gap(policy_gap(error)));
                return;
            }
        };
        record_warning(&output.stderr, inventory);
        let Ok(document) = serde_json::from_slice::<DevicesDocument>(&output.stdout) else {
            inventory
                .gaps
                .push(probe_gap(InventoryGapReason::InvalidToolOutput));
            return;
        };

        for (runtime_identifier, devices) in document.devices {
            for device in devices {
                let (Some(udid), Some(name), Some(available), Some(data_path)) = (
                    device.udid,
                    device.name,
                    device.is_available,
                    device.data_path,
                ) else {
                    inventory
                        .gaps
                        .push(probe_gap(InventoryGapReason::InvalidToolOutput));
                    continue;
                };
                if !valid_udid(&udid) {
                    inventory
                        .gaps
                        .push(probe_gap(InventoryGapReason::InvalidToolOutput));
                    continue;
                }
                let expected_suffix = Path::new("Library/Developer/CoreSimulator/Devices")
                    .join(&udid)
                    .join("data");
                if !Path::new(&data_path).is_absolute()
                    || !Path::new(&data_path).ends_with(&expected_suffix)
                {
                    inventory
                        .gaps
                        .push(probe_gap(InventoryGapReason::InvalidToolOutput));
                    continue;
                }
                let path = devices_root.join(&udid);
                if excluded(&path, self.excludes) {
                    continue;
                }
                let (measurements, observations) = match measure_store(
                    self.root,
                    &path,
                    self.excludes,
                    self.volume_has_snapshots.unwrap_or(false),
                ) {
                    Ok(measured) => measured,
                    Err(error) => {
                        inventory.gaps.push(io_gap(
                            "xcode.simulator_devices",
                            &path,
                            InventoryStage::MeasureObject,
                            &error,
                        ));
                        continue;
                    }
                };
                let normalized_path = format!("~/Library/Developer/CoreSimulator/Devices/{udid}");
                inventory.items.push(InventoryItem {
                    rule_id: "xcode.simulator_device".to_owned(),
                    normalized_path,
                    path: Some(path),
                    measurements,
                    observations,
                    identity: InventoryIdentity::SimulatorDevice {
                        udid,
                        name,
                        runtime_identifier: runtime_identifier.clone(),
                        available,
                    },
                });
            }
        }
    }

    fn inventory_runtimes(&self, ctx: &mut PolicyCtx<'_>, inventory: &mut Inventory) {
        let output = match ctx.run(SIMCTL_RUNTIMES) {
            Ok(output) if output.success => output,
            Ok(_) => {
                inventory
                    .gaps
                    .push(probe_gap(InventoryGapReason::ProbeFailed));
                return;
            }
            Err(error) => {
                inventory.gaps.push(probe_gap(policy_gap(error)));
                return;
            }
        };
        record_warning(&output.stderr, inventory);
        let Ok(document) = serde_json::from_slice::<RuntimesDocument>(&output.stdout) else {
            inventory
                .gaps
                .push(probe_gap(InventoryGapReason::InvalidToolOutput));
            return;
        };
        let mut any_runtime = false;

        for runtime in document.runtimes {
            let (Some(identifier), Some(name), Some(version), Some(build), Some(available)) = (
                runtime.identifier,
                runtime.name,
                runtime.version,
                runtime.build,
                runtime.is_available,
            ) else {
                inventory
                    .gaps
                    .push(probe_gap(InventoryGapReason::InvalidToolOutput));
                continue;
            };
            any_runtime = true;
            if !valid_runtime_identifier(&identifier) {
                inventory
                    .gaps
                    .push(probe_gap(InventoryGapReason::InvalidToolOutput));
                continue;
            }
            let normalized_path = runtime
                .bundle_path
                .as_deref()
                .and_then(|path| normalized_report_path(self.root.path(), Path::new(path)).ok())
                .unwrap_or_else(|| format!("/@simctl/runtimes/{identifier}"));
            inventory.items.push(InventoryItem {
                rule_id: "xcode.simulator_runtime".to_owned(),
                normalized_path,
                path: None,
                measurements: vec![Measurement {
                    plane: MeasurementPlane::ToolchainAttribution,
                    basis: MeasurementBasis::VendorReported,
                    scope: MeasurementScope {
                        kind: MeasurementScopeKind::ToolchainStore,
                        id: identifier.clone(),
                    },
                    coverage: MeasurementCoverage {
                        status: MeasurementCoverageStatus::Unmeasurable,
                        gap_ids: vec!["xcode.simulator_runtime.size".to_owned()],
                    },
                    value: MeasurementValue::Unmeasurable,
                }],
                observations: Vec::new(),
                identity: InventoryIdentity::SimulatorRuntime {
                    identifier,
                    name,
                    version,
                    build,
                    available,
                },
            });
        }
        if any_runtime {
            inventory.gaps.push(InventoryGap {
                region: "xcode.simulator_runtimes",
                path: None,
                reason: InventoryGapReason::RuntimeSizeUnavailable,
                stage: Some(InventoryStage::SimctlRuntimes),
                errno: None,
            });
        }
    }
}

#[derive(Deserialize)]
struct DevicesDocument {
    devices: BTreeMap<String, Vec<SimctlDevice>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimctlDevice {
    udid: Option<String>,
    name: Option<String>,
    is_available: Option<bool>,
    data_path: Option<String>,
}

#[derive(Deserialize)]
struct RuntimesDocument {
    runtimes: Vec<SimctlRuntime>,
}

#[derive(Deserialize)]
struct SimctlRuntime {
    identifier: Option<String>,
    name: Option<String>,
    version: Option<String>,
    #[serde(rename = "buildversion")]
    build: Option<String>,
    #[serde(rename = "isAvailable")]
    is_available: Option<bool>,
    #[serde(rename = "bundlePath")]
    bundle_path: Option<String>,
}

fn expand_home_pattern(
    root: &Root,
    pattern: &str,
    excludes: &[PathBuf],
) -> Result<Vec<PathBuf>, (PathBuf, io::Error)> {
    let Some(relative) = pattern.strip_prefix("~/") else {
        return Ok(Vec::new());
    };
    let mut current = vec![root.path().to_path_buf()];
    for component in relative.split('/') {
        let mut next = Vec::new();
        for parent in current {
            if excluded(&parent, excludes) {
                continue;
            }
            let children = match root.children(&parent) {
                Ok(children) => children,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err((parent, error)),
            };
            for child in children {
                if excluded(&child.path, excludes) {
                    continue;
                }
                let wildcard = component == "*";
                let matches =
                    wildcard || child.path.file_name().is_some_and(|name| name == component);
                if matches {
                    if child.kind != RootEntryKind::Directory {
                        // A wildcard enumerates candidate stores, and a real store directory sits
                        // next to unrelated files -- `CoreSimulator/Devices` always holds
                        // `device_set.plist` beside the device directories. Aborting the whole
                        // expansion there loses every sibling store. A literal component is
                        // different: the rule named that path, so a kind mismatch is a rule
                        // defect worth surfacing.
                        if wildcard {
                            continue;
                        }
                        return Err((
                            child.path,
                            io::Error::other("rule path matched an unsupported entry kind"),
                        ));
                    }
                    next.push(child.path);
                }
            }
        }
        current = next;
    }
    Ok(current)
}

fn measure_store(
    root: &Root,
    path: &Path,
    excludes: &[PathBuf],
    volume_has_snapshots: bool,
) -> io::Result<(Vec<Measurement>, Vec<SignalObservation>)> {
    let mut stack = vec![path.to_path_buf()];
    let mut objects = BTreeMap::<_, (ObjectMeasurements, u64)>::new();
    while let Some(directory) = stack.pop() {
        if excluded(&directory, excludes) {
            continue;
        }
        for entry in root.children(&directory)? {
            if excluded(&entry.path, excludes) {
                continue;
            }
            match entry.kind {
                RootEntryKind::Directory => stack.push(entry.path),
                RootEntryKind::File | RootEntryKind::Symlink | RootEntryKind::Other => {
                    let measured = root.measure_object(&entry.path)?;
                    let value = objects.entry(measured.identity).or_insert((measured, 0));
                    value.1 = value
                        .1
                        .checked_add(1)
                        .ok_or_else(|| io::Error::other("hardlink count overflow"))?;
                }
            }
        }
    }

    let mut logical_bytes = 0_u64;
    let mut allocated_bytes = Some(0_u64);
    let mut extents = Vec::new();
    let mut observations = Vec::new();
    if volume_has_snapshots {
        observations.push(observation(
            SignalId::VolumeHasSnapshots,
            ObservationRelation::PossibleWidthExplanation,
            ObservationScope::Volume,
        ));
    }
    for (measured, covered_link_count) in objects.values() {
        logical_bytes = logical_bytes
            .checked_add(measured.logical_bytes)
            .ok_or_else(|| io::Error::other("logical size overflow"))?;
        allocated_bytes = match (allocated_bytes, measured.allocated_bytes) {
            (Some(total), Some(bytes)) => total.checked_add(bytes),
            _ => None,
        };
        observations.extend(object_observations(measured));
        extents.push(ExtentObservation {
            identity: measured.identity,
            kind: ExtentKind::FileForks,
            link_count: measured.link_count,
            covered_link_count: *covered_link_count,
            allocated_bytes: measured.allocated_bytes,
            private_bytes: measured.private_bytes,
            signals: Vec::<StorageSignal>::new(),
        });
    }
    let estimate = estimate_disposition(&extents, false)
        .map_err(|_| io::Error::other("disposition estimate overflow"))?;
    observations.sort();
    observations.dedup();
    let scope = normalized_report_path(root.path(), path)
        .map_err(|_| io::Error::other("store path is not normalized"))?;
    let measured_coverage = MeasurementCoverage {
        status: MeasurementCoverageStatus::Complete,
        gap_ids: Vec::new(),
    };
    let interval_coverage = MeasurementCoverage {
        status: if estimate.has_unmeasurable_objects {
            MeasurementCoverageStatus::Partial
        } else {
            MeasurementCoverageStatus::Complete
        },
        gap_ids: if estimate.has_unmeasurable_objects {
            vec!["private_or_allocated_size_unmeasurable".to_owned()]
        } else {
            Vec::new()
        },
    };

    Ok((
        vec![
            Measurement {
                plane: MeasurementPlane::ToolchainAttribution,
                basis: MeasurementBasis::LogicalSize,
                scope: MeasurementScope {
                    kind: MeasurementScopeKind::ToolchainStore,
                    id: scope.clone(),
                },
                coverage: measured_coverage,
                value: MeasurementValue::ExactBytes {
                    bytes: logical_bytes,
                },
            },
            Measurement {
                plane: MeasurementPlane::ToolchainAttribution,
                basis: MeasurementBasis::AllocatedFootprint,
                scope: MeasurementScope {
                    kind: MeasurementScopeKind::ToolchainStore,
                    id: scope.clone(),
                },
                coverage: MeasurementCoverage {
                    status: if allocated_bytes.is_some() {
                        MeasurementCoverageStatus::Complete
                    } else {
                        MeasurementCoverageStatus::Unmeasurable
                    },
                    gap_ids: if allocated_bytes.is_some() {
                        Vec::new()
                    } else {
                        vec!["allocated_size_unmeasurable".to_owned()]
                    },
                },
                value: allocated_bytes.map_or(MeasurementValue::Unmeasurable, |bytes| {
                    MeasurementValue::ExactBytes { bytes }
                }),
            },
            Measurement {
                plane: MeasurementPlane::DispositionEstimate,
                basis: MeasurementBasis::PrivateFloorAllocatedCeiling,
                scope: MeasurementScope {
                    kind: MeasurementScopeKind::ObjectSet,
                    id: scope,
                },
                coverage: interval_coverage,
                value: MeasurementValue::IntervalBytes {
                    floor_bytes: estimate.floor_bytes,
                    ceiling_bytes: estimate.ceiling_bytes,
                    applicable_action: DispositionAction::PermanentUnlinkAfterReferencesClose,
                },
            },
        ],
        observations,
    ))
}

fn object_observations(measured: &ObjectMeasurements) -> Vec<SignalObservation> {
    let mut observations = Vec::new();
    let extended = measured.extended_flags.unwrap_or_default();
    if extended & 0x0000_0001 != 0 {
        observations.push(observation(
            SignalId::MayShareBlocks,
            ObservationRelation::PossibleWidthExplanation,
            ObservationScope::Inode,
        ));
    }
    if measured.resource_fork_allocated_bytes.unwrap_or_default() > 0 {
        observations.push(observation(
            SignalId::ResourceForkAllocated,
            ObservationRelation::TestedWidthCorrelate,
            ObservationScope::Object,
        ));
    }
    if measured.bsd_flags & 0x0000_0020 != 0 {
        observations.push(observation(
            SignalId::FilesystemCompressed,
            ObservationRelation::TestedWidthCorrelate,
            ObservationScope::Object,
        ));
    }
    if extended & 0x0000_0010 != 0 {
        observations.push(observation(
            SignalId::Sparse,
            ObservationRelation::LogicalAllocationGap,
            ObservationScope::Object,
        ));
    }
    if extended & 0x0000_0008 != 0 {
        observations.push(observation(
            SignalId::Purgeable,
            ObservationRelation::ReclaimPolicy,
            ObservationScope::Object,
        ));
    }
    if measured.link_count > 1 {
        observations.push(observation(
            SignalId::MultipleHardlinks,
            ObservationRelation::DeletionScope,
            ObservationScope::Inode,
        ));
    }
    observations
}

const fn observation(
    signal: SignalId,
    relation: ObservationRelation,
    scope: ObservationScope,
) -> SignalObservation {
    SignalObservation {
        observation: ObservationKind::Direct,
        signal,
        relation,
        scope,
    }
}

fn excluded(path: &Path, excludes: &[PathBuf]) -> bool {
    excludes.iter().any(|excluded| path.starts_with(excluded))
}

fn valid_udid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn valid_runtime_identifier(value: &str) -> bool {
    value.starts_with("com.apple.CoreSimulator.SimRuntime.")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
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

fn policy_gap(error: PolicyError) -> InventoryGapReason {
    match error {
        PolicyError::TimedOut(_) => InventoryGapReason::TimedOut,
        PolicyError::Disabled(_) => InventoryGapReason::Disabled,
        PolicyError::UndeclaredProbe(_)
        | PolicyError::CallLimitExceeded(_)
        | PolicyError::InvocationFailed(_) => InventoryGapReason::ProbeFailed,
    }
}

const fn probe_gap(reason: InventoryGapReason) -> InventoryGap {
    InventoryGap::diagnostic("xcode.simctl", reason)
}

fn io_gap(
    region: &'static str,
    path: &Path,
    stage: InventoryStage,
    error: &io::Error,
) -> InventoryGap {
    let errno = error.raw_os_error();
    let reason = match errno {
        Some(2) => InventoryGapReason::AbsentOrChanged,
        Some(13) => InventoryGapReason::AccessDenied,
        Some(1) => InventoryGapReason::PolicyDeniedUnknown,
        _ => InventoryGapReason::TraversalFailed,
    };
    InventoryGap {
        region,
        path: Some(path.to_path_buf()),
        reason,
        stage: Some(stage),
        errno,
    }
}

fn record_warning(stderr: &[u8], inventory: &mut Inventory) {
    let diagnostic = String::from_utf8_lossy(stderr);
    if !diagnostic.trim().is_empty() {
        eprintln!("sizetrail: simctl diagnostic: {}", diagnostic.trim());
        inventory.warnings.push("simctl_stderr_nonempty".to_owned());
    }
}

const VERIFIED_VERSIONS: &[(&str, &str, &str)] = &[
    ("16.4", "16F6", "1010.15"),
    ("26.6", "17F113", "1051.55"),
    ("27.0", "27A5228h", "1169.1"),
];

fn core_simulator_compatible(
    ctx: &mut PolicyCtx<'_>,
    state: &AdapterState,
) -> Result<(), InventoryGapReason> {
    let AdapterState::Ready { version } = state else {
        return Err(InventoryGapReason::UnknownVersion);
    };
    let expected = VERIFIED_VERSIONS
        .iter()
        .find_map(|(xcode, build, core_simulator)| {
            (version == &format!("{xcode} ({build})")).then_some(*core_simulator)
        })
        .ok_or(InventoryGapReason::UnknownVersion)?;
    let output = ctx.run(CORE_SIMULATOR_VERSION).map_err(policy_gap)?;
    if !output.success {
        return Err(InventoryGapReason::ProbeFailed);
    }
    let observed = std::str::from_utf8(&output.stdout)
        .map_err(|_| InventoryGapReason::InvalidToolOutput)?
        .trim();
    if observed != expected {
        return Err(InventoryGapReason::CoreSimulatorVersionMismatch);
    }
    Ok(())
}

pub fn probe(ctx: &mut PolicyCtx<'_>) -> AdapterState {
    let selected = match ctx.run(SELECT_DEVELOPER_DIR) {
        Ok(output) => output,
        Err(PolicyError::InvocationFailed(_)) => return AdapterState::NotPresent,
        Err(error) => return degraded_from_policy(error),
    };
    if !selected.success {
        return AdapterState::NotPresent;
    }
    let developer_dir = String::from_utf8_lossy(&selected.stdout);
    let developer_dir = developer_dir.trim();
    if developer_dir.ends_with("/Library/Developer/CommandLineTools") {
        return AdapterState::NotPresent;
    }
    if !developer_dir.ends_with("/Contents/Developer") || !developer_dir.contains(".app/") {
        return AdapterState::Degraded {
            observed_version: None,
            reason: AdapterDegradedReason::InvalidSelection,
        };
    }

    let version = match ctx.run(XCODEBUILD_VERSION) {
        Ok(output) if output.success => output,
        Ok(_) => {
            return AdapterState::Degraded {
                observed_version: None,
                reason: AdapterDegradedReason::ProbeFailed,
            };
        }
        Err(error) => return degraded_from_policy(error),
    };
    let text = String::from_utf8_lossy(&version.stdout);
    let mut lines = text.lines();
    let version_number = lines
        .next()
        .and_then(|line| line.strip_prefix("Xcode "))
        .map(str::to_owned);
    let build = lines
        .next()
        .and_then(|line| line.strip_prefix("Build version "))
        .map(str::to_owned);
    let verified =
        version_number
            .as_deref()
            .zip(build.as_deref())
            .is_some_and(|(version, build)| {
                VERIFIED_VERSIONS
                    .iter()
                    .any(|(known_version, known_build, _)| {
                        version == *known_version && build == *known_build
                    })
            });
    let observed_version = version_number
        .as_deref()
        .zip(build.as_deref())
        .map(|(version, build)| format!("{version} ({build})"))
        .or(version_number);

    match (verified, observed_version) {
        (true, Some(version)) => match ctx.run(FIRST_LAUNCH_STATUS) {
            Ok(output) if output.success => AdapterState::Ready { version },
            Ok(_) => AdapterState::Degraded {
                observed_version: Some(version),
                reason: AdapterDegradedReason::NotReady,
            },
            Err(error) => degraded_from_policy(error),
        },
        (_, observed_version) => AdapterState::Degraded {
            observed_version,
            reason: AdapterDegradedReason::UnknownVersion,
        },
    }
}

fn degraded_from_policy(error: PolicyError) -> AdapterState {
    AdapterState::Degraded {
        observed_version: None,
        reason: if matches!(error, PolicyError::Disabled(_)) {
            AdapterDegradedReason::Disabled
        } else {
            AdapterDegradedReason::ProbeFailed
        },
    }
}

#[cfg(test)]
mod io_gap_tests {
    use std::io;
    use std::path::Path;

    use super::{InventoryGapReason, InventoryStage, io_gap};

    #[test]
    fn target_errors_preserve_stage_and_errno_without_claiming_fda() {
        for (errno, reason) in [
            (2, InventoryGapReason::AbsentOrChanged),
            (13, InventoryGapReason::AccessDenied),
            (1, InventoryGapReason::PolicyDeniedUnknown),
        ] {
            let gap = io_gap(
                "xcode.fixture",
                Path::new("/fixture/target"),
                InventoryStage::ListDirectory,
                &io::Error::from_raw_os_error(errno),
            );

            assert_eq!(gap.reason, reason);
            assert_eq!(gap.stage, Some(InventoryStage::ListDirectory));
            assert_eq!(gap.errno, Some(errno));
        }
    }
}
