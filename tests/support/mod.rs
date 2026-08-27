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
    link_count: u64,
    mtime_seconds: i64,
    mtime_nanoseconds: i64,
    ctime_seconds: i64,
    ctime_nanoseconds: i64,
    size: u64,
    xattrs: BTreeMap<OsString, Vec<u8>>,
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

#[derive(Debug)]
pub struct HighValueEntrySnapshot {
    roots: BTreeMap<PathBuf, Option<BTreeSet<OsString>>>,
}

impl HighValueEntrySnapshot {
    pub fn capture(real_home: Option<&Path>) -> io::Result<Self> {
        // Deliberately shallow: this catches high-value new entries, not arbitrary writes.
        let mut paths = vec![PathBuf::from("/tmp"), PathBuf::from("/var/tmp")];
        if let Some(home) = real_home {
            paths.extend([
                home.join("Library/Logs"),
                home.join("Library/Caches"),
                home.join("Library/Preferences"),
                home.join("Library/Application Support"),
            ]);
        }

        let mut roots = BTreeMap::new();
        for path in paths {
            let entries = match fs::read_dir(&path) {
                Ok(entries) => Some(
                    entries
                        .map(|entry| entry.map(|entry| entry.file_name()))
                        .collect::<io::Result<_>>()?,
                ),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            roots.insert(path, entries);
        }
        Ok(Self { roots })
    }

    pub fn new_entries_since(&self, before: &Self) -> BTreeSet<PathBuf> {
        let mut added = BTreeSet::new();
        for (root, after_entries) in &self.roots {
            match (
                before.roots.get(root).and_then(Option::as_ref),
                after_entries,
            ) {
                (Some(before_entries), Some(after_entries)) => {
                    added.extend(
                        after_entries
                            .difference(before_entries)
                            .map(|entry| root.join(entry)),
                    );
                }
                (None, Some(after_entries)) => {
                    added.insert(root.clone());
                    added.extend(after_entries.iter().map(|entry| root.join(entry)));
                }
                _ => {}
            }
        }
        added
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
    let mut xattrs = BTreeMap::new();
    for name in xattr::list(path)? {
        let value = xattr::get(path, &name)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "xattr disappeared during snapshot: {}",
                    name.to_string_lossy()
                ),
            )
        })?;
        xattrs.insert(name, value);
    }
    let relative = path
        .strip_prefix(root)
        .expect("captured path must remain under the snapshot root")
        .to_path_buf();

    entries.insert(
        relative,
        SnapshotEntry {
            kind,
            inode: metadata.ino(),
            link_count: metadata.nlink(),
            mtime_seconds: metadata.mtime(),
            mtime_nanoseconds: metadata.mtime_nsec(),
            ctime_seconds: metadata.ctime(),
            ctime_nanoseconds: metadata.ctime_nsec(),
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
        for relative in [
            "tmp",
            "xdg/cache",
            "xdg/config",
            "xdg/data",
            "xdg/state",
            "xdg/runtime",
        ] {
            fs::create_dir_all(snapshot_root.join(relative))?;
        }
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

    pub fn environment(&self) -> [(&'static str, PathBuf); 10] {
        let temporary = self.snapshot_root.join("tmp");
        [
            ("HOME", self.home.clone()),
            ("CFFIXED_USER_HOME", self.home.clone()),
            ("TMPDIR", temporary.clone()),
            ("TMP", temporary.clone()),
            ("TEMP", temporary),
            ("XDG_CACHE_HOME", self.snapshot_root.join("xdg/cache")),
            ("XDG_CONFIG_HOME", self.snapshot_root.join("xdg/config")),
            ("XDG_DATA_HOME", self.snapshot_root.join("xdg/data")),
            ("XDG_STATE_HOME", self.snapshot_root.join("xdg/state")),
            ("XDG_RUNTIME_DIR", self.snapshot_root.join("xdg/runtime")),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{ReadOnlyFixture, TreeSnapshot};
    use std::collections::BTreeSet;
    use std::ffi::OsStr;
    use std::fs;

    #[test]
    fn environment_redirects_known_write_locations_under_the_snapshot_root() {
        const EXPECTED: &[&str] = &[
            "CFFIXED_USER_HOME",
            "HOME",
            "TEMP",
            "TMP",
            "TMPDIR",
            "XDG_CACHE_HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_RUNTIME_DIR",
            "XDG_STATE_HOME",
        ];
        let fixture = ReadOnlyFixture::create().expect("fixture must be created");
        let environment = fixture.environment();

        assert_eq!(
            environment
                .iter()
                .map(|(name, _)| *name)
                .collect::<BTreeSet<_>>(),
            EXPECTED.iter().copied().collect()
        );
        assert!(
            environment
                .iter()
                .all(|(_, path)| path.starts_with(&fixture.snapshot_root))
        );
    }

    #[test]
    fn snapshot_records_xattr_values() {
        let fixture = ReadOnlyFixture::create().expect("fixture must be created");
        let artifact = fixture.home.join("Library/Caches/example/artifact.bin");
        xattr::set(&artifact, "com.sizetrail.fixture", b"changed")
            .expect("fixture xattr must be changed");

        let snapshot = fixture.snapshot().expect("snapshot must succeed");
        let entry = snapshot
            .entries
            .get(
                artifact
                    .strip_prefix(&fixture.snapshot_root)
                    .expect("relative path"),
            )
            .expect("artifact must be present");

        assert_eq!(
            entry.xattrs.get(OsStr::new("com.sizetrail.fixture")),
            Some(&b"changed".to_vec())
        );
    }

    #[test]
    fn snapshot_records_hard_link_count() {
        let directory = tempfile::tempdir().expect("fixture directory must be created");
        let artifact = directory.path().join("artifact.bin");
        fs::write(&artifact, b"fixture").expect("fixture file must be written");
        fs::hard_link(&artifact, directory.path().join("second.bin"))
            .expect("hard link must be created");

        let snapshot = TreeSnapshot::capture(&artifact).expect("snapshot must succeed");
        let entry = snapshot
            .entries
            .get(std::path::Path::new(""))
            .expect("artifact must be present");

        assert_eq!(entry.link_count, 2);
    }
}
