#![allow(clippy::disallowed_methods)]

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use sizetrail::model::{
    Measurement, MeasurementBasis, MeasurementCoverage, MeasurementCoverageStatus,
    MeasurementPlane, MeasurementScope, MeasurementScopeKind, MeasurementValue,
};

#[test]
fn adapter_free_scan_emits_a_complete_json_document() {
    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--root", "/fixture/home"])
        .output()
        .expect("scan must run");

    assert!(output.status.success());
    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout must be JSON");

    assert_eq!(document["schema_version"], "0.1.0-unstable");
    assert!(document["environment"].is_object());
    assert!(document["payload"].is_object());
    assert_eq!(document["payload"]["regions"], serde_json::json!([]));
    assert_eq!(document["payload"]["findings"], serde_json::json!([]));
    assert_eq!(
        document["payload"]["coverage_gaps"][0]["reason"],
        "no_adapters_compiled"
    );
}

#[test]
fn measurement_schema_makes_basis_scope_coverage_and_uncertainty_explicit() {
    let measurement = Measurement {
        plane: MeasurementPlane::ToolchainAttribution,
        basis: MeasurementBasis::AllocatedFootprint,
        scope: MeasurementScope {
            kind: MeasurementScopeKind::ToolchainStore,
            id: "fixture.store".to_owned(),
        },
        coverage: MeasurementCoverage {
            status: MeasurementCoverageStatus::Complete,
            gap_ids: Vec::new(),
        },
        value: MeasurementValue::IntervalBytes {
            floor_bytes: 0,
            ceiling_bytes: Some(1024),
        },
    };
    let serialized = serde_json::to_value(measurement).expect("measurement must serialize");

    assert_eq!(serialized["basis"], "allocated_footprint");
    assert_eq!(serialized["scope"]["kind"], "toolchain_store");
    assert_eq!(serialized["coverage"]["status"], "complete");
    assert_eq!(serialized["value"]["kind"], "interval_bytes");
}
