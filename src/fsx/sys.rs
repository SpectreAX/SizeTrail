#![allow(unsafe_code)]

use std::ffi::{CStr, c_char, c_int, c_void};
use std::fs::File;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;

const ATTR_BIT_MAP_COUNT: u16 = 5;
const ATTR_CMN_FSID: u32 = 0x0000_0004;
const ATTR_CMN_NAME: u32 = 0x0000_0001;
const ATTR_CMN_FILEID: u32 = 0x0200_0000;
const ATTR_CMN_RETURNED_ATTRS: u32 = 0x8000_0000;
const ATTR_VOL_SIZE: u32 = 0x0000_0004;
const ATTR_VOL_SPACEFREE: u32 = 0x0000_0008;
const ATTR_VOL_SPACEAVAIL: u32 = 0x0000_0010;
const ATTR_VOL_CAPABILITIES: u32 = 0x0002_0000;
const ATTR_VOL_SPACEUSED: u32 = 0x0080_0000;
const ATTR_VOL_ATTRIBUTES: u32 = 0x4000_0000;
const ATTR_VOL_INFO: u32 = 0x8000_0000;
const ATTR_FILE_ALLOCSIZE: u32 = 0x0000_0004;
const ATTR_FILE_DATAALLOCSIZE: u32 = 0x0000_0400;
const ATTR_FILE_RSRCALLOCSIZE: u32 = 0x0000_2000;
const ATTR_CMNEXT_PRIVATESIZE: u32 = 0x0000_0008;
const ATTR_CMNEXT_REALFSID: u32 = 0x0000_0080;
const ATTR_CMNEXT_EXT_FLAGS: u32 = 0x0000_0200;
const FSOPT_NOFOLLOW: u64 = 0x0000_0001;
const FSOPT_PACK_INVAL_ATTRS: u64 = 0x0000_0008;
const FSOPT_ATTR_CMN_EXTENDED: u64 = 0x0000_0020;
const FSOPT_RETURN_REALDEV: u64 = 0x0000_0200;
const FSOPT_NOFOLLOW_ANY: u64 = 0x0000_0800;

const IOPOL_SCOPE_PROCESS: c_int = 0;
const IOPOL_TYPE_VFS_ATIME_UPDATES: c_int = 2;
const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: c_int = 3;
const IOPOL_TYPE_VFS_TRIGGER_RESOLVE: c_int = 5;

#[repr(C)]
struct AttrList {
    bitmap_count: u16,
    reserved: u16,
    common: u32,
    volume: u32,
    directory: u32,
    file: u32,
    fork: u32,
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub(super) struct AttributeSet {
    pub common: u32,
    pub volume: u32,
    pub directory: u32,
    pub file: u32,
    pub fork: u32,
}

#[repr(C)]
struct StatFs {
    block_size: u32,
    io_size: i32,
    blocks: u64,
    blocks_free: u64,
    blocks_available: u64,
    files: u64,
    files_free: u64,
    fsid: [i32; 2],
    owner: u32,
    fs_type: u32,
    flags: u32,
    fs_subtype: u32,
    fs_type_name: [c_char; 16],
    mounted_on: [c_char; 1024],
    mounted_from: [c_char; 1024],
    flags_extended: u32,
    reserved: [u32; 7],
}

unsafe extern "C" {
    fn fs_snapshot_list(
        directory: c_int,
        attributes: *mut AttrList,
        buffer: *mut c_void,
        buffer_size: usize,
        flags: u32,
    ) -> c_int;
    fn fstatfs(file: c_int, stats: *mut StatFs) -> c_int;
    fn getattrlist(
        path: *const c_char,
        attributes: *const AttrList,
        buffer: *mut c_void,
        buffer_size: usize,
        options: u64,
    ) -> c_int;
    fn getiopolicy_np(policy_type: c_int, scope: c_int) -> c_int;
    fn setiopolicy_np(policy_type: c_int, scope: c_int, policy: c_int) -> c_int;
    fn statfs(path: *const c_char, stats: *mut StatFs) -> c_int;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFURLVolumeAvailableCapacityForImportantUsageKey: *const c_void;
    static kCFURLVolumeAvailableCapacityForOpportunisticUsageKey: *const c_void;
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: *const c_void,
        buffer: *const u8,
        buffer_length: isize,
        is_directory: u8,
    ) -> *const c_void;
    fn CFURLCopyResourcePropertyForKey(
        url: *const c_void,
        key: *const c_void,
        value: *mut *const c_void,
        error: *mut *const c_void,
    ) -> u8;
    fn CFNumberGetValue(number: *const c_void, number_type: isize, value: *mut c_void) -> u8;
    fn CFRelease(object: *const c_void);
}

#[derive(Clone, Copy, Debug)]
pub(super) struct VolumeRaw {
    pub returned: AttributeSet,
    pub size: u64,
    pub free: u64,
    pub available: u64,
    pub used: u64,
    pub capabilities: [u32; 4],
    pub valid_capabilities: [u32; 4],
    pub valid_attributes: AttributeSet,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ObjectRaw {
    pub returned: AttributeSet,
    pub fileid: u64,
    pub allocated: u64,
    pub data_allocated: u64,
    pub resource_allocated: u64,
    pub private: u64,
    pub real_fsid: [i32; 2],
    pub extended_flags: u64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct IdentityRaw {
    pub returned: AttributeSet,
    pub fileid: u64,
    pub real_fsid: [i32; 2],
}

#[derive(Clone, Copy, Debug)]
pub(super) struct StatFsRaw {
    pub block_size: u64,
    pub blocks: u64,
    pub blocks_free: u64,
    pub blocks_available: u64,
}

pub(super) fn install_read_policies() -> io::Result<()> {
    for (policy_type, value) in [
        (IOPOL_TYPE_VFS_ATIME_UPDATES, 1),
        (IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES, 1),
        (IOPOL_TYPE_VFS_TRIGGER_RESOLVE, 1),
    ] {
        // SAFETY: the call takes only integer policy values. The return is checked.
        let status = unsafe { setiopolicy_np(policy_type, IOPOL_SCOPE_PROCESS, value) };
        if status == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: this query takes only integer policy values. The return is checked.
        let observed = unsafe { getiopolicy_np(policy_type, IOPOL_SCOPE_PROCESS) };
        if observed == -1 {
            return Err(io::Error::last_os_error());
        }
        if observed != value {
            return Err(io::Error::other(format!(
                "I/O policy {policy_type} verified as {observed}, expected {value}"
            )));
        }
    }
    Ok(())
}

pub(super) fn volume(path: &CStr) -> io::Result<VolumeRaw> {
    let attributes = AttrList {
        bitmap_count: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        common: ATTR_CMN_RETURNED_ATTRS,
        volume: ATTR_VOL_SIZE
            | ATTR_VOL_SPACEFREE
            | ATTR_VOL_SPACEAVAIL
            | ATTR_VOL_CAPABILITIES
            | ATTR_VOL_SPACEUSED
            | ATTR_VOL_ATTRIBUTES
            | ATTR_VOL_INFO,
        directory: 0,
        file: 0,
        fork: 0,
    };
    let bytes = call_getattrlist(path, &attributes)?;
    let mut cursor = Cursor::new(&bytes)?;
    let returned = cursor.attribute_set()?;
    let size = cursor.u64()?;
    let free = cursor.u64()?;
    let available = cursor.u64()?;
    let used = cursor.u64()?;
    let capabilities = cursor.u32_array()?;
    let valid_capabilities = cursor.u32_array()?;
    let valid_attributes = cursor.attribute_set()?;
    let _native_attributes = cursor.attribute_set()?;
    Ok(VolumeRaw {
        returned,
        size,
        free,
        available,
        used,
        capabilities,
        valid_capabilities,
        valid_attributes,
    })
}

pub(super) fn identity(path: &CStr) -> io::Result<IdentityRaw> {
    let attributes = AttrList {
        bitmap_count: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        common: ATTR_CMN_RETURNED_ATTRS | ATTR_CMN_FSID | ATTR_CMN_FILEID,
        volume: 0,
        directory: 0,
        file: 0,
        fork: ATTR_CMNEXT_REALFSID,
    };
    let bytes = call_getattrlist(path, &attributes)?;
    let mut cursor = Cursor::new(&bytes)?;
    let returned = cursor.attribute_set()?;
    let _logical_fsid = cursor.i32_pair()?;
    Ok(IdentityRaw {
        returned,
        fileid: cursor.u64()?,
        real_fsid: cursor.i32_pair()?,
    })
}

pub(super) fn object(path: &CStr) -> io::Result<ObjectRaw> {
    let attributes = AttrList {
        bitmap_count: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        common: ATTR_CMN_RETURNED_ATTRS | ATTR_CMN_FSID | ATTR_CMN_FILEID,
        volume: 0,
        directory: 0,
        file: ATTR_FILE_ALLOCSIZE | ATTR_FILE_DATAALLOCSIZE | ATTR_FILE_RSRCALLOCSIZE,
        fork: ATTR_CMNEXT_PRIVATESIZE | ATTR_CMNEXT_REALFSID | ATTR_CMNEXT_EXT_FLAGS,
    };
    let bytes = call_getattrlist(path, &attributes)?;
    let mut cursor = Cursor::new(&bytes)?;
    let returned = cursor.attribute_set()?;
    let _logical_fsid = cursor.i32_pair()?;
    Ok(ObjectRaw {
        returned,
        fileid: cursor.u64()?,
        allocated: cursor.u64()?,
        data_allocated: cursor.u64()?,
        resource_allocated: cursor.u64()?,
        private: cursor.u64()?,
        real_fsid: cursor.i32_pair()?,
        extended_flags: cursor.u64()?,
    })
}

pub(super) fn filesystem(path: &CStr) -> io::Result<StatFsRaw> {
    let stats = call_statfs(path)?;
    Ok(StatFsRaw {
        block_size: u64::from(stats.block_size),
        blocks: stats.blocks,
        blocks_free: stats.blocks_free,
        blocks_available: stats.blocks_available,
    })
}

pub(super) fn snapshot_count(path: &CStr) -> io::Result<usize> {
    let path_stats = call_statfs(path)?;
    // SAFETY: successful statfs initializes mounted_on as a NUL-terminated C array.
    let mounted_on = unsafe { CStr::from_ptr(path_stats.mounted_on.as_ptr()) };
    let mount_root = File::open(std::ffi::OsStr::from_bytes(mounted_on.to_bytes()))?;
    let mut root_stats = MaybeUninit::<StatFs>::zeroed();
    // SAFETY: the file descriptor is live and the output points to correctly laid-out storage.
    let status = unsafe { fstatfs(mount_root.as_raw_fd(), root_stats.as_mut_ptr()) };
    if status == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstatfs initialized the structure.
    let root_stats = unsafe { root_stats.assume_init() };
    if root_stats.fsid != path_stats.fsid {
        return Err(io::Error::other(
            "volume changed while resolving its mounted root",
        ));
    }
    snapshot_count_fd(&mount_root)
}

fn call_statfs(path: &CStr) -> io::Result<StatFs> {
    let mut stats = MaybeUninit::<StatFs>::zeroed();
    // SAFETY: the pointer addresses a correctly laid-out allocation and is read
    // only after a checked successful call.
    let status = unsafe { statfs(path.as_ptr(), stats.as_mut_ptr()) };
    if status == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful statfs initialized the structure.
    Ok(unsafe { stats.assume_init() })
}

fn snapshot_count_fd(mount_root: &File) -> io::Result<usize> {
    let mut attributes = AttrList {
        bitmap_count: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        common: ATTR_CMN_NAME | ATTR_CMN_RETURNED_ATTRS,
        volume: 0,
        directory: 0,
        file: 0,
        fork: 0,
    };
    let mut buffer = vec![0_u8; 8 * 1024];
    // SAFETY: the descriptor is a verified mounted-volume root, flags are zero,
    // and both ABI structures remain live for the checked call.
    let status = unsafe {
        fs_snapshot_list(
            mount_root.as_raw_fd(),
            &raw mut attributes,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            0,
        )
    };
    if status == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(status as usize)
    }
}

pub(super) fn important_capacity(path: &CStr) -> io::Result<u64> {
    // SAFETY: the imported key is an immutable CoreFoundation constant.
    let key = unsafe { kCFURLVolumeAvailableCapacityForImportantUsageKey };
    resource_capacity(path, key)
}

pub(super) fn opportunistic_capacity(path: &CStr) -> io::Result<u64> {
    // SAFETY: the imported key is an immutable CoreFoundation constant.
    let key = unsafe { kCFURLVolumeAvailableCapacityForOpportunisticUsageKey };
    resource_capacity(path, key)
}

fn resource_capacity(path: &CStr, key: *const c_void) -> io::Result<u64> {
    let path_bytes = path.to_bytes();
    let length = isize::try_from(path_bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path is too long"))?;
    // SAFETY: bytes and length describe the live CStr storage. A null allocator
    // selects the CoreFoundation default allocator.
    let url = unsafe {
        CFURLCreateFromFileSystemRepresentation(std::ptr::null(), path_bytes.as_ptr(), length, 1)
    };
    if url.is_null() {
        return Err(io::Error::other(
            "CoreFoundation could not create a file URL",
        ));
    }

    let mut value = std::ptr::null();
    let mut error = std::ptr::null();
    // SAFETY: url and key are immutable CF objects; output pointers address
    // initialized local slots and the return value is checked.
    let copied = unsafe { CFURLCopyResourcePropertyForKey(url, key, &mut value, &mut error) };
    if copied == 0 || value.is_null() {
        // SAFETY: non-null results are owned references returned by CF.
        unsafe {
            if !error.is_null() {
                CFRelease(error);
            }
            CFRelease(url);
        }
        return Err(io::Error::other(
            "CoreFoundation capacity property was unavailable",
        ));
    }
    if !error.is_null() {
        // SAFETY: an unexpected non-null error is still an owned CF result.
        unsafe { CFRelease(error) };
    }

    let mut signed_bytes = 0_i64;
    // SAFETY: value is the documented CFNumber for these keys and the output
    // points to an i64 matching kCFNumberSInt64Type (4).
    let converted = unsafe { CFNumberGetValue(value, 4, (&raw mut signed_bytes).cast()) };
    // SAFETY: value and url are owned references and are released once.
    unsafe {
        CFRelease(value);
        CFRelease(url);
    }
    if converted == 0 {
        return Err(io::Error::other(
            "CoreFoundation capacity did not fit a signed 64-bit integer",
        ));
    }
    u64::try_from(signed_bytes)
        .map_err(|_| io::Error::other("CoreFoundation returned a negative capacity"))
}

fn call_getattrlist(path: &CStr, attributes: &AttrList) -> io::Result<Vec<u8>> {
    let mut buffer = vec![0_u8; 256];
    // SAFETY: path is NUL-terminated, AttrList follows the SDK ABI, and the
    // owned buffer remains valid for the complete checked call.
    let status = unsafe {
        getattrlist(
            path.as_ptr(),
            attributes,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            FSOPT_NOFOLLOW
                | FSOPT_PACK_INVAL_ATTRS
                | FSOPT_ATTR_CMN_EXTENDED
                | FSOPT_RETURN_REALDEV
                | FSOPT_NOFOLLOW_ANY,
        )
    };
    if status == -1 {
        return Err(io::Error::last_os_error());
    }
    let length = u32::from_ne_bytes(
        buffer[..4]
            .try_into()
            .map_err(|_| io::Error::other("getattrlist omitted its length"))?,
    ) as usize;
    if !(4..=buffer.len()).contains(&length) {
        return Err(io::Error::other("getattrlist returned an invalid length"));
    }
    buffer.truncate(length);
    Ok(buffer)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> io::Result<Self> {
        if bytes.len() < 4 {
            return Err(io::Error::other("attribute buffer is truncated"));
        }
        Ok(Self { bytes, offset: 4 })
    }

    fn take<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or_else(|| io::Error::other("attribute offset overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| {
                io::Error::other(format!(
                    "attribute buffer is truncated at {}..{end} of {} bytes",
                    self.offset,
                    self.bytes.len()
                ))
            })?
            .try_into()
            .map_err(|_| io::Error::other("attribute width is invalid"))?;
        self.offset = end;
        Ok(value)
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_ne_bytes(self.take()?))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_ne_bytes(self.take()?))
    }

    fn i32_pair(&mut self) -> io::Result<[i32; 2]> {
        Ok([
            i32::from_ne_bytes(self.take()?),
            i32::from_ne_bytes(self.take()?),
        ])
    }

    fn u32_array(&mut self) -> io::Result<[u32; 4]> {
        Ok([self.u32()?, self.u32()?, self.u32()?, self.u32()?])
    }

    fn attribute_set(&mut self) -> io::Result<AttributeSet> {
        Ok(AttributeSet {
            common: self.u32()?,
            volume: self.u32()?,
            directory: self.u32()?,
            file: self.u32()?,
            fork: self.u32()?,
        })
    }
}

pub(super) mod bits {
    pub const COMMON_FSID: u32 = super::ATTR_CMN_FSID;
    pub const COMMON_FILEID: u32 = super::ATTR_CMN_FILEID;
    pub const VOLUME_SIZE: u32 = super::ATTR_VOL_SIZE;
    pub const VOLUME_FREE: u32 = super::ATTR_VOL_SPACEFREE;
    pub const VOLUME_AVAILABLE: u32 = super::ATTR_VOL_SPACEAVAIL;
    pub const VOLUME_USED: u32 = super::ATTR_VOL_SPACEUSED;
    pub const FILE_ALLOCATED: u32 = super::ATTR_FILE_ALLOCSIZE;
    pub const FILE_DATA_ALLOCATED: u32 = super::ATTR_FILE_DATAALLOCSIZE;
    pub const FILE_RESOURCE_ALLOCATED: u32 = super::ATTR_FILE_RSRCALLOCSIZE;
    pub const FORK_PRIVATE: u32 = super::ATTR_CMNEXT_PRIVATESIZE;
    pub const FORK_REAL_FSID: u32 = super::ATTR_CMNEXT_REALFSID;
    pub const FORK_EXTENDED_FLAGS: u32 = super::ATTR_CMNEXT_EXT_FLAGS;
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::{AttrList, AttributeSet, StatFs, install_read_policies, snapshot_count_fd};

    #[test]
    fn copied_darwin_abi_layouts_match_the_sdk() {
        assert_eq!(size_of::<AttrList>(), 24);
        assert_eq!(size_of::<AttributeSet>(), 20);
        assert_eq!(size_of::<StatFs>(), 2168);
    }

    #[test]
    fn read_induced_write_guards_set_and_verify_on_this_process() {
        install_read_policies().expect("read I/O policies must round-trip");
    }

    #[test]
    fn snapshot_listing_rejects_a_non_root_directory_descriptor() {
        let fixture = tempfile::tempdir().expect("fixture directory must be created");
        let directory = File::open(fixture.path()).expect("fixture directory must open");
        let error = snapshot_count_fd(&directory)
            .expect_err("snapshot listing requires the mounted filesystem root");
        assert_eq!(error.raw_os_error(), Some(22));
    }
}
