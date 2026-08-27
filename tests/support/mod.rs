use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

#[derive(Debug, PartialEq, Eq)]
pub struct TreeSnapshot {
    entries: BTreeMap<PathBuf, SnapshotEntry>,
}

#[derive(Debug, PartialEq, Eq)]
struct SnapshotEntry {
    kind: EntryKind,
    inode: u64,
    mtime_seconds: i64,
    mtime_nanoseconds: i64,
    size: u64,
    xattrs: BTreeSet<OsString>,
}

#[derive(Debug, PartialEq, Eq)]
enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

impl TreeSnapshot {
    pub fn capture(root: &Path) -> io::Result<Self> {
        let mut entries = BTreeMap::new();
        capture_entry(root, root, &mut entries)?;
        Ok(Self { entries })
    }
}

fn capture_entry(
    root: &Path,
    path: &Path,
    entries: &mut BTreeMap<PathBuf, SnapshotEntry>,
) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::Other
    };
    let xattrs = xattr::list(path)?.collect();
    let relative = path
        .strip_prefix(root)
        .expect("captured path must remain under the snapshot root")
        .to_path_buf();

    entries.insert(
        relative,
        SnapshotEntry {
            kind,
            inode: metadata.ino(),
            mtime_seconds: metadata.mtime(),
            mtime_nanoseconds: metadata.mtime_nsec(),
            size: metadata.size(),
            xattrs,
        },
    );

    if file_type.is_dir() {
        let mut children = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(fs::DirEntry::file_name);
        for child in children {
            capture_entry(root, &child.path(), entries)?;
        }
    }

    Ok(())
}

pub struct ReadOnlyFixture {
    _temporary_directory: TempDir,
    pub home: PathBuf,
    snapshot_root: PathBuf,
}

impl ReadOnlyFixture {
    pub fn create() -> io::Result<Self> {
        let temporary_directory = tempfile::tempdir()?;
        let snapshot_root = temporary_directory.path().to_path_buf();
        let home = snapshot_root.join("home");
        let cache = home.join("Library/Caches/example");

        fs::create_dir_all(&cache)?;
        fs::write(cache.join("artifact.bin"), b"fixture")?;
        fs::write(snapshot_root.join("outside-root.txt"), b"sentinel")?;
        xattr::set(
            cache.join("artifact.bin"),
            "com.sizetrail.fixture",
            b"present",
        )?;

        Ok(Self {
            _temporary_directory: temporary_directory,
            home,
            snapshot_root,
        })
    }

    pub fn snapshot(&self) -> io::Result<TreeSnapshot> {
        TreeSnapshot::capture(&self.snapshot_root)
    }
}
