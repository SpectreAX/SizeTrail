mod sys;

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::model::FileIdentity;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityKind {
    ContainerAllocated,
    VolumeSize,
    VolumeUsed,
    VolumeFree,
    AvailableNormal,
    AvailableImportant,
    AvailableOpportunistic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityBasis {
    AttrVolSize,
    AttrVolSpaceUsed,
    AttrVolSpaceFree,
    AttrVolSpaceAvailable,
    StatfsBlocks,
    StatfsBlocksMinusFree,
    StatfsFreeBlocks,
    StatfsAvailableBlocks,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
}

#[derive(Debug)]
pub struct Root {
    path: PathBuf,
    path_c: CString,
    identity: FileIdentity,
    volume: sys::VolumeRaw,
}

impl Root {
    pub fn open(path: &Path) -> io::Result<Self> {
        reject_cloud_root(path)?;
        sys::install_read_policies()?;
        let path_c = path_to_c_string(path)?;
        let volume = sys::volume(&path_c)?;
        let raw = sys::identity(&path_c)?;
        let identity = identity(&raw)?;
        Ok(Self {
            path: path.to_path_buf(),
            path_c,
            identity,
            volume,
        })
    }

    #[must_use]
    pub const fn identity(&self) -> FileIdentity {
        self.identity
    }

    pub fn measure_object(&self, path: &Path) -> io::Result<ObjectMeasurements> {
        if !path.starts_with(&self.path) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "object is outside the initialized root",
            ));
        }
        let path_c = path_to_c_string(path)?;
        let raw = sys::object(&path_c)?;
        let identity = object_identity(&raw)?;
        if identity.fsid != self.identity.fsid {
            return Err(io::Error::new(
                io::ErrorKind::CrossesDevices,
                "object crosses a physical mount or firmlink boundary",
            ));
        }
        let metadata = std::fs::symlink_metadata(path)?;
        let valid = self.volume.valid_attributes;
        let returned = raw.returned;

        Ok(ObjectMeasurements {
            identity,
            logical_bytes: metadata.len(),
            link_count: metadata.nlink(),
            allocated_bytes: present(returned.file, valid.file, sys::bits::FILE_ALLOCATED)
                .then_some(raw.allocated),
            data_allocated_bytes: present(
                returned.file,
                valid.file,
                sys::bits::FILE_DATA_ALLOCATED,
            )
            .then_some(raw.data_allocated),
            resource_fork_allocated_bytes: present(
                returned.file,
                valid.file,
                sys::bits::FILE_RESOURCE_ALLOCATED,
            )
            .then_some(raw.resource_allocated),
            private_bytes: present(returned.fork, valid.fork, sys::bits::FORK_PRIVATE)
                .then_some(raw.private),
            extended_flags: present(returned.fork, valid.fork, sys::bits::FORK_EXTENDED_FLAGS)
                .then_some(raw.extended_flags),
        })
    }

    pub fn capacity(&self) -> io::Result<Vec<CapacityValue>> {
        let fallback = sys::filesystem(&self.path_c)?;
        let returned = self.volume.returned.volume;
        let valid = self.volume.valid_attributes.volume;
        let block_bytes = |blocks: u64| fallback.block_size.checked_mul(blocks);
        let measured_or = |kind, bit, primary, primary_basis, fallback, fallback_basis| {
            if present(returned, valid, bit) {
                CapacityValue::Measured {
                    kind,
                    bytes: primary,
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

        Ok(vec![
            CapacityValue::Unmeasurable {
                kind: CapacityKind::ContainerAllocated,
                reason: "no public per-container allocated-byte API is used",
            },
            measured_or(
                CapacityKind::VolumeSize,
                sys::bits::VOLUME_SIZE,
                self.volume.size,
                CapacityBasis::AttrVolSize,
                block_bytes(fallback.blocks),
                CapacityBasis::StatfsBlocks,
            ),
            measured_or(
                CapacityKind::VolumeUsed,
                sys::bits::VOLUME_USED,
                self.volume.used,
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
                self.volume.free,
                CapacityBasis::AttrVolSpaceFree,
                block_bytes(fallback.blocks_free),
                CapacityBasis::StatfsFreeBlocks,
            ),
            measured_or(
                CapacityKind::AvailableNormal,
                sys::bits::VOLUME_AVAILABLE,
                self.volume.available,
                CapacityBasis::AttrVolSpaceAvailable,
                block_bytes(fallback.blocks_available),
                CapacityBasis::StatfsAvailableBlocks,
            ),
            CapacityValue::Unmeasurable {
                kind: CapacityKind::AvailableImportant,
                reason: "requires a separately gated Foundation capacity source",
            },
            CapacityValue::Unmeasurable {
                kind: CapacityKind::AvailableOpportunistic,
                reason: "requires a separately gated Foundation capacity source",
            },
        ])
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
