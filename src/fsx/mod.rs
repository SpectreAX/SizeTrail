mod sys;

use std::ffi::CString;
use std::io;
use std::os::macos::fs::MetadataExt as MacMetadataExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use crate::model::FileIdentity;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityKind {
    ContainerAllocated,
    VolumeSize,
    VolumeUsed,
    VolumeFree,
    AvailableNormal,
    AvailableImportant,
    AvailableOpportunistic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityBasis {
    AttrVolSize,
    AttrVolSpaceUsed,
    AttrVolSpaceFree,
    AttrVolSpaceAvailable,
    StatfsBlocks,
    StatfsBlocksMinusFree,
    StatfsFreeBlocks,
    StatfsAvailableBlocks,
    CoreFoundationImportantUsage,
    CoreFoundationOpportunisticUsage,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CapacityValue {
    Measured {
        kind: CapacityKind,
        bytes: u64,
        basis: CapacityBasis,
    },
    Unmeasurable {
        kind: CapacityKind,
        reason: &'static str,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectMeasurements {
    pub identity: FileIdentity,
    pub logical_bytes: u64,
    pub link_count: u64,
    pub allocated_bytes: Option<u64>,
    pub data_allocated_bytes: Option<u64>,
    pub resource_fork_allocated_bytes: Option<u64>,
    pub private_bytes: Option<u64>,
    pub extended_flags: Option<u64>,
    pub bsd_flags: u32,
    pub dataless: bool,
}

#[derive(Debug)]
pub struct Root {
    path: PathBuf,
    path_c: CString,
    identity: FileIdentity,
    volume: Option<sys::VolumeRaw>,
}

impl Root {
    pub fn open(path: &Path) -> io::Result<Self> {
        require_normalized_absolute(path)?;
        reject_cloud_root(path)?;
        initialize_root(sys::install_read_policies, || {
            let path_c = path_to_c_string(path)?;
            Ok((
                path_c.clone(),
                sys::volume(&path_c).ok(),
                sys::identity(&path_c)?,
            ))
        })
        .and_then(|(path_c, volume, raw)| {
            let identity = identity(&raw)?;
            Ok(Self {
                path: path.to_path_buf(),
                path_c,
                identity,
                volume,
            })
        })
    }

    #[must_use]
    pub const fn identity(&self) -> FileIdentity {
        self.identity
    }

    pub fn measure_object(&self, path: &Path) -> io::Result<ObjectMeasurements> {
        require_normalized_absolute(path)?;
        if !path.starts_with(&self.path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object is outside the initialized root",
            ));
        }
        let path_c = path_to_c_string(path)?;
        let raw = sys::object(&path_c)?;
        let identity = object_identity(&raw)?;
        enforce_mount_boundary(self.identity, identity)?;
        let metadata = std::fs::symlink_metadata(path)?;
        let dataless = metadata.st_flags() & 0x4000_0000 != 0;
        let valid = self
            .volume
            .map_or_else(sys::AttributeSet::default, |volume| volume.valid_attributes);
        let returned = raw.returned;

        Ok(ObjectMeasurements {
            identity,
            logical_bytes: metadata.len(),
            link_count: metadata.nlink(),
            allocated_bytes: (!dataless
                && present(returned.file, valid.file, sys::bits::FILE_ALLOCATED))
            .then_some(raw.allocated),
            data_allocated_bytes: (!dataless
                && present(returned.file, valid.file, sys::bits::FILE_DATA_ALLOCATED))
            .then_some(raw.data_allocated),
            resource_fork_allocated_bytes: (!dataless
                && present(
                    returned.file,
                    valid.file,
                    sys::bits::FILE_RESOURCE_ALLOCATED,
                ))
            .then_some(raw.resource_allocated),
            private_bytes: (!dataless
                && present(returned.fork, valid.fork, sys::bits::FORK_PRIVATE))
            .then_some(raw.private),
            extended_flags: present(returned.fork, valid.fork, sys::bits::FORK_EXTENDED_FLAGS)
                .then_some(raw.extended_flags),
            bsd_flags: metadata.st_flags(),
            dataless,
        })
    }

    pub fn capacity(&self) -> io::Result<Vec<CapacityValue>> {
        let fallback = sys::filesystem(&self.path_c)?;
        let important = sys::important_capacity(&self.path_c);
        let opportunistic = sys::opportunistic_capacity(&self.path_c);
        Ok(capacity_values(
            self.volume,
            fallback,
            important,
            opportunistic,
        ))
    }
}

fn capacity_values(
    volume: Option<sys::VolumeRaw>,
    fallback: sys::StatFsRaw,
    important: io::Result<u64>,
    opportunistic: io::Result<u64>,
) -> Vec<CapacityValue> {
    let returned = volume.map_or(0, |value| value.returned.volume);
    let valid = volume.map_or(0, |value| value.valid_attributes.volume);
    let block_bytes = |blocks: u64| fallback.block_size.checked_mul(blocks);
    let measured_or = |kind, bit, primary: Option<u64>, primary_basis, fallback, fallback_basis| {
        if present(returned, valid, bit)
            && let Some(bytes) = primary
        {
            CapacityValue::Measured {
                kind,
                bytes,
                basis: primary_basis,
            }
        } else if let Some(bytes) = fallback {
            CapacityValue::Measured {
                kind,
                bytes,
                basis: fallback_basis,
            }
        } else {
            CapacityValue::Unmeasurable {
                kind,
                reason: "capacity arithmetic overflowed",
            }
        }
    };

    vec![
        if volume.is_some_and(|value| {
            value.valid_capabilities[0] & 0x0080_0000 != 0
                && value.capabilities[0] & 0x0080_0000 != 0
        }) {
            fallback
                .blocks
                .checked_sub(fallback.blocks_free)
                .and_then(block_bytes)
                .map_or(
                    CapacityValue::Unmeasurable {
                        kind: CapacityKind::ContainerAllocated,
                        reason: "statfs container arithmetic overflowed",
                    },
                    |bytes| CapacityValue::Measured {
                        kind: CapacityKind::ContainerAllocated,
                        bytes,
                        basis: CapacityBasis::StatfsBlocksMinusFree,
                    },
                )
        } else {
            CapacityValue::Unmeasurable {
                kind: CapacityKind::ContainerAllocated,
                reason: "volume does not report valid shared-container capability",
            }
        },
        measured_or(
            CapacityKind::VolumeSize,
            sys::bits::VOLUME_SIZE,
            volume.map(|value| value.size),
            CapacityBasis::AttrVolSize,
            block_bytes(fallback.blocks),
            CapacityBasis::StatfsBlocks,
        ),
        measured_or(
            CapacityKind::VolumeUsed,
            sys::bits::VOLUME_USED,
            volume.map(|value| value.used),
            CapacityBasis::AttrVolSpaceUsed,
            fallback
                .blocks
                .checked_sub(fallback.blocks_free)
                .and_then(block_bytes),
            CapacityBasis::StatfsBlocksMinusFree,
        ),
        measured_or(
            CapacityKind::VolumeFree,
            sys::bits::VOLUME_FREE,
            volume.map(|value| value.free),
            CapacityBasis::AttrVolSpaceFree,
            block_bytes(fallback.blocks_free),
            CapacityBasis::StatfsFreeBlocks,
        ),
        measured_or(
            CapacityKind::AvailableNormal,
            sys::bits::VOLUME_AVAILABLE,
            volume.map(|value| value.available),
            CapacityBasis::AttrVolSpaceAvailable,
            block_bytes(fallback.blocks_available),
            CapacityBasis::StatfsAvailableBlocks,
        ),
        important.map_or(
            CapacityValue::Unmeasurable {
                kind: CapacityKind::AvailableImportant,
                reason: "CoreFoundation important-usage capacity was unavailable",
            },
            |bytes| CapacityValue::Measured {
                kind: CapacityKind::AvailableImportant,
                bytes,
                basis: CapacityBasis::CoreFoundationImportantUsage,
            },
        ),
        opportunistic.map_or(
            CapacityValue::Unmeasurable {
                kind: CapacityKind::AvailableOpportunistic,
                reason: "CoreFoundation opportunistic-usage capacity was unavailable",
            },
            |bytes| CapacityValue::Measured {
                kind: CapacityKind::AvailableOpportunistic,
                bytes,
                basis: CapacityBasis::CoreFoundationOpportunisticUsage,
            },
        ),
    ]
}

fn initialize_root<T>(
    install_policies: impl FnOnce() -> io::Result<()>,
    probe: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    install_policies()?;
    probe()
}

fn enforce_mount_boundary(root: FileIdentity, object: FileIdentity) -> io::Result<()> {
    if object.fsid == root.fsid {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::CrossesDevices,
            "object crosses a physical mount or firmlink boundary",
        ))
    }
}

fn identity(raw: &sys::IdentityRaw) -> io::Result<FileIdentity> {
    if raw.returned.common & (sys::bits::COMMON_FSID | sys::bits::COMMON_FILEID)
        != sys::bits::COMMON_FSID | sys::bits::COMMON_FILEID
        || raw.returned.fork & sys::bits::FORK_REAL_FSID == 0
    {
        return Err(io::Error::other(
            "filesystem did not return physical fsid and fileid",
        ));
    }
    Ok(FileIdentity {
        fsid: raw.real_fsid,
        fileid: raw.fileid,
    })
}

fn object_identity(raw: &sys::ObjectRaw) -> io::Result<FileIdentity> {
    if raw.returned.common & (sys::bits::COMMON_FSID | sys::bits::COMMON_FILEID)
        != sys::bits::COMMON_FSID | sys::bits::COMMON_FILEID
        || raw.returned.fork & sys::bits::FORK_REAL_FSID == 0
    {
        return Err(io::Error::other(
            "filesystem did not return physical fsid and fileid",
        ));
    }
    Ok(FileIdentity {
        fsid: raw.real_fsid,
        fileid: raw.fileid,
    })
}

const fn present(returned: u32, valid: u32, bit: u32) -> bool {
    returned & bit != 0 && valid & bit != 0
}

fn path_to_c_string(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))
}

fn require_normalized_absolute(path: &Path) -> io::Result<()> {
    if path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "root and object paths must be normalized absolute paths",
        ))
    }
}

fn reject_cloud_root(path: &Path) -> io::Result<()> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Ok(());
    };
    for cloud in [
        home.join("Library/Mobile Documents"),
        home.join("Library/CloudStorage"),
    ] {
        if path.starts_with(cloud) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cloud and File Provider roots are permanently excluded",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io;

    use super::{
        CapacityBasis, CapacityKind, CapacityValue, FileIdentity, capacity_values,
        enforce_mount_boundary, initialize_root, sys,
    };

    #[test]
    fn failed_read_policy_verification_prevents_all_root_probes() {
        let probed = Cell::new(false);
        let result = initialize_root(
            || Err(io::Error::other("policy verification failed")),
            || {
                probed.set(true);
                Ok(())
            },
        );

        assert!(result.is_err());
        assert!(!probed.get());
    }

    #[test]
    fn real_fsid_change_rejects_mount_and_firmlink_boundaries() {
        let root = FileIdentity {
            fsid: [1, 2],
            fileid: 10,
        };
        let same_volume = FileIdentity {
            fsid: [1, 2],
            fileid: 11,
        };
        let data_volume = FileIdentity {
            fsid: [3, 4],
            fileid: 11,
        };

        assert!(enforce_mount_boundary(root, same_volume).is_ok());
        assert_eq!(
            enforce_mount_boundary(root, data_volume)
                .expect_err("physical volume change must be rejected")
                .kind(),
            io::ErrorKind::CrossesDevices
        );
    }

    #[test]
    fn a_failed_volume_attr_query_uses_only_explicit_statfs_fallbacks() {
        let values = capacity_values(
            None,
            sys::StatFsRaw {
                block_size: 4096,
                blocks: 100,
                blocks_free: 40,
                blocks_available: 30,
            },
            Err(io::Error::other("important unavailable")),
            Err(io::Error::other("opportunistic unavailable")),
        );

        assert!(values.iter().any(|value| matches!(
            value,
            CapacityValue::Measured {
                kind: CapacityKind::VolumeUsed,
                bytes: 245_760,
                basis: CapacityBasis::StatfsBlocksMinusFree,
            }
        )));
        assert!(values.iter().any(|value| matches!(
            value,
            CapacityValue::Unmeasurable {
                kind: CapacityKind::ContainerAllocated,
                ..
            }
        )));
    }
}
