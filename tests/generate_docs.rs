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
    );
    let rendered = serde_json::to_string_pretty(&document).expect("document must serialize");
    let output = Path::new("docs/generated/empty-scan.json");

    fs::create_dir_all(output.parent().expect("generated document has a parent"))
        .expect("generated directory must exist");
    fs::write(output, format!("{rendered}\n")).expect("generated document must be written");
}
