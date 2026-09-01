use std::collections::BTreeMap;
use std::path::{Component, Path};
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use serde::Serialize;

use crate::fsx::CapacityValue;

pub const SCHEMA_VERSION: &str = "0.1.0-unstable";
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const FINDING_ID_VERSION: &str = "f1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FindingIdError {
    InvalidAdapterId,
    InvalidRuleId,
    InvalidNormalizedPath,
}

pub fn normalized_report_path(home: &Path, path: &Path) -> Result<String, FindingIdError> {
    if !is_normalized_absolute(home) || !is_normalized_absolute(path) {
        return Err(FindingIdError::InvalidNormalizedPath);
    }
    if let Ok(relative) = path.strip_prefix(home) {
        let relative = relative.to_string_lossy();
        return Ok(if relative.is_empty() {
            "~".to_owned()
        } else {
            format!("~/{relative}")
        });
    }
    Ok(path.to_string_lossy().into_owned())
}

pub fn finding_id(
    adapter_id: &str,
    rule_id: &str,
    canonical_subject_key: &str,
) -> Result<String, FindingIdError> {
    if !valid_id(adapter_id) || adapter_id.contains(':') {
        return Err(FindingIdError::InvalidAdapterId);
    }
    if !valid_id(rule_id) {
        return Err(FindingIdError::InvalidRuleId);
    }
    if !valid_canonical_subject_key(canonical_subject_key) {
        return Err(FindingIdError::InvalidNormalizedPath);
    }

    let mut digest = 0xcbf2_9ce4_8422_2325_u64;
    for component in [adapter_id, rule_id, canonical_subject_key] {
        for byte in component.bytes().chain([0]) {
            digest ^= u64::from(byte);
            digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    Ok(format!("{FINDING_ID_VERSION}:{adapter_id}:{digest:016x}"))
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
}

fn valid_canonical_subject_key(value: &str) -> bool {
    valid_normalized_subject(value) || value.strip_prefix("object_set:").is_some_and(valid_id)
}

fn valid_normalized_subject(value: &str) -> bool {
    (value == "~" || value.starts_with("~/") || value.starts_with('/'))
        && !value
            .split('/')
            .any(|component| matches!(component, "." | ".."))
}

fn is_normalized_absolute(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
}

#[derive(Debug, Serialize)]
pub struct ScanDocument {
    pub schema_version: &'static str,
    /// Peer of `schema_version` rather than part of `environment`: the build is not host-dependent,
    /// so it must not be replaced by the fixture's injected environment, and the two versions
    /// advance on different schedules (Q48).
    pub tool_version: &'static str,
    pub environment: EnvironmentEnvelope,
    pub payload: ScanPayload,
}

#[derive(Debug, Serialize)]
pub struct EnvironmentEnvelope {
    pub generated_at_unix_seconds: u64,
    pub hostname: String,
    pub home: String,
    pub tool_versions: BTreeMap<String, String>,
}

impl EnvironmentEnvelope {
    pub fn capture(root: Option<&Path>) -> Result<Self, SystemTimeError> {
        let generated_at_unix_seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_owned());
        let home = root.map_or_else(
            || std::env::var("HOME").unwrap_or_else(|_| "unknown".to_owned()),
            |path| path.to_string_lossy().into_owned(),
        );

        Ok(Self {
            generated_at_unix_seconds,
            hostname,
            home,
            tool_versions: BTreeMap::new(),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct ScanPayload {
    pub capacity: Vec<CapacityValue>,
    pub regions: Vec<RegionReport>,
    pub findings: Vec<Finding>,
    pub coverage_gaps: Vec<CoverageGap>,
}

#[derive(Debug, Serialize)]
pub struct RegionReport {
    pub id: String,
    pub status: RegionStatus,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionStatus {
    Complete,
    NotPresent,
    ExcludedByUser,
    /// A gap that documents a permanent product-scope boundary. It must not mark the
    /// owning region unmeasurable or produce exit code 3 (Q54).
    DeclaredScopeBoundary,
    Unmeasurable,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FindingSubject {
    FilesystemPath { normalized_path: String },
    ToolchainObjectSet { object_set_id: String },
}

impl FindingSubject {
    pub fn canonical_key(&self) -> Result<String, FindingIdError> {
        match self {
            Self::FilesystemPath { normalized_path }
                if valid_normalized_subject(normalized_path) =>
            {
                Ok(normalized_path.clone())
            }
            Self::FilesystemPath { .. } => Err(FindingIdError::InvalidNormalizedPath),
            Self::ToolchainObjectSet { object_set_id } if valid_id(object_set_id) => {
                Ok(format!("object_set:{object_set_id}"))
            }
            Self::ToolchainObjectSet { .. } => Err(FindingIdError::InvalidNormalizedPath),
        }
    }

    #[must_use]
    pub fn filesystem_path(&self) -> Option<&str> {
        match self {
            Self::FilesystemPath { normalized_path } => Some(normalized_path),
            Self::ToolchainObjectSet { .. } => None,
        }
    }

    #[must_use]
    pub fn display_value(&self) -> &str {
        match self {
            Self::FilesystemPath { normalized_path } => normalized_path,
            Self::ToolchainObjectSet { object_set_id } => object_set_id,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Finding {
    pub id: String,
    pub adapter_id: String,
    pub rule_id: String,
    pub title: String,
    pub summary: String,
    pub subject: FindingSubject,
    pub mechanism: String,
    pub recoverability: String,
    pub sensitivity: String,
    pub evidence: String,
    pub unexplained_private_gap: bool,
    pub measurements: Vec<Measurement>,
    pub observations: Vec<SignalObservation>,
    pub advice: Vec<Advice>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    Direct,
    Derived,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationRelation {
    TestedWidthCorrelate,
    PossibleWidthExplanation,
    LogicalAllocationGap,
    ReclaimPolicy,
    DeletionScope,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalId {
    FilesystemCompressed,
    ResourceForkAllocated,
    MayShareBlocks,
    VolumeHasSnapshots,
    Sparse,
    Purgeable,
    MultipleHardlinks,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationScope {
    Object,
    Inode,
    Volume,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SignalObservation {
    pub observation: ObservationKind,
    pub signal: SignalId,
    pub relation: ObservationRelation,
    pub scope: ObservationScope,
}

pub fn normalize_findings(findings: &mut [Finding]) {
    for finding in findings.iter_mut() {
        finding.observations.sort_by_key(|observation| {
            (observation.relation, observation.signal, observation.scope)
        });
        finding.observations.dedup();
        finding.summary = finding.observations.first().map_or_else(
            || "no allocation signal observed".to_owned(),
            signal_summary,
        );
    }
    findings.sort_by(|left, right| {
        observation_key(left)
            .cmp(&observation_key(right))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn observation_key(finding: &Finding) -> (bool, Option<&SignalObservation>) {
    (
        finding.observations.is_empty(),
        finding.observations.first(),
    )
}

fn signal_summary(observation: &SignalObservation) -> String {
    match observation.signal {
        SignalId::FilesystemCompressed => {
            "compressed storage makes the private floor uninformative"
        }
        SignalId::ResourceForkAllocated => {
            "resource-fork allocation is correlated with private-floor uncertainty"
        }
        SignalId::MayShareBlocks => "clone sharing may contribute to allocation uncertainty",
        SignalId::VolumeHasSnapshots => "volume snapshots may contribute to allocation uncertainty",
        SignalId::Sparse => "sparse allocation explains part of the logical-size gap",
        SignalId::Purgeable => "the filesystem marks some content as purgeable",
        SignalId::MultipleHardlinks => "multiple hardlinks widen the deletion scope",
    }
    .to_owned()
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Advice {
    Command(CommandAdvice),
    Reveal(RevealAdvice),
}

#[derive(Debug, Serialize)]
pub struct CommandAdvice {
    pub display_command: String,
    pub impact: AdviceImpact,
    pub explanation: String,
    pub reliable_preview_available: bool,
}

#[derive(Debug, Serialize)]
pub struct RevealAdvice {
    pub normalized_path: String,
    pub recovery_semantics: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdviceImpact {
    Inspect,
    Reversible,
    Destructive,
}

#[derive(Clone, Debug, Serialize)]
pub struct Measurement {
    pub plane: MeasurementPlane,
    pub quantity: MeasurementQuantity,
    pub basis: MeasurementBasis,
    pub scope: MeasurementScope,
    pub coverage: MeasurementCoverage,
    pub value: MeasurementValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementQuantity {
    LogicalSize,
    AllocatedFootprint,
    DispositionEstimate,
    VendorReportedSize,
    DiskImageLogicalLimit,
    HostAllocatedFootprint,
    DaemonUsed,
    DaemonReclaimable,
    ObjectCount,
    ActiveObjectCount,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementPlane {
    Capacity,
    ToolchainAttribution,
    DispositionEstimate,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementBasis {
    LogicalSize,
    AllocatedFootprint,
    PrivateSize,
    VolumeSpaceUsed,
    VendorReported,
    DockerSystemDf,
    PrivateFloorAllocatedCeiling,
}

#[derive(Clone, Debug, Serialize)]
pub struct MeasurementScope {
    pub kind: MeasurementScopeKind,
    pub id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementScopeKind {
    Container,
    Volume,
    ToolchainStore,
    ObjectSet,
}

#[derive(Clone, Debug, Serialize)]
pub struct MeasurementCoverage {
    pub status: MeasurementCoverageStatus,
    pub gap_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementCoverageStatus {
    Complete,
    Partial,
    Unmeasurable,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeasurementValue {
    ExactBytes {
        bytes: u64,
    },
    IntervalBytes {
        floor_bytes: u64,
        ceiling_bytes: Option<u64>,
        applicable_action: DispositionAction,
    },
    RoundedBytes {
        reported: String,
        lower_bound_bytes: u64,
        upper_bound_bytes: u64,
    },
    ExactCount {
        count: u64,
    },
    Unmeasurable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoundedBytesError;

pub fn rounded_bytes(reported: &str) -> Result<MeasurementValue, RoundedBytesError> {
    let mut fields = reported.split_ascii_whitespace();
    let token = fields.next().ok_or(RoundedBytesError)?;
    if let Some(percent) = fields.next()
        && (fields.next().is_some()
            || percent.len() <= 3
            || !percent.starts_with('(')
            || !percent.ends_with("%)")
            || !percent[1..percent.len() - 2]
                .bytes()
                .all(|byte| byte.is_ascii_digit()))
    {
        return Err(RoundedBytesError);
    }

    let (number, unit_bytes) = [
        ("EB", 1_000_000_000_000_000_000_u64),
        ("PB", 1_000_000_000_000_000_u64),
        ("TB", 1_000_000_000_000_u64),
        ("GB", 1_000_000_000_u64),
        ("MB", 1_000_000_u64),
        ("kB", 1_000_u64),
        ("B", 1_u64),
    ]
    .into_iter()
    .find_map(|(suffix, bytes)| token.strip_suffix(suffix).map(|number| (number, bytes)))
    .ok_or(RoundedBytesError)?;
    let mut parts = number.split('.');
    let whole = parts.next().ok_or(RoundedBytesError)?;
    let fraction = parts.next().unwrap_or("");
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(RoundedBytesError);
    }
    let scale = 10_u64
        .checked_pow(u32::try_from(fraction.len()).map_err(|_| RoundedBytesError)?)
        .ok_or(RoundedBytesError)?;
    let mantissa = format!("{whole}{fraction}")
        .parse::<u64>()
        .map_err(|_| RoundedBytesError)?;
    let center = mantissa.checked_mul(unit_bytes).ok_or(RoundedBytesError)?;
    let (lower_bound_bytes, upper_bound_bytes) = if unit_bytes == 1 && fraction.is_empty() {
        (center, center)
    } else {
        let lower = center.saturating_sub(unit_bytes) / scale;
        let upper_numerator = center
            .checked_add(unit_bytes)
            .and_then(|value| value.checked_add(scale - 1))
            .ok_or(RoundedBytesError)?;
        (lower, upper_numerator / scale)
    };
    Ok(MeasurementValue::RoundedBytes {
        reported: reported.to_owned(),
        lower_bound_bytes,
        upper_bound_bytes,
    })
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DispositionAction {
    PermanentUnlinkAfterReferencesClose,
}

#[derive(Debug, Serialize)]
pub struct CoverageGap {
    pub id: String,
    pub plane: MeasurementPlane,
    pub region: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub status: RegionStatus,
    pub reason: CoverageGapReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errno: Option<i32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageGapReason {
    NoAdaptersCompiled,
    RootUnmeasurable,
    AbsentOrChanged,
    AccessDenied,
    PolicyDeniedUnknown,
    UnknownVersion,
    NotReady,
    Disabled,
    ProbeFailed,
    TraversalFailed,
    InvalidToolOutput,
    CoreSimulatorVersionMismatch,
    SimulatorIdentityUnavailable,
    RuntimeSizeUnavailable,
    TimedOut,
    RuleSetInvalid,
    VolumeSnapshotStateUnavailable,
    CaskArtifactOutsidePrefix,
    UnsupportedPathOverride,
    AmbiguousDiskImage,
    ExcludedByUser,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct FileIdentity {
    pub fsid: [i32; 2],
    pub fileid: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtentKind {
    FileForks,
    Directory,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageSignal {
    MayShareBlocks(bool),
    VolumeHasSnapshots(bool),
    ResourceForkAllocated,
    FilesystemCompressed,
    Sparse,
    Purgeable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtentObservation {
    pub identity: FileIdentity,
    pub kind: ExtentKind,
    pub link_count: u64,
    pub covered_link_count: u64,
    pub allocated_bytes: Option<u64>,
    pub private_bytes: Option<u64>,
    pub signals: Vec<StorageSignal>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispositionEstimate {
    pub floor_bytes: u64,
    pub ceiling_bytes: Option<u64>,
    pub has_unmeasurable_objects: bool,
    pub unexplained_private_gap: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeasurementOverflow;

pub fn estimate_disposition(
    observations: &[ExtentObservation],
    snapshots_stable: bool,
) -> Result<DispositionEstimate, MeasurementOverflow> {
    let mut unique = BTreeMap::new();
    for observation in observations {
        if observation.kind == ExtentKind::FileForks {
            unique.entry(observation.identity).or_insert(observation);
        }
    }

    let mut floor_bytes = 0_u64;
    let mut ceiling_bytes = Some(0_u64);
    let mut has_unmeasurable_objects = false;

    for observation in unique.values() {
        if snapshots_stable && observation.covered_link_count >= observation.link_count {
            match observation.private_bytes {
                Some(bytes) => {
                    floor_bytes = floor_bytes.checked_add(bytes).ok_or(MeasurementOverflow)?;
                }
                None => has_unmeasurable_objects = true,
            }
        }

        match (ceiling_bytes, observation.allocated_bytes) {
            (Some(total), Some(bytes)) => {
                ceiling_bytes = Some(total.checked_add(bytes).ok_or(MeasurementOverflow)?);
            }
            (_, None) => {
                ceiling_bytes = None;
                has_unmeasurable_objects = true;
            }
            (None, Some(_)) => {}
        }
    }

    Ok(DispositionEstimate {
        floor_bytes,
        ceiling_bytes,
        has_unmeasurable_objects,
        unexplained_private_gap: true,
    })
}
