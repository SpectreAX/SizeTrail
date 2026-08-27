use std::path::Path;

use crate::fsx::{CapacityKind, CapacityValue, Root};
use crate::model::RegionStatus;

#[derive(Debug)]
pub struct CapacityReport {
    pub status: RegionStatus,
    pub values: Vec<CapacityValue>,
}

pub fn measure(root: &Path) -> CapacityReport {
    let Ok(root) = Root::open(root) else {
        return unknown_report("root initialization or read-policy verification failed");
    };
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
        Err(_) => unknown_report("volume capacity query failed"),
    }
}

fn unknown_report(reason: &'static str) -> CapacityReport {
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
