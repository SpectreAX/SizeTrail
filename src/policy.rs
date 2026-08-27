#![allow(clippy::disallowed_types)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::process::Command;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProbeId(&'static str);

impl ProbeId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProbePolicy {
    pub id: ProbeId,
    pub max_calls_per_scan: usize,
    pub disable_env: &'static str,
    pub command: ReadOnlyCommand,
}

#[derive(Clone, Copy, Debug)]
pub struct ReadOnlyCommand {
    pub program: &'static str,
    pub arguments: &'static [&'static str],
    pub environment: &'static [(&'static str, &'static str)],
    pub remove_environment: &'static [&'static str],
}

pub const XCODE_SELECT_DEVELOPER_DIR: ProbeId = ProbeId::new("xcode.select_developer_dir");
pub const XCODE_XCODEBUILD_VERSION: ProbeId = ProbeId::new("xcode.xcodebuild_version");
pub const XCODE_FIRST_LAUNCH_STATUS: ProbeId = ProbeId::new("xcode.first_launch_status");

const XCODE_PROBE_ENVIRONMENT: &[(&str, &str)] = &[("LANG", "C"), ("LC_ALL", "C")];
const XCODE_REMOVED_ENVIRONMENT: &[&str] = &[
    "DEVELOPER_DIR",
    "SDKROOT",
    "TOOLCHAINS",
    "xcrun_cache",
    "xcrun_log",
    "xcrun_verbose",
];

pub const SIDE_EFFECT_REGISTRY: &[ProbePolicy] = &[
    ProbePolicy {
        id: XCODE_SELECT_DEVELOPER_DIR,
        max_calls_per_scan: 1,
        disable_env: "SIZETRAIL_NO_XCODE_PROBE",
        command: ReadOnlyCommand {
            program: "/usr/bin/xcode-select",
            arguments: &["-p"],
            environment: XCODE_PROBE_ENVIRONMENT,
            remove_environment: XCODE_REMOVED_ENVIRONMENT,
        },
    },
    ProbePolicy {
        id: XCODE_XCODEBUILD_VERSION,
        max_calls_per_scan: 1,
        disable_env: "SIZETRAIL_NO_XCODE_PROBE",
        command: ReadOnlyCommand {
            program: "/usr/bin/xcodebuild",
            arguments: &["-version"],
            environment: XCODE_PROBE_ENVIRONMENT,
            remove_environment: XCODE_REMOVED_ENVIRONMENT,
        },
    },
    ProbePolicy {
        id: XCODE_FIRST_LAUNCH_STATUS,
        max_calls_per_scan: 1,
        disable_env: "SIZETRAIL_NO_XCODE_PROBE",
        command: ReadOnlyCommand {
            program: "/usr/bin/xcodebuild",
            arguments: &["-checkFirstLaunchStatus"],
            environment: XCODE_PROBE_ENVIRONMENT,
            remove_environment: XCODE_REMOVED_ENVIRONMENT,
        },
    },
];

#[derive(Debug, Eq, PartialEq)]
pub enum PolicyError {
    UndeclaredProbe(ProbeId),
    Disabled(ProbeId),
    CallLimitExceeded(ProbeId),
    InvocationFailed(ProbeId),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (message, id) = match self {
            Self::UndeclaredProbe(id) => ("undeclared probe", id),
            Self::Disabled(id) => ("probe disabled by environment", id),
            Self::CallLimitExceeded(id) => ("probe call limit exceeded", id),
            Self::InvocationFailed(id) => ("probe invocation failed", id),
        };
        write!(formatter, "{message}: {}", id.as_str())
    }
}

impl Error for PolicyError {}

pub struct InvocationTracker<'a> {
    policies: &'a [ProbePolicy],
    counts: BTreeMap<ProbeId, usize>,
}

pub struct PolicyCtx<'a> {
    tracker: InvocationTracker<'a>,
}

impl PolicyCtx<'static> {
    #[must_use]
    pub fn for_scan() -> Self {
        Self {
            tracker: InvocationTracker::for_scan(),
        }
    }
}

impl PolicyCtx<'_> {
    #[cfg(test)]
    pub(crate) fn for_test(policies: &[ProbePolicy]) -> PolicyCtx<'_> {
        PolicyCtx {
            tracker: InvocationTracker::for_test(policies),
        }
    }

    pub fn run(&mut self, id: ProbeId) -> Result<ReadOnlyOutput, PolicyError> {
        let command = self.tracker.reserve(id)?.command;
        let mut process = Command::new(command.program);
        process
            .args(command.arguments)
            .envs(command.environment.iter().copied());
        for key in command.remove_environment {
            process.env_remove(key);
        }
        let output = process
            .output()
            .map_err(|_| PolicyError::InvocationFailed(id))?;

        Ok(ReadOnlyOutput {
            success: output.status.success(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    #[must_use]
    pub fn count(&self, id: ProbeId) -> usize {
        self.tracker.count(id)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ReadOnlyOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl InvocationTracker<'static> {
    pub fn for_scan() -> Self {
        Self {
            policies: SIDE_EFFECT_REGISTRY,
            counts: BTreeMap::new(),
        }
    }
}

impl<'a> InvocationTracker<'a> {
    #[cfg(test)]
    fn for_test(policies: &'a [ProbePolicy]) -> Self {
        Self {
            policies,
            counts: BTreeMap::new(),
        }
    }

    pub fn invoke<T>(
        &mut self,
        id: ProbeId,
        invocation: impl FnOnce() -> T,
    ) -> Result<T, PolicyError> {
        self.reserve(id)?;
        Ok(invocation())
    }

    fn reserve(&mut self, id: ProbeId) -> Result<ProbePolicy, PolicyError> {
        let policy = self
            .policies
            .iter()
            .find(|policy| policy.id == id)
            .copied()
            .ok_or(PolicyError::UndeclaredProbe(id))?;

        if std::env::var_os(policy.disable_env).is_some() {
            return Err(PolicyError::Disabled(id));
        }

        let count = self.counts.entry(id).or_default();
        if *count >= policy.max_calls_per_scan {
            return Err(PolicyError::CallLimitExceeded(id));
        }

        *count += 1;
        Ok(policy)
    }

    pub fn count(&self, id: ProbeId) -> usize {
        self.counts.get(&id).copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InvocationTracker, PolicyError, ProbeId, ProbePolicy, ReadOnlyCommand,
        SIDE_EFFECT_REGISTRY, XCODE_FIRST_LAUNCH_STATUS, XCODE_PROBE_ENVIRONMENT,
        XCODE_REMOVED_ENVIRONMENT, XCODE_SELECT_DEVELOPER_DIR, XCODE_XCODEBUILD_VERSION,
    };

    #[test]
    fn production_registry_is_the_exact_reviewed_xcode_probe_set() {
        assert_eq!(SIDE_EFFECT_REGISTRY.len(), 3);
        assert_eq!(SIDE_EFFECT_REGISTRY[0].id, XCODE_SELECT_DEVELOPER_DIR);
        assert_eq!(SIDE_EFFECT_REGISTRY[0].max_calls_per_scan, 1);
        assert_eq!(
            SIDE_EFFECT_REGISTRY[0].command.program,
            "/usr/bin/xcode-select"
        );
        assert_eq!(SIDE_EFFECT_REGISTRY[0].command.arguments, ["-p"]);
        assert_eq!(SIDE_EFFECT_REGISTRY[1].id, XCODE_XCODEBUILD_VERSION);
        assert_eq!(SIDE_EFFECT_REGISTRY[1].max_calls_per_scan, 1);
        assert_eq!(
            SIDE_EFFECT_REGISTRY[1].command.program,
            "/usr/bin/xcodebuild"
        );
        assert_eq!(SIDE_EFFECT_REGISTRY[1].command.arguments, ["-version"]);
        assert_eq!(SIDE_EFFECT_REGISTRY[2].id, XCODE_FIRST_LAUNCH_STATUS);
        assert_eq!(SIDE_EFFECT_REGISTRY[2].max_calls_per_scan, 1);
        assert_eq!(
            SIDE_EFFECT_REGISTRY[2].command.program,
            "/usr/bin/xcodebuild"
        );
        assert_eq!(
            SIDE_EFFECT_REGISTRY[2].command.arguments,
            ["-checkFirstLaunchStatus"]
        );
        for policy in SIDE_EFFECT_REGISTRY {
            assert_eq!(policy.command.environment, XCODE_PROBE_ENVIRONMENT);
            assert_eq!(policy.command.remove_environment, XCODE_REMOVED_ENVIRONMENT);
        }
    }

    #[test]
    fn production_tracker_uses_the_compiled_registry() {
        const UNKNOWN_ID: ProbeId = ProbeId::new("fixture.unknown");
        let mut tracker = InvocationTracker::for_scan();

        assert_eq!(
            tracker.invoke(UNKNOWN_ID, || ()),
            Err(PolicyError::UndeclaredProbe(UNKNOWN_ID))
        );
    }

    #[test]
    fn registry_caps_declared_probes_and_rejects_undeclared_probes_before_invocation() {
        const DECLARED_ID: ProbeId = ProbeId::new("fixture.read_only");
        const UNDECLARED_ID: ProbeId = ProbeId::new("fixture.undeclared");
        const POLICIES: &[ProbePolicy] = &[ProbePolicy {
            id: DECLARED_ID,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_FIXTURE_PROBE",
            command: ReadOnlyCommand {
                program: "/usr/bin/true",
                arguments: &[],
                environment: &[],
                remove_environment: &[],
            },
        }];

        let mut tracker = InvocationTracker::for_test(POLICIES);
        let mut actual_calls = 0;

        tracker
            .invoke(DECLARED_ID, || actual_calls += 1)
            .expect("the declared call is within its limit");
        assert!(tracker.invoke(DECLARED_ID, || actual_calls += 1).is_err());
        assert!(tracker.invoke(UNDECLARED_ID, || actual_calls += 1).is_err());

        assert_eq!(actual_calls, 1);
        assert_eq!(tracker.count(DECLARED_ID), 1);
        assert_eq!(tracker.count(UNDECLARED_ID), 0);
    }
}
