use crate::model::{Advice, Finding};
use crate::policy::PolicyCtx;

pub mod xcode;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AdapterId(&'static str);

impl AdapterId {
    #[must_use]
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterState {
    Ready {
        version: String,
    },
    NotPresent,
    Degraded {
        observed_version: Option<String>,
        reason: AdapterDegradedReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterDegradedReason {
    UnknownVersion,
    ProbeFailed,
    Disabled,
    InvalidSelection,
    NotReady,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inventory;

pub trait ToolchainAdapter {
    fn id(&self) -> AdapterId;
    fn probe(&self, ctx: &mut PolicyCtx<'_>) -> AdapterState;
    fn inventory(&self, ctx: &mut PolicyCtx<'_>) -> Inventory;
    fn classify(&self, inventory: &Inventory) -> Vec<Finding>;
    fn advise(&self, finding: &Finding) -> Vec<Advice>;
}

#[cfg(test)]
mod tests {
    use super::{AdapterDegradedReason, AdapterState, xcode};
    use crate::policy::{PolicyCtx, ProbePolicy, ReadOnlyCommand};

    const ABSENT_POLICIES: &[ProbePolicy] = &[
        ProbePolicy {
            id: xcode::SELECT_DEVELOPER_DIR,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            command: ReadOnlyCommand {
                program: "/usr/bin/false",
                arguments: &[],
                environment: &[],
                remove_environment: &[],
            },
        },
        ProbePolicy {
            id: xcode::XCODEBUILD_VERSION,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            command: ReadOnlyCommand {
                program: "/usr/bin/printf",
                arguments: &["Xcode 26.0\nBuild version Fixture\n"],
                environment: &[],
                remove_environment: &[],
            },
        },
        ProbePolicy {
            id: xcode::FIRST_LAUNCH_STATUS,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            command: ReadOnlyCommand {
                program: "/usr/bin/true",
                arguments: &[],
                environment: &[],
                remove_environment: &[],
            },
        },
    ];

    const UNKNOWN_VERSION_POLICIES: &[ProbePolicy] = &[
        ProbePolicy {
            id: xcode::SELECT_DEVELOPER_DIR,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            command: ReadOnlyCommand {
                program: "/usr/bin/printf",
                arguments: &["/Applications/Xcode.app/Contents/Developer\n"],
                environment: &[],
                remove_environment: &[],
            },
        },
        ProbePolicy {
            id: xcode::XCODEBUILD_VERSION,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            command: ReadOnlyCommand {
                program: "/usr/bin/printf",
                arguments: &["Xcode 999.0\nBuild version Fixture\n"],
                environment: &[],
                remove_environment: &[],
            },
        },
        ProbePolicy {
            id: xcode::FIRST_LAUNCH_STATUS,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            command: ReadOnlyCommand {
                program: "/usr/bin/true",
                arguments: &[],
                environment: &[],
                remove_environment: &[],
            },
        },
    ];

    #[test]
    fn missing_xcode_is_not_present_and_does_not_run_the_version_probe() {
        let mut ctx = PolicyCtx::for_test(ABSENT_POLICIES);

        assert_eq!(xcode::probe(&mut ctx), AdapterState::NotPresent);
        assert_eq!(ctx.count(xcode::SELECT_DEVELOPER_DIR), 1);
        assert_eq!(ctx.count(xcode::XCODEBUILD_VERSION), 0);
        assert_eq!(ctx.count(xcode::FIRST_LAUNCH_STATUS), 0);
    }

    #[test]
    fn an_unverified_xcode_version_degrades_after_both_registered_probes_run() {
        let mut ctx = PolicyCtx::for_test(UNKNOWN_VERSION_POLICIES);

        assert_eq!(
            xcode::probe(&mut ctx),
            AdapterState::Degraded {
                observed_version: Some("999.0 (Fixture)".to_owned()),
                reason: AdapterDegradedReason::UnknownVersion,
            }
        );
        assert_eq!(ctx.count(xcode::SELECT_DEVELOPER_DIR), 1);
        assert_eq!(ctx.count(xcode::XCODEBUILD_VERSION), 1);
        assert_eq!(ctx.count(xcode::FIRST_LAUNCH_STATUS), 0);
    }

    #[test]
    fn the_production_xcode_probe_executes_only_its_registered_commands() {
        let mut ctx = PolicyCtx::for_scan();
        let state = xcode::probe(&mut ctx);

        assert_eq!(ctx.count(xcode::SELECT_DEVELOPER_DIR), 1);
        assert!(ctx.count(xcode::XCODEBUILD_VERSION) <= 1);
        assert!(ctx.count(xcode::FIRST_LAUNCH_STATUS) <= 1);
        assert_eq!(
            ctx.count(xcode::XCODEBUILD_VERSION),
            usize::from(!matches!(state, AdapterState::NotPresent))
        );
        if std::env::var_os("GITHUB_ACTIONS").is_some() {
            assert!(
                matches!(
                    state,
                    AdapterState::Ready { .. }
                        | AdapterState::Degraded {
                            reason: AdapterDegradedReason::NotReady,
                            ..
                        }
                ),
                "the hosted runner Xcode version must be in the reviewed set: {state:?}"
            );
        }
    }
}
