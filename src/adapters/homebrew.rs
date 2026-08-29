use std::path::{Component, Path};

use crate::adapters::{AdapterDegradedReason, AdapterState};
use crate::policy::PolicyCtx;

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
