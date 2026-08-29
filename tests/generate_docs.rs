#![allow(clippy::disallowed_methods)]

use std::fs;
use std::path::Path;

use sizetrail::capacity::CapacityReport;
use sizetrail::fsx::{CapacityBasis, CapacityKind, CapacityValue};
use sizetrail::model::EnvironmentEnvelope;
use sizetrail::model::RegionStatus;
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
}
