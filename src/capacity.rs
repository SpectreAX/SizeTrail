use std::path::Path;

use crate::fsx::{CapacityKind, CapacityValue, Root, UnmeasurableReason};
use crate::model::RegionStatus;

#[derive(Debug)]
pub struct CapacityReport {
    pub status: RegionStatus,
    pub values: Vec<CapacityValue>,
}

pub fn measure(root: &Path) -> CapacityReport {
    let root = match Root::open(root) {
        Ok(root) => root,
        Err(error) => return unknown_report(error.reason()),
    };
    measure_root(&root)
}

pub fn measure_root(root: &Root) -> CapacityReport {
    match root.capacity() {
        Ok(values) => CapacityReport {
            status: if values
                .iter()
                .any(|value| matches!(value, CapacityValue::Unmeasurable { .. }))
            {
                RegionStatus::Unmeasurable
            } else {
                RegionStatus::Complete
            },
            values,
        },
        Err(_) => unknown_report(UnmeasurableReason::VolumeCapacityQueryFailed),
    }
}

fn unknown_report(reason: UnmeasurableReason) -> CapacityReport {
    CapacityReport {
        status: RegionStatus::Unmeasurable,
        values: [
            CapacityKind::ContainerAllocated,
            CapacityKind::VolumeSize,
            CapacityKind::VolumeUsed,
            CapacityKind::VolumeFree,
            CapacityKind::AvailableNormal,
            CapacityKind::AvailableImportant,
            CapacityKind::AvailableOpportunistic,
        ]
        .into_iter()
        .map(|kind| CapacityValue::Unmeasurable { kind, reason })
        .collect(),
    }
}
