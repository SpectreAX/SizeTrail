#![allow(clippy::disallowed_methods)]

use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use sizetrail::capacity::CapacityReport;
use sizetrail::model::{
    CoverageGap, CoverageGapReason, DispositionAction, EnvironmentEnvelope, Measurement,
    MeasurementBasis, MeasurementCoverage, MeasurementCoverageStatus, MeasurementPlane,
    MeasurementQuantity, MeasurementScope, MeasurementScopeKind, MeasurementValue, RegionStatus,
};
use sizetrail::scan::{AdapterReport, scan};

#[test]
fn explicitly_excluded_adapter_scan_emits_a_complete_json_document() {
    let fixture = tempfile::tempdir().expect("scan root must be created");
    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--no-xcode", "--root"])
        .arg(fixture.path())
        .output()
        .expect("scan must run");

    assert_eq!(output.status.code(), Some(0));
    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout must be JSON");

    assert_eq!(document["schema_version"], "0.1.0-unstable");
    assert!(document["environment"].is_object());
    assert!(document["payload"].is_object());
    assert_eq!(document["payload"]["regions"][0]["id"], "capacity");
    assert!(document["payload"]["capacity"].is_array());
    assert_eq!(document["payload"]["findings"], serde_json::json!([]));
    let xcode = document["payload"]["regions"]
        .as_array()
        .expect("regions must be an array")
        .iter()
        .find(|region| region["id"] == "xcode")
        .expect("xcode region must be present");
    assert_eq!(xcode["status"], "excluded_by_user");
}

#[test]
fn adapter_arrival_order_does_not_change_the_payload() {
    fn report(id: &str) -> AdapterReport {
        AdapterReport {
            id: id.to_owned(),
            status: RegionStatus::NotPresent,
            tool_version: None,
            warnings: Vec::new(),
            findings: Vec::new(),
            coverage_gaps: Vec::new(),
        }
    }
    let environment = EnvironmentEnvelope {
        generated_at_unix_seconds: 1_800_000_000,
        hostname: "fixture-host".to_owned(),
        home: "/Users/fixture".to_owned(),
        tool_versions: Default::default(),
    };
    let capacity = CapacityReport {
        status: RegionStatus::Complete,
        values: Vec::new(),
    };
    let first = scan(
        environment,
        CapacityReport {
            status: capacity.status,
            values: Vec::new(),
        },
        vec![report("z-last"), report("a-first")],
    );
    let second = scan(
        EnvironmentEnvelope {
            generated_at_unix_seconds: 1_800_000_000,
            hostname: "fixture-host".to_owned(),
            home: "/Users/fixture".to_owned(),
            tool_versions: Default::default(),
        },
        capacity,
        vec![report("a-first"), report("z-last")],
    );

    assert_eq!(
        serde_json::to_vec(&first.payload).expect("payload must serialize"),
        serde_json::to_vec(&second.payload).expect("payload must serialize")
    );
}

#[test]
fn adapter_failure_and_user_exclusion_remain_distinct_document_states() {
    let failed = AdapterReport {
        id: "xcode".to_owned(),
        status: RegionStatus::Unmeasurable,
        tool_version: Some("Fixture".to_owned()),
        warnings: vec!["fixture warning".to_owned()],
        findings: Vec::new(),
        coverage_gaps: vec![CoverageGap {
            id: "xcode.simctl.timeout".to_owned(),
            plane: MeasurementPlane::ToolchainAttribution,
            region: "xcode".to_owned(),
            path: None,
            status: RegionStatus::Unmeasurable,
            reason: CoverageGapReason::TimedOut,
            stage: Some("simctl_devices".to_owned()),
            errno: None,
        }],
    };
    let excluded = AdapterReport {
        id: "homebrew".to_owned(),
        status: RegionStatus::ExcludedByUser,
        tool_version: None,
        warnings: Vec::new(),
        findings: Vec::new(),
        coverage_gaps: Vec::new(),
    };
    let document = scan(
        EnvironmentEnvelope {
            generated_at_unix_seconds: 1_800_000_000,
            hostname: "fixture-host".to_owned(),
            home: "/Users/fixture".to_owned(),
            tool_versions: Default::default(),
        },
        CapacityReport {
            status: RegionStatus::Complete,
            values: Vec::new(),
        },
        vec![excluded, failed],
    );

    let json = serde_json::to_value(&document).expect("document must serialize");
    assert_eq!(json["payload"]["regions"][1]["id"], "homebrew");
    assert_eq!(json["payload"]["regions"][1]["status"], "excluded_by_user");
    assert_eq!(json["payload"]["regions"][2]["id"], "xcode");
    assert_eq!(json["payload"]["coverage_gaps"][0]["reason"], "timed_out");
}

#[test]
fn every_measured_capacity_number_carries_its_basis() {
    let fixture = tempfile::tempdir().expect("scan root must be created");
    let output = cargo_bin_cmd!("sizetrail")
        .args(["scan", "--json", "--root"])
        .arg(fixture.path())
        .output()
        .expect("scan must run");
    let document: Value = serde_json::from_slice(&output.stdout).expect("stdout must be JSON");

    for value in document["payload"]["capacity"]
        .as_array()
        .expect("capacity must be an array")
    {
        if value["status"] == "measured" {
            assert!(value["basis"].is_string());
            assert!(value["bytes"].is_u64());
        }
    }
}

#[test]
fn measurement_schema_makes_basis_scope_coverage_and_uncertainty_explicit() {
    let measurement = Measurement {
        plane: MeasurementPlane::ToolchainAttribution,
        quantity: MeasurementQuantity::AllocatedFootprint,
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
            applicable_action: DispositionAction::PermanentUnlinkAfterReferencesClose,
        },
    };
    let serialized = serde_json::to_value(measurement).expect("measurement must serialize");

    assert_eq!(serialized["basis"], "allocated_footprint");
    assert_eq!(serialized["quantity"], "allocated_footprint");
    assert_eq!(serialized["scope"]["kind"], "toolchain_store");
    assert_eq!(serialized["coverage"]["status"], "complete");
    assert_eq!(serialized["value"]["kind"], "interval_bytes");
    assert_eq!(
        serialized["value"]["applicable_action"],
        "permanent_unlink_after_references_close"
    );
}

#[test]
fn vendor_human_sizes_are_typed_as_rounded_ranges_not_exact_bytes() {
    let measurement = Measurement {
        plane: MeasurementPlane::ToolchainAttribution,
        quantity: MeasurementQuantity::DaemonReclaimable,
        basis: MeasurementBasis::DockerSystemDf,
        scope: MeasurementScope {
            kind: MeasurementScopeKind::ToolchainStore,
            id: "docker.images".to_owned(),
        },
        coverage: MeasurementCoverage {
            status: MeasurementCoverageStatus::Complete,
            gap_ids: Vec::new(),
        },
        value: sizetrail::model::rounded_bytes("2.498GB (94%)")
            .expect("verified Docker formatter output must parse"),
    };
    let serialized = serde_json::to_value(measurement).expect("measurement must serialize");

    assert_eq!(serialized["quantity"], "daemon_reclaimable");
    assert_eq!(serialized["basis"], "docker_system_df");
    assert_eq!(serialized["value"]["kind"], "rounded_bytes");
    assert_eq!(serialized["value"]["reported"], "2.498GB (94%)");
    assert!(
        serialized["value"]["lower_bound_bytes"]
            .as_u64()
            .expect("lower bound")
            < 2_498_000_000
    );
    assert!(
        serialized["value"]["upper_bound_bytes"]
            .as_u64()
            .expect("upper bound")
            > 2_498_000_000
    );
    assert!(serialized["value"].get("bytes").is_none());

    assert_eq!(
        serde_json::to_value(sizetrail::model::rounded_bytes("158B").expect("byte value"))
            .expect("value must serialize")["lower_bound_bytes"],
        158
    );
    for invalid in ["", "-1GB", "1.2.3GB", "1XB", "2GB garbage", "2GB (%)"] {
        assert!(
            sizetrail::model::rounded_bytes(invalid).is_err(),
            "accepted invalid vendor size {invalid:?}"
        );
    }
}
