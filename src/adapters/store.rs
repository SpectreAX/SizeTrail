use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::fsx::{ObjectMeasurements, Root, RootEntryKind};
use crate::model::{
    DispositionAction, ExtentKind, ExtentObservation, Measurement, MeasurementBasis,
    MeasurementCoverage, MeasurementCoverageStatus, MeasurementPlane, MeasurementQuantity,
    MeasurementScope, MeasurementScopeKind, MeasurementValue, ObservationKind, ObservationRelation,
    ObservationScope, SignalId, SignalObservation, StorageSignal, estimate_disposition,
    normalized_report_path,
};

pub(crate) fn expand_home_pattern(
    root: &Root,
    pattern: &str,
    excludes: &[PathBuf],
) -> Result<Vec<PathBuf>, (PathBuf, io::Error)> {
    let Some(relative) = pattern.strip_prefix("~/") else {
        return Ok(Vec::new());
    };
    expand_pattern(root, root.path(), relative, excludes)
}

pub(crate) fn expand_pattern(
    root: &Root,
    start: &Path,
    relative_pattern: &str,
    excludes: &[PathBuf],
) -> Result<Vec<PathBuf>, (PathBuf, io::Error)> {
    let mut current = vec![start.to_path_buf()];
    for component in relative_pattern.split('/') {
        let mut next = Vec::new();
        for parent in current {
            if excluded(&parent, excludes) {
                continue;
            }
            let children = match root.children(&parent) {
                Ok(children) => children,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err((parent, error)),
            };
            for child in children {
                if excluded(&child.path, excludes) {
                    continue;
                }
                let wildcard = component == "*";
                let matches =
                    wildcard || child.path.file_name().is_some_and(|name| name == component);
                if matches {
                    if child.kind != RootEntryKind::Directory {
                        if wildcard {
                            continue;
                        }
                        return Err((
                            child.path,
                            io::Error::other("rule path matched an unsupported entry kind"),
                        ));
                    }
                    next.push(child.path);
                }
            }
        }
        current = next;
    }
    Ok(current)
}

pub(crate) fn measure_store(
    root: &Root,
    path: &Path,
    excludes: &[PathBuf],
    volume_has_snapshots: bool,
) -> io::Result<(Vec<Measurement>, Vec<SignalObservation>)> {
    let scope = normalized_report_path(root.path(), path)
        .map_err(|_| io::Error::other("store path is not normalized"))?;
    measure_store_as(root, path, excludes, volume_has_snapshots, scope)
}

pub(crate) fn measure_store_as(
    root: &Root,
    path: &Path,
    excludes: &[PathBuf],
    volume_has_snapshots: bool,
    scope: String,
) -> io::Result<(Vec<Measurement>, Vec<SignalObservation>)> {
    let mut stack = vec![path.to_path_buf()];
    let mut objects = BTreeMap::<_, (ObjectMeasurements, u64)>::new();
    while let Some(directory) = stack.pop() {
        if excluded(&directory, excludes) {
            continue;
        }
        for entry in root.children(&directory)? {
            if excluded(&entry.path, excludes) {
                continue;
            }
            match entry.kind {
                RootEntryKind::Directory => stack.push(entry.path),
                RootEntryKind::File | RootEntryKind::Symlink | RootEntryKind::Other => {
                    let measured = root.measure_object(&entry.path)?;
                    let value = objects.entry(measured.identity).or_insert((measured, 0));
                    value.1 = value
                        .1
                        .checked_add(1)
                        .ok_or_else(|| io::Error::other("hardlink count overflow"))?;
                }
            }
        }
    }

    let mut logical_bytes = 0_u64;
    let mut allocated_bytes = Some(0_u64);
    let mut extents = Vec::new();
    let mut observations = Vec::new();
    if volume_has_snapshots {
        observations.push(observation(
            SignalId::VolumeHasSnapshots,
            ObservationRelation::PossibleWidthExplanation,
            ObservationScope::Volume,
        ));
    }
    for (measured, covered_link_count) in objects.values() {
        logical_bytes = logical_bytes
            .checked_add(measured.logical_bytes)
            .ok_or_else(|| io::Error::other("logical size overflow"))?;
        allocated_bytes = match (allocated_bytes, measured.allocated_bytes) {
            (Some(total), Some(bytes)) => total.checked_add(bytes),
            _ => None,
        };
        observations.extend(object_observations(measured));
        extents.push(ExtentObservation {
            identity: measured.identity,
            kind: ExtentKind::FileForks,
            link_count: measured.link_count,
            covered_link_count: *covered_link_count,
            allocated_bytes: measured.allocated_bytes,
            private_bytes: measured.private_bytes,
            signals: Vec::<StorageSignal>::new(),
        });
    }
    let estimate = estimate_disposition(&extents, false)
        .map_err(|_| io::Error::other("disposition estimate overflow"))?;
    observations.sort();
    observations.dedup();
    let measured_coverage = MeasurementCoverage {
        status: MeasurementCoverageStatus::Complete,
        gap_ids: Vec::new(),
    };
    let interval_coverage = MeasurementCoverage {
        status: if estimate.has_unmeasurable_objects {
            MeasurementCoverageStatus::Partial
        } else {
            MeasurementCoverageStatus::Complete
        },
        gap_ids: if estimate.has_unmeasurable_objects {
            vec!["private_or_allocated_size_unmeasurable".to_owned()]
        } else {
            Vec::new()
        },
    };

    Ok((
        vec![
            Measurement {
                plane: MeasurementPlane::ToolchainAttribution,
                quantity: MeasurementQuantity::LogicalSize,
                basis: MeasurementBasis::LogicalSize,
                scope: MeasurementScope {
                    kind: MeasurementScopeKind::ToolchainStore,
                    id: scope.clone(),
                },
                coverage: measured_coverage,
                value: MeasurementValue::ExactBytes {
                    bytes: logical_bytes,
                },
            },
            Measurement {
                plane: MeasurementPlane::ToolchainAttribution,
                quantity: MeasurementQuantity::AllocatedFootprint,
                basis: MeasurementBasis::AllocatedFootprint,
                scope: MeasurementScope {
                    kind: MeasurementScopeKind::ToolchainStore,
                    id: scope.clone(),
                },
                coverage: MeasurementCoverage {
                    status: if allocated_bytes.is_some() {
                        MeasurementCoverageStatus::Complete
                    } else {
                        MeasurementCoverageStatus::Unmeasurable
                    },
                    gap_ids: if allocated_bytes.is_some() {
                        Vec::new()
                    } else {
                        vec!["allocated_size_unmeasurable".to_owned()]
                    },
                },
                value: allocated_bytes.map_or(MeasurementValue::Unmeasurable, |bytes| {
                    MeasurementValue::ExactBytes { bytes }
                }),
            },
            Measurement {
                plane: MeasurementPlane::DispositionEstimate,
                quantity: MeasurementQuantity::DispositionEstimate,
                basis: MeasurementBasis::PrivateFloorAllocatedCeiling,
                scope: MeasurementScope {
                    kind: MeasurementScopeKind::ObjectSet,
                    id: scope,
                },
                coverage: interval_coverage,
                value: MeasurementValue::IntervalBytes {
                    floor_bytes: estimate.floor_bytes,
                    ceiling_bytes: estimate.ceiling_bytes,
                    applicable_action: DispositionAction::PermanentUnlinkAfterReferencesClose,
                },
            },
        ],
        observations,
    ))
}

pub(crate) fn object_observations(measured: &ObjectMeasurements) -> Vec<SignalObservation> {
    let mut observations = Vec::new();
    let extended = measured.extended_flags.unwrap_or_default();
    if extended & 0x0000_0001 != 0 {
        observations.push(observation(
            SignalId::MayShareBlocks,
            ObservationRelation::PossibleWidthExplanation,
            ObservationScope::Inode,
        ));
    }
    if measured.resource_fork_allocated_bytes.unwrap_or_default() > 0 {
        observations.push(observation(
            SignalId::ResourceForkAllocated,
            ObservationRelation::TestedWidthCorrelate,
            ObservationScope::Object,
        ));
    }
    if measured.bsd_flags & 0x0000_0020 != 0 {
        observations.push(observation(
            SignalId::FilesystemCompressed,
            ObservationRelation::TestedWidthCorrelate,
            ObservationScope::Object,
        ));
    }
    if extended & 0x0000_0010 != 0 {
        observations.push(observation(
            SignalId::Sparse,
            ObservationRelation::LogicalAllocationGap,
            ObservationScope::Object,
        ));
    }
    if extended & 0x0000_0008 != 0 {
        observations.push(observation(
            SignalId::Purgeable,
            ObservationRelation::ReclaimPolicy,
            ObservationScope::Object,
        ));
    }
    if measured.link_count > 1 {
        observations.push(observation(
            SignalId::MultipleHardlinks,
            ObservationRelation::DeletionScope,
            ObservationScope::Inode,
        ));
    }
    observations
}

const fn observation(
    signal: SignalId,
    relation: ObservationRelation,
    scope: ObservationScope,
) -> SignalObservation {
    SignalObservation {
        observation: ObservationKind::Direct,
        signal,
        relation,
        scope,
    }
}

pub(crate) fn excluded(path: &Path, excludes: &[PathBuf]) -> bool {
    excludes.iter().any(|excluded| path.starts_with(excluded))
}
