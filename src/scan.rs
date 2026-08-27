use crate::model::{
    CoverageGap, CoverageGapReason, EnvironmentEnvelope, MeasurementPlane, RegionStatus,
    SCHEMA_VERSION, ScanDocument, ScanPayload,
};

pub fn scan(environment: EnvironmentEnvelope) -> ScanDocument {
    ScanDocument {
        schema_version: SCHEMA_VERSION,
        environment,
        payload: ScanPayload {
            regions: Vec::new(),
            findings: Vec::new(),
            coverage_gaps: vec![CoverageGap {
                id: "p1.toolchain_adapters",
                plane: MeasurementPlane::ToolchainAttribution,
                region: "toolchain_adapters",
                status: RegionStatus::Unmeasurable,
                reason: CoverageGapReason::NoAdaptersCompiled,
            }],
        },
    }
}
