use std::path::{Component, Path, PathBuf};

use crate::adapters::{AdapterDegradedReason, AdapterState};
use crate::fsx::{Root, RootError};
use crate::policy::PolicyCtx;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    pub prefix: PathBuf,
    pub repository: PathBuf,
    pub cellar: Option<PathBuf>,
}

pub fn discover_layout(sandbox_root: Option<&Path>) -> Option<Layout> {
    [
        (Path::new("/opt/homebrew"), Path::new("/opt/homebrew")),
        (Path::new("/usr/local"), Path::new("/usr/local/Homebrew")),
    ]
    .into_iter()
    .find_map(|(prefix, repository)| {
        let prefix = under_sandbox(sandbox_root, prefix);
        if std::fs::symlink_metadata(prefix.join("bin/brew")).is_err()
            || (std::fs::symlink_metadata(prefix.join("Cellar")).is_err()
                && std::fs::symlink_metadata(prefix.join("Caskroom")).is_err())
        {
            return None;
        }
        let repository = under_sandbox(sandbox_root, repository);
        let cellar = if std::fs::symlink_metadata(prefix.join("Cellar")).is_ok() {
            Some(prefix.join("Cellar"))
        } else if std::fs::symlink_metadata(repository.join("Cellar")).is_ok() {
            Some(repository.join("Cellar"))
        } else {
            None
        };
        Some(Layout {
            prefix,
            repository,
            cellar,
        })
    })
}

pub fn open_prefix_root(layout: &Layout) -> Result<Root, RootError> {
    Root::open(&layout.prefix)
}

pub fn probe_version(repository: &Path, _ctx: &mut PolicyCtx<'_>) -> AdapterState {
    read_version(repository).map_or_else(
        || AdapterState::Degraded {
            observed_version: None,
            reason: AdapterDegradedReason::UnknownVersion,
        },
        |version| AdapterState::Ready { version },
    )
}

fn read_version(repository: &Path) -> Option<String> {
    let git = repository.join(".git");
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    let sha = if let Some(reference) = head.strip_prefix("ref: ") {
        read_reference(&git, reference)?
    } else {
        valid_sha(head).then(|| head.to_owned())?
    };
    let version = std::fs::read_to_string(git.join("describe-cache").join(sha)).ok()?;
    let version = version.trim();
    (!version.is_empty() && !version.chars().any(char::is_whitespace)).then(|| version.to_owned())
}

fn read_reference(git: &Path, reference: &str) -> Option<String> {
    let reference_path = Path::new(reference);
    if !reference.starts_with("refs/")
        || !reference_path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    if let Ok(contents) = std::fs::read_to_string(git.join(reference_path)) {
        let sha = contents.trim();
        return valid_sha(sha).then(|| sha.to_owned());
    }
    let packed = std::fs::read_to_string(git.join("packed-refs")).ok()?;
    packed.lines().find_map(|line| {
        let (sha, candidate) = line.split_once(' ')?;
        (candidate == reference && valid_sha(sha)).then(|| sha.to_owned())
    })
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn under_sandbox(sandbox_root: Option<&Path>, absolute: &Path) -> PathBuf {
    sandbox_root.map_or_else(
        || absolute.to_path_buf(),
        |root| root.join(absolute.strip_prefix(Path::new("/")).unwrap_or(absolute)),
    )
}
