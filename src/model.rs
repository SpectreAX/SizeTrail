use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use serde::Serialize;

pub const SCHEMA_VERSION: &str = "0.1.0-unstable";

#[derive(Debug, Serialize)]
pub struct ScanDocument {
    pub schema_version: &'static str,
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
    pub regions: Vec<RegionReport>,
    pub findings: Vec<Finding>,
    pub coverage_gaps: Vec<CoverageGap>,
}

#[derive(Debug, Serialize)]
pub struct RegionReport {
    pub id: String,
    pub status: RegionStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionStatus {
    Complete,
    NotPresent,
    ExcludedByUser,
    Unmeasurable,
}

#[derive(Debug, Serialize)]
pub struct Finding {
    pub id: String,
    pub adapter_id: String,
    pub rule_id: String,
    pub normalized_path: String,
    pub measurements: Vec<Measurement>,
}

#[derive(Debug, Serialize)]
pub struct Measurement {
    pub plane: MeasurementPlane,
    pub basis: MeasurementBasis,
    pub scope: MeasurementScope,
    pub coverage: MeasurementCoverage,
    pub value: MeasurementValue,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementPlane {
    Capacity,
    ToolchainAttribution,
    DispositionEstimate,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementBasis {
    LogicalSize,
    AllocatedFootprint,
    PrivateSize,
    VolumeSpaceUsed,
    VendorReported,
}

#[derive(Debug, Serialize)]
pub struct MeasurementScope {
    pub kind: MeasurementScopeKind,
    pub id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementScopeKind {
    Container,
    Volume,
    ToolchainStore,
    ObjectSet,
}

#[derive(Debug, Serialize)]
pub struct MeasurementCoverage {
    pub status: MeasurementCoverageStatus,
    pub gap_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementCoverageStatus {
    Complete,
    Partial,
    Unmeasurable,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeasurementValue {
    ExactBytes {
        bytes: u64,
    },
    IntervalBytes {
        floor_bytes: u64,
        ceiling_bytes: Option<u64>,
    },
    Unmeasurable,
}

#[derive(Debug, Serialize)]
pub struct CoverageGap {
    pub id: &'static str,
    pub plane: MeasurementPlane,
    pub region: &'static str,
    pub status: RegionStatus,
    pub reason: CoverageGapReason,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageGapReason {
    NoAdaptersCompiled,
}
