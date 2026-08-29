use std::path::PathBuf;

use serde::Serialize;

use crate::model::{Advice, Finding, Measurement, SignalObservation};
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterDegradedReason {
    UnknownVersion,
    ProbeFailed,
    Disabled,
    InvalidSelection,
    NotReady,
}

#[derive(Clone, Debug, Default)]
pub struct Inventory {
    pub items: Vec<InventoryItem>,
    pub gaps: Vec<InventoryGap>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct InventoryItem {
    pub rule_id: String,
    pub normalized_path: String,
    pub path: Option<PathBuf>,
    pub measurements: Vec<Measurement>,
    pub observations: Vec<SignalObservation>,
    pub identity: InventoryIdentity,
}

#[derive(Clone, Debug)]
pub enum InventoryIdentity {
    Path,
    SimulatorDevice {
        udid: String,
        name: String,
        runtime_identifier: String,
        available: bool,
    },
    SimulatorRuntime {
        identifier: String,
        name: String,
        version: String,
        build: String,
        available: bool,
    },
}

#[derive(Clone, Debug)]
pub struct InventoryGap {
    pub region: &'static str,
    pub path: Option<PathBuf>,
    pub reason: InventoryGapReason,
    pub stage: Option<InventoryStage>,
    pub errno: Option<i32>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum InventoryGapReason {
    AbsentOrChanged,
    AccessDenied,
    PolicyDeniedUnknown,
    UnknownVersion,
    NotReady,
    Disabled,
    ProbeFailed,
    TraversalFailed,
    InvalidToolOutput,
    CoreSimulatorVersionMismatch,
    RuntimeSizeUnavailable,
    TimedOut,
    RuleSetInvalid,
    VolumeSnapshotStateUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryStage {
    ListDirectory,
    MeasureObject,
    NormalizePath,
    SimctlDevices,
    SimctlRuntimes,
    RuleEvaluation,
    ToolchainProbe,
    VolumeSnapshots,
}

impl InventoryStage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ListDirectory => "list_directory",
            Self::MeasureObject => "measure_object",
            Self::NormalizePath => "normalize_path",
            Self::SimctlDevices => "simctl_devices",
            Self::SimctlRuntimes => "simctl_runtimes",
            Self::RuleEvaluation => "rule_evaluation",
            Self::ToolchainProbe => "toolchain_probe",
            Self::VolumeSnapshots => "volume_snapshots",
        }
    }
}

impl InventoryGap {
    #[must_use]
    pub const fn diagnostic(region: &'static str, reason: InventoryGapReason) -> Self {
        Self {
            region,
            path: None,
            reason,
            stage: None,
            errno: None,
        }
    }
}

pub trait ToolchainAdapter {
    fn id(&self) -> AdapterId;
    fn probe(&self, ctx: &mut PolicyCtx<'_>) -> AdapterState;
    fn inventory(&self, ctx: &mut PolicyCtx<'_>, state: &AdapterState) -> Inventory;
    fn classify(&self, inventory: &Inventory) -> Result<Vec<Finding>, InventoryGapReason>;
    fn advise(&self, finding: &Finding) -> Vec<Advice>;
}

#[cfg(test)]
mod tests {
    use super::{
        AdapterDegradedReason, AdapterState, InventoryGapReason, InventoryStage, ToolchainAdapter,
        xcode,
    };
    use crate::fsx::Root;
    use crate::policy::{PolicyCtx, ProbePolicy, ReadOnlyCommand};

    const ABSENT_POLICIES: &[ProbePolicy] = &[
        ProbePolicy {
            id: xcode::SELECT_DEVELOPER_DIR,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            known_side_effects: &[],
            command: ReadOnlyCommand {
                program: "/usr/bin/false",
                arguments: &[],
                environment: &[],
                remove_environment: &[],
                timeout_millis: 10_000,
            },
        },
        ProbePolicy {
            id: xcode::XCODEBUILD_VERSION,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            known_side_effects: &[],
            command: ReadOnlyCommand {
                program: "/usr/bin/printf",
                arguments: &["Xcode 26.0\nBuild version Fixture\n"],
                environment: &[],
                remove_environment: &[],
                timeout_millis: 10_000,
            },
        },
        ProbePolicy {
            id: xcode::FIRST_LAUNCH_STATUS,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            known_side_effects: &[],
            command: ReadOnlyCommand {
                program: "/usr/bin/true",
                arguments: &[],
                environment: &[],
                remove_environment: &[],
                timeout_millis: 10_000,
            },
        },
    ];

    const UNKNOWN_VERSION_POLICIES: &[ProbePolicy] = &[
        ProbePolicy {
            id: xcode::SELECT_DEVELOPER_DIR,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            known_side_effects: &[],
            command: ReadOnlyCommand {
                program: "/usr/bin/printf",
                arguments: &["/Applications/Xcode.app/Contents/Developer\n"],
                environment: &[],
                remove_environment: &[],
                timeout_millis: 10_000,
            },
        },
        ProbePolicy {
            id: xcode::XCODEBUILD_VERSION,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            known_side_effects: &[],
            command: ReadOnlyCommand {
                program: "/usr/bin/printf",
                arguments: &["Xcode 999.0\nBuild version Fixture\n"],
                environment: &[],
                remove_environment: &[],
                timeout_millis: 10_000,
            },
        },
        ProbePolicy {
            id: xcode::FIRST_LAUNCH_STATUS,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
            known_side_effects: &[],
            command: ReadOnlyCommand {
                program: "/usr/bin/true",
                arguments: &[],
                environment: &[],
                remove_environment: &[],
                timeout_millis: 10_000,
            },
        },
    ];

    const DEVICES_JSON: &str = r#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-17-4":[{"dataPath":"/Users/test/Library/Developer/CoreSimulator/Devices/11111111-1111-1111-1111-111111111111/data","udid":"11111111-1111-1111-1111-111111111111","isAvailable":true,"deviceTypeIdentifier":"com.apple.CoreSimulator.SimDeviceType.iPhone-15","state":"Shutdown","name":"iPhone 15","futureKey":{"ignore":true}}]}}"#;
    const RUNTIMES_JSON: &str = r#"{"runtimes":[{"bundlePath":"/Library/Developer/CoreSimulator/Volumes/iOS_21E213/Library/Developer/CoreSimulator/Profiles/Runtimes/iOS 17.4.simruntime","buildversion":"21E213","identifier":"com.apple.CoreSimulator.SimRuntime.iOS-17-4","version":"17.4","isAvailable":true,"name":"iOS 17.4"}]}"#;
    const MATCHED_CORE_SIMULATOR: &[&str] = &["1010.15\n"];
    const MISMATCHED_CORE_SIMULATOR: &[&str] = &["1051.17.8\n"];

    fn inventory_policies(core_simulator_version: &'static [&'static str]) -> [ProbePolicy; 3] {
        [
            ProbePolicy {
                id: xcode::CORE_SIMULATOR_VERSION,
                max_calls_per_scan: 1,
                disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
                known_side_effects: &[],
                command: ReadOnlyCommand {
                    program: "/usr/bin/printf",
                    arguments: core_simulator_version,
                    environment: &[],
                    remove_environment: &[],
                    timeout_millis: 10_000,
                },
            },
            ProbePolicy {
                id: xcode::SIMCTL_DEVICES,
                max_calls_per_scan: 1,
                disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
                known_side_effects: &[],
                command: ReadOnlyCommand {
                    program: "/usr/bin/printf",
                    arguments: &[DEVICES_JSON],
                    environment: &[],
                    remove_environment: &[],
                    timeout_millis: 10_000,
                },
            },
            ProbePolicy {
                id: xcode::SIMCTL_RUNTIMES,
                max_calls_per_scan: 1,
                disable_env: "SIZETRAIL_NO_XCODE_PROBE_FIXTURE",
                known_side_effects: &[],
                command: ReadOnlyCommand {
                    program: "/usr/bin/printf",
                    arguments: &[RUNTIMES_JSON],
                    environment: &[],
                    remove_environment: &[],
                    timeout_millis: 10_000,
                },
            },
        ]
    }

    fn fixture_home() -> std::path::PathBuf {
        std::fs::canonicalize(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xcode-home"),
        )
        .expect("checked-in Xcode fixture must canonicalize")
    }

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
                matches!(state, AdapterState::Ready { .. }),
                "the hosted runner Xcode version must be in the reviewed set: {state:?}"
            );

            let home = fixture_home();
            let root = Root::open(&home).expect("hosted inventory fixture must initialize");
            let adapter = xcode::XcodeAdapter::new(&root, &[], Ok(false));
            let inventory = adapter.inventory(&mut ctx, &state);
            let version_mismatch = inventory
                .gaps
                .iter()
                .any(|gap| gap.reason == InventoryGapReason::CoreSimulatorVersionMismatch);

            assert_eq!(ctx.count(xcode::CORE_SIMULATOR_VERSION), 1);
            if version_mismatch {
                assert_eq!(ctx.count(xcode::SIMCTL_DEVICES), 0);
                assert_eq!(ctx.count(xcode::SIMCTL_RUNTIMES), 0);
            } else {
                assert_eq!(ctx.count(xcode::SIMCTL_DEVICES), 1);
                assert_eq!(ctx.count(xcode::SIMCTL_RUNTIMES), 1);
                assert!(
                    inventory.gaps.iter().all(|gap| {
                        !matches!(
                            gap.reason,
                            InventoryGapReason::ProbeFailed | InventoryGapReason::InvalidToolOutput
                        ) || !matches!(
                            gap.stage,
                            Some(InventoryStage::SimctlDevices | InventoryStage::SimctlRuntimes)
                        )
                    }),
                    "matching hosted CoreSimulator must execute and parse the direct binary"
                );
            }
        }
    }

    #[test]
    fn xcode_inventory_joins_static_stores_and_simctl_identity() {
        let home = fixture_home();
        let root = Root::open(&home).expect("fixture root must initialize");
        let adapter = xcode::XcodeAdapter::new(&root, &[], Ok(false));
        let state = AdapterState::Ready {
            version: "16.4 (16F6)".to_owned(),
        };
        let policies = inventory_policies(MATCHED_CORE_SIMULATOR);
        let mut ctx = PolicyCtx::for_test(&policies);

        let inventory = adapter.inventory(&mut ctx, &state);
        let rule_ids = inventory
            .items
            .iter()
            .map(|item| item.rule_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(
            rule_ids,
            std::collections::BTreeSet::from([
                "xcode.archives",
                "xcode.derived_data_build",
                "xcode.device_support",
                "xcode.simulator_device",
                "xcode.simulator_runtime",
            ])
        );
        assert_eq!(inventory.gaps.len(), 1);
        assert_eq!(
            inventory.gaps[0].reason,
            InventoryGapReason::RuntimeSizeUnavailable
        );
        assert_eq!(ctx.count(xcode::SIMCTL_DEVICES), 1);
        assert_eq!(ctx.count(xcode::SIMCTL_RUNTIMES), 1);
        let findings = adapter
            .classify(&inventory)
            .expect("fixture classification must succeed");
        for floor in findings
            .iter()
            .flat_map(|finding| &finding.measurements)
            .filter_map(|measurement| match &measurement.value {
                crate::model::MeasurementValue::IntervalBytes { floor_bytes, .. } => {
                    Some(floor_bytes)
                }
                _ => None,
            })
        {
            assert_eq!(
                *floor, 0,
                "unproven concurrency stability must zero the floor"
            );
        }
        let rendered = serde_json::to_string(
            &findings
                .iter()
                .flat_map(|finding| &finding.advice)
                .collect::<Vec<_>>(),
        )
        .expect("compiled advice must serialize");
        for forbidden in ["--force", "--yes", "|", "sudo ", "/Users/test"] {
            assert!(
                !rendered.contains(forbidden),
                "compiled advice contains forbidden input: {forbidden}"
            );
        }
        assert!(rendered.contains("xcrun simctl delete 11111111-1111-1111-1111-111111111111"));
    }

    #[test]
    fn a_coresimulator_version_mismatch_keeps_static_inventory_but_never_calls_simctl() {
        let home = fixture_home();
        let root = Root::open(&home).expect("fixture root must initialize");
        let adapter = xcode::XcodeAdapter::new(&root, &[], Ok(false));
        let state = AdapterState::Ready {
            version: "16.4 (16F6)".to_owned(),
        };
        let policies = inventory_policies(MISMATCHED_CORE_SIMULATOR);
        let mut ctx = PolicyCtx::for_test(&policies);

        let inventory = adapter.inventory(&mut ctx, &state);

        assert!(
            inventory
                .items
                .iter()
                .any(|item| item.rule_id == "xcode.derived_data_build")
        );
        assert!(
            inventory
                .gaps
                .iter()
                .any(|gap| { gap.reason == InventoryGapReason::CoreSimulatorVersionMismatch })
        );
        assert_eq!(ctx.count(xcode::CORE_SIMULATOR_VERSION), 1);
        assert_eq!(ctx.count(xcode::SIMCTL_DEVICES), 0);
        assert_eq!(ctx.count(xcode::SIMCTL_RUNTIMES), 0);
    }

    #[test]
    fn degraded_xcode_never_starts_coresimulator_inventory() {
        let fixture = tempfile::tempdir().expect("fixture root must be created");
        let home = std::fs::canonicalize(fixture.path()).expect("fixture must canonicalize");
        let root = Root::open(&home).expect("fixture root must initialize");
        let adapter = xcode::XcodeAdapter::new(&root, &[], Ok(false));
        let state = AdapterState::Degraded {
            observed_version: Some("999.0 (Fixture)".to_owned()),
            reason: AdapterDegradedReason::UnknownVersion,
        };
        let policies = inventory_policies(MATCHED_CORE_SIMULATOR);
        let mut ctx = PolicyCtx::for_test(&policies);

        let inventory = adapter.inventory(&mut ctx, &state);

        assert!(inventory.items.is_empty());
        assert_eq!(inventory.gaps[0].reason, InventoryGapReason::UnknownVersion);
        assert_eq!(ctx.count(xcode::SIMCTL_DEVICES), 0);
        assert_eq!(ctx.count(xcode::SIMCTL_RUNTIMES), 0);
    }

    #[test]
    fn excluding_simulator_devices_prevents_the_devices_probe() {
        let home = fixture_home();
        let root = Root::open(&home).expect("fixture root must initialize");
        let devices = root.path().join("Library/Developer/CoreSimulator/Devices");
        let adapter = xcode::XcodeAdapter::new(&root, std::slice::from_ref(&devices), Ok(false));
        let state = AdapterState::Ready {
            version: "16.4 (16F6)".to_owned(),
        };
        let policies = inventory_policies(MATCHED_CORE_SIMULATOR);
        let mut ctx = PolicyCtx::for_test(&policies);

        let inventory = adapter.inventory(&mut ctx, &state);

        assert_eq!(ctx.count(xcode::SIMCTL_DEVICES), 0);
        assert_eq!(ctx.count(xcode::SIMCTL_RUNTIMES), 1);
        assert!(
            root.test_touched_paths()
                .iter()
                .all(|path| !path.starts_with(&devices)),
            "excluded simulator subtree was touched"
        );
        assert!(
            inventory
                .items
                .iter()
                .all(|item| item.rule_id != "xcode.simulator_device")
        );
    }

    #[test]
    fn xcode_fixture_payload_is_byte_stable_across_scans() {
        let home = fixture_home();
        let root = Root::open(&home).expect("fixture root must initialize");
        let adapter = xcode::XcodeAdapter::new(&root, &[], Ok(false));
        let state = AdapterState::Ready {
            version: "16.4 (16F6)".to_owned(),
        };
        let policies = inventory_policies(MATCHED_CORE_SIMULATOR);
        let mut first_ctx = PolicyCtx::for_test(&policies);
        let mut second_ctx = PolicyCtx::for_test(&policies);

        let first = adapter
            .classify(&adapter.inventory(&mut first_ctx, &state))
            .expect("first classification must succeed");
        let second = adapter
            .classify(&adapter.inventory(&mut second_ctx, &state))
            .expect("second classification must succeed");

        assert_eq!(
            serde_json::to_vec(&first).expect("first payload must serialize"),
            serde_json::to_vec(&second).expect("second payload must serialize")
        );
    }

    #[test]
    fn volume_snapshot_state_is_a_typed_signal_or_an_explicit_gap() {
        let home = fixture_home();
        let root = Root::open(&home).expect("fixture root must initialize");
        let state = AdapterState::Ready {
            version: "16.4 (16F6)".to_owned(),
        };
        let with_snapshots = xcode::XcodeAdapter::new(&root, &[], Ok(true));
        let policies = inventory_policies(MATCHED_CORE_SIMULATOR);
        let mut ctx = PolicyCtx::for_test(&policies);
        let inventory = with_snapshots.inventory(&mut ctx, &state);
        assert!(inventory.items.iter().any(|item| {
            item.observations
                .iter()
                .any(|signal| signal.signal == crate::model::SignalId::VolumeHasSnapshots)
        }));

        let unavailable = xcode::XcodeAdapter::new(&root, &[], Err(None));
        let mut ctx = PolicyCtx::for_test(&policies);
        let inventory = unavailable.inventory(&mut ctx, &state);
        assert!(
            inventory
                .gaps
                .iter()
                .any(|gap| { gap.reason == InventoryGapReason::VolumeSnapshotStateUnavailable })
        );
    }

    #[test]
    #[ignore = "records a runner-specific fixture benchmark for publication"]
    fn xcode_inventory_fixture_benchmark() {
        let home = fixture_home();
        let root = Root::open(&home).expect("fixture root must initialize");
        let adapter = xcode::XcodeAdapter::new(&root, &[], Ok(false));
        let state = AdapterState::Ready {
            version: "16.4 (16F6)".to_owned(),
        };
        let mut samples = Vec::new();
        let policies = inventory_policies(MATCHED_CORE_SIMULATOR);
        for _ in 0..5 {
            let mut ctx = PolicyCtx::for_test(&policies);
            let started = std::time::Instant::now();
            let inventory = adapter.inventory(&mut ctx, &state);
            let elapsed = started.elapsed().as_nanos();
            assert!(!inventory.items.is_empty());
            samples.push(elapsed);
        }
        samples.sort_unstable();
        println!(
            "SIZETRAIL_BENCHMARK_JSON={}",
            serde_json::json!({
                "adapter": "xcode",
                "scope": "checked_in_fixture_inventory_with_stubbed_simctl",
                "iterations": samples.len(),
                "median_wall_nanoseconds": samples[samples.len() / 2],
                "all_wall_nanoseconds": samples,
            })
        );
    }
}
