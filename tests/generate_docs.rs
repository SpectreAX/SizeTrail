#![allow(clippy::disallowed_methods)]

use std::fs;
use std::path::Path;

use serde::Serialize;
use sizetrail::capacity::CapacityReport;
use sizetrail::fsx::{CapacityBasis, CapacityKind, CapacityValue, UnmeasurableReason};
use sizetrail::model::{
    CoverageGapReason, DispositionAction, EnvironmentEnvelope, MeasurementBasis,
    MeasurementCoverageStatus, MeasurementPlane, MeasurementQuantity, MeasurementScopeKind,
    ObservationKind, ObservationRelation, ObservationScope, RegionStatus, SCHEMA_VERSION, SignalId,
};
use sizetrail::scan::scan;

#[test]
#[ignore = "writes the checked-in fixture-generated JSON example"]
fn generate_empty_scan_document() {
    let environment = EnvironmentEnvelope {
        generated_at_unix_seconds: 1_800_000_000,
        hostname: "fixture-host".to_owned(),
        home: "/Users/fixture".to_owned(),
        tool_versions: Default::default(),
    };
    let document = scan(
        environment,
        CapacityReport {
            status: RegionStatus::Complete,
            values: vec![CapacityValue::Measured {
                kind: CapacityKind::VolumeUsed,
                bytes: 4096,
                basis: CapacityBasis::AttrVolSpaceUsed,
            }],
        },
        Vec::new(),
    );
    let rendered = serde_json::to_string_pretty(&document).expect("document must serialize");
    let output = Path::new("docs/generated/empty-scan.json");

    fs::create_dir_all(output.parent().expect("generated document has a parent"))
        .expect("generated directory must exist");
    fs::write(output, format!("{rendered}\n")).expect("generated document must be written");

    let platforms: serde_json::Value = serde_json::from_str(
        &fs::read_to_string("ci/platforms.json").expect("platform source must be readable"),
    )
    .expect("platform source must be JSON");
    let mut support = format!(
        "Release: **{}**\n\nAPI baseline: **{}**\n\n| Hosted lane | Architecture | Evidence status |\n|---|---|---|\n",
        platforms["release"].as_str().expect("release must be text"),
        platforms["api_baseline"]
            .as_str()
            .expect("API baseline must be text")
    );
    for (key, absent_status) in [
        ("runtime_lanes", "experimental; non-blocking"),
        ("real_environment_lanes", "real environment; non-blocking"),
    ] {
        for lane in platforms[key]
            .as_array()
            .unwrap_or_else(|| panic!("{key} must be an array"))
        {
            support.push_str(&format!(
                "| {} (`{}`) | `{}` | {} |\n",
                lane["label"].as_str().expect("label must be text"),
                lane["runner"].as_str().expect("runner must be text"),
                lane["arch"].as_str().expect("architecture must be text"),
                if lane["required"] == true {
                    "required"
                } else {
                    absent_status
                }
            ));
        }
    }
    fs::write("docs/generated/support-matrix.md", support).expect("support matrix must be written");

    let capacity = document
        .payload
        .capacity
        .first()
        .expect("fixture must contain capacity evidence");
    let fixture_report = match capacity {
        CapacityValue::Measured { kind, bytes, basis } => format!(
            "The generated fixture reports `{bytes}` bytes for `{kind:?}` using `{basis:?}`.\n\nIt also reports `{}` structured coverage gap and never derives a global remainder.",
            document.payload.coverage_gaps.len()
        ),
        CapacityValue::Unmeasurable { .. } => {
            panic!("fixture capacity must be measured")
        }
    };
    fs::write(
        "docs/generated/fixture-report.md",
        format!("{fixture_report}\n"),
    )
    .expect("fixture report must be written");

    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "SizeTrail scan document",
        "type": "object",
        "additionalProperties": false,
        "required": ["schema_version", "tool_version", "environment", "payload"],
        "properties": {
            "schema_version": { "const": SCHEMA_VERSION },
            "tool_version": { "type": "string" },
            "environment": {
                "type": "object",
                "required": ["generated_at_unix_seconds", "hostname", "home", "tool_versions"],
                "properties": {
                    "generated_at_unix_seconds": { "type": "integer", "minimum": 0 },
                    "hostname": { "type": "string" },
                    "home": { "type": "string" },
                    "tool_versions": { "type": "object", "additionalProperties": { "type": "string" } }
                }
            },
            "payload": {
                "type": "object",
                "required": ["capacity", "regions", "findings", "coverage_gaps"],
                "properties": {
                    "capacity": { "type": "array" },
                    "regions": { "type": "array" },
                    "findings": { "type": "array" },
                    "coverage_gaps": { "type": "array" }
                }
            }
        },
        "enums": {
            "region_status": names(region_statuses()),
            "measurement_plane": names(measurement_planes()),
            "measurement_quantity": names(measurement_quantities()),
            "measurement_basis": names(measurement_bases()),
            "measurement_scope_kind": names(measurement_scope_kinds()),
            "measurement_coverage_status": names(measurement_coverage_statuses()),
            "coverage_gap_reason": names(coverage_gap_reasons()),
            "capacity_kind": names(capacity_kinds()),
            "capacity_basis": names(capacity_bases()),
            "unmeasurable_reason": names(unmeasurable_reasons()),
            "observation_kind": names(observation_kinds()),
            "observation_relation": names(observation_relations()),
            "observation_scope": names(observation_scopes()),
            "signal_id": names(signal_ids()),
            "disposition_action": names(disposition_actions()),
        }
    });
    write_pretty(
        "docs/generated/scan.schema.json",
        &serde_json::to_string_pretty(&schema).expect("schema must serialize"),
    );

    let gap_reasons = document
        .payload
        .coverage_gaps
        .iter()
        .map(|gap| {
            format!(
                "`{}`",
                serde_json::to_value(&gap.reason)
                    .expect("reason must serialize")
                    .as_str()
                    .expect("reason must be text")
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    write_pretty(
        "docs/generated/coverage-unknown-baseline.md",
        &format!(
            "Generated from the checked-in empty-scan fixture.\n\n- schema version `{SCHEMA_VERSION}`\n- findings: `{}`\n- coverage_gaps: `{}`\n- gap reasons: {gap_reasons}\n",
            document.payload.findings.len(),
            document.payload.coverage_gaps.len()
        ),
    );

    let mut basis = String::from(
        "Serialized measurement and capacity basis names locked from the compiled types.\n\n## Measurement basis\n\n",
    );
    for name in names(measurement_bases()) {
        basis.push_str(&format!("- `{name}`\n"));
    }
    basis.push_str("\n## Capacity basis\n\n");
    for name in names(capacity_bases()) {
        basis.push_str(&format!("- `{name}`\n"));
    }
    write_pretty("docs/generated/measurement-basis.md", &basis);
}

fn write_pretty(path: &str, contents: &str) {
    let mut text = contents.to_owned();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    fs::write(path, text).unwrap_or_else(|_| panic!("{path} must be written"));
}

fn names(values: impl IntoIterator<Item = impl Serialize>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| {
            serde_json::to_value(value)
                .expect("enum must serialize")
                .as_str()
                .expect("unit enum must serialize as a string")
                .to_owned()
        })
        .collect()
}

fn region_statuses() -> [RegionStatus; 5] {
    [
        RegionStatus::Complete,
        RegionStatus::NotPresent,
        RegionStatus::ExcludedByUser,
        RegionStatus::DeclaredScopeBoundary,
        RegionStatus::Unmeasurable,
    ]
}

fn measurement_planes() -> [MeasurementPlane; 3] {
    [
        MeasurementPlane::Capacity,
        MeasurementPlane::ToolchainAttribution,
        MeasurementPlane::DispositionEstimate,
    ]
}

fn measurement_quantities() -> [MeasurementQuantity; 10] {
    [
        MeasurementQuantity::LogicalSize,
        MeasurementQuantity::AllocatedFootprint,
        MeasurementQuantity::DispositionEstimate,
        MeasurementQuantity::VendorReportedSize,
        MeasurementQuantity::DiskImageLogicalLimit,
        MeasurementQuantity::HostAllocatedFootprint,
        MeasurementQuantity::DaemonUsed,
        MeasurementQuantity::DaemonReclaimable,
        MeasurementQuantity::ObjectCount,
        MeasurementQuantity::ActiveObjectCount,
    ]
}

fn measurement_bases() -> [MeasurementBasis; 7] {
    [
        MeasurementBasis::LogicalSize,
        MeasurementBasis::AllocatedFootprint,
        MeasurementBasis::PrivateSize,
        MeasurementBasis::VolumeSpaceUsed,
        MeasurementBasis::VendorReported,
        MeasurementBasis::DockerSystemDf,
        MeasurementBasis::PrivateFloorAllocatedCeiling,
    ]
}

fn measurement_scope_kinds() -> [MeasurementScopeKind; 4] {
    [
        MeasurementScopeKind::Container,
        MeasurementScopeKind::Volume,
        MeasurementScopeKind::ToolchainStore,
        MeasurementScopeKind::ObjectSet,
    ]
}

fn measurement_coverage_statuses() -> [MeasurementCoverageStatus; 3] {
    [
        MeasurementCoverageStatus::Complete,
        MeasurementCoverageStatus::Partial,
        MeasurementCoverageStatus::Unmeasurable,
    ]
}

fn coverage_gap_reasons() -> [CoverageGapReason; 22] {
    [
        CoverageGapReason::NoAdaptersCompiled,
        CoverageGapReason::RootUnmeasurable,
        CoverageGapReason::AbsentOrChanged,
        CoverageGapReason::AccessDenied,
        CoverageGapReason::PolicyDeniedUnknown,
        CoverageGapReason::UnknownVersion,
        CoverageGapReason::NotReady,
        CoverageGapReason::Disabled,
        CoverageGapReason::ProbeFailed,
        CoverageGapReason::TraversalFailed,
        CoverageGapReason::InvalidToolOutput,
        CoverageGapReason::CoreSimulatorVersionMismatch,
        CoverageGapReason::SimulatorIdentityUnavailable,
        CoverageGapReason::RuntimeSizeUnavailable,
        CoverageGapReason::TimedOut,
        CoverageGapReason::RuleSetInvalid,
        CoverageGapReason::VolumeSnapshotStateUnavailable,
        CoverageGapReason::CaskArtifactOutsidePrefix,
        CoverageGapReason::UnsupportedPathOverride,
        CoverageGapReason::AmbiguousDiskImage,
        CoverageGapReason::DaemonInventoryExcludesInactiveStore,
        CoverageGapReason::ExcludedByUser,
    ]
}

fn capacity_kinds() -> [CapacityKind; 7] {
    [
        CapacityKind::ContainerAllocated,
        CapacityKind::VolumeSize,
        CapacityKind::VolumeUsed,
        CapacityKind::VolumeFree,
        CapacityKind::AvailableNormal,
        CapacityKind::AvailableImportant,
        CapacityKind::AvailableOpportunistic,
    ]
}

fn capacity_bases() -> [CapacityBasis; 10] {
    [
        CapacityBasis::AttrVolSize,
        CapacityBasis::AttrVolSpaceUsed,
        CapacityBasis::AttrVolSpaceFree,
        CapacityBasis::AttrVolSpaceAvailable,
        CapacityBasis::StatfsBlocks,
        CapacityBasis::StatfsBlocksMinusFree,
        CapacityBasis::StatfsFreeBlocks,
        CapacityBasis::StatfsAvailableBlocks,
        CapacityBasis::CoreFoundationImportantUsage,
        CapacityBasis::CoreFoundationOpportunisticUsage,
    ]
}

fn unmeasurable_reasons() -> [UnmeasurableReason; 11] {
    [
        UnmeasurableReason::NotNormalizedAbsolute,
        UnmeasurableReason::CloudRootExcluded,
        UnmeasurableReason::ReadPolicyVerificationFailed,
        UnmeasurableReason::RootPathUnresolvable,
        UnmeasurableReason::RootPathNotEncodable,
        UnmeasurableReason::RootIdentityUnavailable,
        UnmeasurableReason::SymlinkTraversalRejected,
        UnmeasurableReason::VolumeCapacityQueryFailed,
        UnmeasurableReason::SharedContainerCapabilityUnavailable,
        UnmeasurableReason::CapacityArithmeticOverflowed,
        UnmeasurableReason::CoreFoundationCapacityUnavailable,
    ]
}

fn observation_kinds() -> [ObservationKind; 2] {
    [ObservationKind::Direct, ObservationKind::Derived]
}

fn observation_relations() -> [ObservationRelation; 5] {
    [
        ObservationRelation::TestedWidthCorrelate,
        ObservationRelation::PossibleWidthExplanation,
        ObservationRelation::LogicalAllocationGap,
        ObservationRelation::ReclaimPolicy,
        ObservationRelation::DeletionScope,
    ]
}

fn observation_scopes() -> [ObservationScope; 3] {
    [
        ObservationScope::Object,
        ObservationScope::Inode,
        ObservationScope::Volume,
    ]
}

fn signal_ids() -> [SignalId; 7] {
    [
        SignalId::FilesystemCompressed,
        SignalId::ResourceForkAllocated,
        SignalId::MayShareBlocks,
        SignalId::VolumeHasSnapshots,
        SignalId::Sparse,
        SignalId::Purgeable,
        SignalId::MultipleHardlinks,
    ]
}

fn disposition_actions() -> [DispositionAction; 1] {
    [DispositionAction::PermanentUnlinkAfterReferencesClose]
}
