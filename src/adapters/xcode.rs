use crate::adapters::{AdapterDegradedReason, AdapterState};
use crate::policy::{
    PolicyCtx, PolicyError, ProbeId, XCODE_FIRST_LAUNCH_STATUS, XCODE_SELECT_DEVELOPER_DIR,
    XCODE_XCODEBUILD_VERSION,
};

pub const SELECT_DEVELOPER_DIR: ProbeId = XCODE_SELECT_DEVELOPER_DIR;
pub const XCODEBUILD_VERSION: ProbeId = XCODE_XCODEBUILD_VERSION;
pub const FIRST_LAUNCH_STATUS: ProbeId = XCODE_FIRST_LAUNCH_STATUS;

const VERIFIED_VERSIONS: &[(&str, &str)] = &[("16.4", "16F6"), ("26.5", "17F42")];

pub fn probe(ctx: &mut PolicyCtx<'_>) -> AdapterState {
    let selected = match ctx.run(SELECT_DEVELOPER_DIR) {
        Ok(output) => output,
        Err(PolicyError::InvocationFailed(_)) => return AdapterState::NotPresent,
        Err(error) => return degraded_from_policy(error),
    };
    if !selected.success {
        return AdapterState::NotPresent;
    }
    let developer_dir = String::from_utf8_lossy(&selected.stdout);
    let developer_dir = developer_dir.trim();
    if developer_dir.ends_with("/Library/Developer/CommandLineTools") {
        return AdapterState::NotPresent;
    }
    if !developer_dir.ends_with("/Contents/Developer") || !developer_dir.contains(".app/") {
        return AdapterState::Degraded {
            observed_version: None,
            reason: AdapterDegradedReason::InvalidSelection,
        };
    }

    let version = match ctx.run(XCODEBUILD_VERSION) {
        Ok(output) if output.success => output,
        Ok(_) => {
            return AdapterState::Degraded {
                observed_version: None,
                reason: AdapterDegradedReason::ProbeFailed,
            };
        }
        Err(error) => return degraded_from_policy(error),
    };
    let text = String::from_utf8_lossy(&version.stdout);
    let mut lines = text.lines();
    let observed = lines
        .next()
        .and_then(|line| line.strip_prefix("Xcode "))
        .map(str::to_owned);
    let build = lines
        .next()
        .and_then(|line| line.strip_prefix("Build version "));
    let verified = observed
        .as_deref()
        .zip(build)
        .is_some_and(|pair| VERIFIED_VERSIONS.contains(&pair));

    match (verified, observed) {
        (true, Some(version)) => match ctx.run(FIRST_LAUNCH_STATUS) {
            Ok(output) if output.success => AdapterState::Ready { version },
            Ok(_) => AdapterState::Degraded {
                observed_version: Some(version),
                reason: AdapterDegradedReason::NotReady,
            },
            Err(error) => degraded_from_policy(error),
        },
        (_, observed_version) => AdapterState::Degraded {
            observed_version,
            reason: AdapterDegradedReason::UnknownVersion,
        },
    }
}

fn degraded_from_policy(error: PolicyError) -> AdapterState {
    AdapterState::Degraded {
        observed_version: None,
        reason: if matches!(error, PolicyError::Disabled(_)) {
            AdapterDegradedReason::Disabled
        } else {
            AdapterDegradedReason::ProbeFailed
        },
    }
}
