use crate::capacity::CapacityReport;
use crate::model::{
    CoverageGap, CoverageGapReason, EnvironmentEnvelope, MeasurementPlane, RegionStatus,
    SCHEMA_VERSION, ScanDocument, ScanPayload,
};

pub fn scan(environment: EnvironmentEnvelope, capacity: CapacityReport) -> ScanDocument {
    let mut coverage_gaps = vec![CoverageGap {
        id: "p1.toolchain_adapters",
        plane: MeasurementPlane::ToolchainAttribution,
        region: "toolchain_adapters",
        status: RegionStatus::Unmeasurable,
        reason: CoverageGapReason::NoAdaptersCompiled,
    }];
    if capacity.status == RegionStatus::Unmeasurable {
        coverage_gaps.push(CoverageGap {
            id: "p2.capacity",
            plane: MeasurementPlane::Capacity,
            region: "capacity",
            status: RegionStatus::Unmeasurable,
            reason: CoverageGapReason::RootUnmeasurable,
        });
    }
    ScanDocument {
        schema_version: SCHEMA_VERSION,
        environment,
        payload: ScanPayload {
            capacity: capacity.values,
            regions: vec![crate::model::RegionReport {
                id: "capacity".to_owned(),
                status: capacity.status,
            }],
            findings: Vec::new(),
            coverage_gaps,
        },
    }
}
