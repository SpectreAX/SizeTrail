use crate::adapters::xcode::XcodeAdapter;
use crate::adapters::{AdapterState, InventoryGapReason, ToolchainAdapter};
use crate::capacity::CapacityReport;
use crate::fsx::Root;
use crate::model::{
    CoverageGap, CoverageGapReason, EnvironmentEnvelope, Finding, MeasurementPlane, RegionReport,
    RegionStatus, SCHEMA_VERSION, ScanDocument, ScanPayload, normalize_findings,
};
use crate::policy::PolicyCtx;

#[derive(serde::Serialize)]
pub struct AdapterReport {
    pub id: String,
    pub status: RegionStatus,
    pub tool_version: Option<String>,
    pub warnings: Vec<String>,
    pub findings: Vec<Finding>,
    pub coverage_gaps: Vec<CoverageGap>,
}

pub fn xcode_report(
    root: &Root,
    ctx: &mut PolicyCtx<'_>,
    excludes: &[std::path::PathBuf],
) -> AdapterReport {
    xcode_report_with_sink(root, ctx, excludes, |_| {})
}

pub fn xcode_report_with_sink(
    root: &Root,
    ctx: &mut PolicyCtx<'_>,
    excludes: &[std::path::PathBuf],
    mut finding_sink: impl FnMut(&Finding),
) -> AdapterReport {
    let adapter = XcodeAdapter::new(root, excludes);
    let state = adapter.probe(ctx);
    let mut inventory = crate::adapters::Inventory::default();
    let mut findings = Vec::new();
    adapter.visit_inventory_stages(ctx, &state, |mut stage| {
        match adapter.classify(&stage) {
            Ok(stage_findings) => {
                for finding in &stage_findings {
                    finding_sink(finding);
                }
                findings.extend(stage_findings);
            }
            Err(reason) => stage
                .gaps
                .push(crate::adapters::InventoryGap::diagnostic("xcode", reason)),
        }
        inventory.gaps.append(&mut stage.gaps);
        inventory.warnings.append(&mut stage.warnings);
    });
    inventory.gaps.sort_by(|left, right| {
        left.region
            .cmp(right.region)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.reason.cmp(&right.reason))
    });
    normalize_findings(&mut findings);
    let mut coverage_gaps = inventory
        .gaps
        .into_iter()
        .enumerate()
        .map(|(index, gap)| CoverageGap {
            id: format!("xcode.{}.{index}", gap_reason_id(gap.reason)),
            plane: MeasurementPlane::ToolchainAttribution,
            region: gap.region.to_owned(),
            path: gap
                .path
                .as_deref()
                .and_then(|path| crate::model::normalized_report_path(root.path(), path).ok()),
            status: RegionStatus::Unmeasurable,
            reason: coverage_reason(gap.reason),
            stage: gap.stage.map(|stage| stage.as_str().to_owned()),
            errno: gap.errno,
        })
        .collect::<Vec<_>>();
    for (index, path) in excludes.iter().enumerate() {
        coverage_gaps.push(CoverageGap {
            id: format!("xcode.excluded_by_user.{index}"),
            plane: MeasurementPlane::ToolchainAttribution,
            region: "xcode".to_owned(),
            path: crate::model::normalized_report_path(root.path(), path).ok(),
            status: RegionStatus::ExcludedByUser,
            reason: CoverageGapReason::ExcludedByUser,
            stage: None,
            errno: None,
        });
    }
    let (status, tool_version) = match state {
        AdapterState::Ready { version } => (
            if coverage_gaps
                .iter()
                .all(|gap| gap.status != RegionStatus::Unmeasurable)
            {
                RegionStatus::Complete
            } else {
                RegionStatus::Unmeasurable
            },
            Some(version),
        ),
        AdapterState::NotPresent => (RegionStatus::NotPresent, None),
        AdapterState::Degraded {
            observed_version, ..
        } => (RegionStatus::Unmeasurable, observed_version),
    };

    AdapterReport {
        id: "xcode".to_owned(),
        status,
        tool_version,
        warnings: inventory.warnings,
        findings,
        coverage_gaps,
    }
}

#[must_use]
pub fn excluded_adapter_report(id: &str) -> AdapterReport {
    AdapterReport {
        id: id.to_owned(),
        status: RegionStatus::ExcludedByUser,
        tool_version: None,
        warnings: Vec::new(),
        findings: Vec::new(),
        coverage_gaps: Vec::new(),
    }
}

pub fn unmeasurable_adapter_report(id: &str) -> AdapterReport {
    AdapterReport {
        id: id.to_owned(),
        status: RegionStatus::Unmeasurable,
        tool_version: None,
        warnings: Vec::new(),
        findings: Vec::new(),
        coverage_gaps: vec![CoverageGap {
            id: format!("{id}.root_unmeasurable"),
            plane: MeasurementPlane::ToolchainAttribution,
            region: id.to_owned(),
            path: None,
            status: RegionStatus::Unmeasurable,
            reason: CoverageGapReason::RootUnmeasurable,
            stage: Some("root_initialization".to_owned()),
            errno: None,
        }],
    }
}

const fn coverage_reason(reason: InventoryGapReason) -> CoverageGapReason {
    match reason {
        InventoryGapReason::AbsentOrChanged => CoverageGapReason::AbsentOrChanged,
        InventoryGapReason::AccessDenied => CoverageGapReason::AccessDenied,
        InventoryGapReason::PolicyDeniedUnknown => CoverageGapReason::PolicyDeniedUnknown,
        InventoryGapReason::UnknownVersion => CoverageGapReason::UnknownVersion,
        InventoryGapReason::NotReady => CoverageGapReason::NotReady,
        InventoryGapReason::Disabled => CoverageGapReason::Disabled,
        InventoryGapReason::ProbeFailed => CoverageGapReason::ProbeFailed,
        InventoryGapReason::TraversalFailed => CoverageGapReason::TraversalFailed,
        InventoryGapReason::InvalidToolOutput => CoverageGapReason::InvalidToolOutput,
        InventoryGapReason::RuntimeSizeUnavailable => CoverageGapReason::RuntimeSizeUnavailable,
        InventoryGapReason::TimedOut => CoverageGapReason::TimedOut,
        InventoryGapReason::RuleSetInvalid => CoverageGapReason::RuleSetInvalid,
    }
}

const fn gap_reason_id(reason: InventoryGapReason) -> &'static str {
    match reason {
        InventoryGapReason::AbsentOrChanged => "absent_or_changed",
        InventoryGapReason::AccessDenied => "access_denied",
        InventoryGapReason::PolicyDeniedUnknown => "policy_denied_unknown",
        InventoryGapReason::UnknownVersion => "unknown_version",
        InventoryGapReason::NotReady => "not_ready",
        InventoryGapReason::Disabled => "disabled",
        InventoryGapReason::ProbeFailed => "probe_failed",
        InventoryGapReason::TraversalFailed => "traversal_failed",
        InventoryGapReason::InvalidToolOutput => "invalid_tool_output",
        InventoryGapReason::RuntimeSizeUnavailable => "runtime_size_unavailable",
        InventoryGapReason::TimedOut => "timed_out",
        InventoryGapReason::RuleSetInvalid => "rule_set_invalid",
    }
}

pub fn scan(
    environment: EnvironmentEnvelope,
    capacity: CapacityReport,
    mut adapters: Vec<AdapterReport>,
) -> ScanDocument {
    adapters.sort_by(|left, right| left.id.cmp(&right.id));
    let mut coverage_gaps = if adapters.is_empty() {
        vec![CoverageGap {
            id: "p1.toolchain_adapters".to_owned(),
            plane: MeasurementPlane::ToolchainAttribution,
            region: "toolchain_adapters".to_owned(),
            path: None,
            status: RegionStatus::Unmeasurable,
            reason: CoverageGapReason::NoAdaptersCompiled,
            stage: None,
            errno: None,
        }]
    } else {
        Vec::new()
    };
    if capacity.status == RegionStatus::Unmeasurable {
        coverage_gaps.push(CoverageGap {
            id: "p2.capacity".to_owned(),
            plane: MeasurementPlane::Capacity,
            region: "capacity".to_owned(),
            path: None,
            status: RegionStatus::Unmeasurable,
            reason: CoverageGapReason::RootUnmeasurable,
            stage: Some("capacity_measurement".to_owned()),
            errno: None,
        });
    }
    let mut regions = vec![RegionReport {
        id: "capacity".to_owned(),
        status: capacity.status,
        warnings: Vec::new(),
    }];
    let mut findings = Vec::new();
    for adapter in adapters {
        regions.push(RegionReport {
            id: adapter.id,
            status: adapter.status,
            warnings: adapter.warnings,
        });
        findings.extend(adapter.findings);
        coverage_gaps.extend(adapter.coverage_gaps);
    }
    normalize_findings(&mut findings);
    coverage_gaps.sort_by(|left, right| left.id.cmp(&right.id));

    ScanDocument {
        schema_version: SCHEMA_VERSION,
        environment,
        payload: ScanPayload {
            capacity: capacity.values,
            regions,
            findings,
            coverage_gaps,
        },
    }
}
