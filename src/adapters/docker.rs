use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::adapters::{AdapterDegradedReason, AdapterState, InventoryGapReason};
use crate::model::rounded_bytes;
use crate::policy::{
    DOCKER_CONTEXT_INSPECT, DOCKER_SYSTEM_DF, DOCKER_VERSION, PolicyCtx, PolicyError, ProbeId,
};

pub const CONTEXT_INSPECT: ProbeId = DOCKER_CONTEXT_INSPECT;
pub const VERSION: ProbeId = DOCKER_VERSION;
pub const SYSTEM_DF: ProbeId = DOCKER_SYSTEM_DF;

const VERIFIED_VERSIONS: &[(&str, &str, &str, &str, &str)] = &[(
    "29.7.2",
    "1.55",
    "29.7.2",
    "1.55",
    "Docker Desktop 4.88.1 (237512)",
)];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DockerUsageRow {
    pub object_set_id: &'static str,
    pub total_count: u64,
    pub active_count: u64,
    pub size: String,
    pub reclaimable: String,
}

pub fn probe(ctx: &mut PolicyCtx<'_>, home: &Path) -> AdapterState {
    let context = match ctx.run(CONTEXT_INSPECT) {
        Ok(output) if output.success => output.stdout,
        Ok(_) => {
            return AdapterState::Degraded {
                observed_version: None,
                reason: AdapterDegradedReason::InvalidSelection,
            };
        }
        Err(PolicyError::InvocationFailed(_)) => return AdapterState::NotPresent,
        Err(error) => return degraded_from_policy(error, None),
    };
    if !verified_context(&context, home) {
        return AdapterState::Degraded {
            observed_version: None,
            reason: AdapterDegradedReason::InvalidSelection,
        };
    }

    let output = match ctx.run(VERSION) {
        Ok(output) if output.success => output.stdout,
        Ok(output) => {
            return AdapterState::Degraded {
                observed_version: observed_version(&output.stdout),
                reason: AdapterDegradedReason::ProbeFailed,
            };
        }
        Err(error) => return degraded_from_policy(error, None),
    };
    let Some(version) = parse_verified_version(&output) else {
        return AdapterState::Degraded {
            observed_version: observed_version(&output),
            reason: AdapterDegradedReason::UnknownVersion,
        };
    };
    AdapterState::Ready { version }
}

pub fn system_df(
    ctx: &mut PolicyCtx<'_>,
    state: &AdapterState,
) -> Result<Vec<DockerUsageRow>, InventoryGapReason> {
    match state {
        AdapterState::Ready { .. } => {}
        AdapterState::NotPresent => return Err(InventoryGapReason::AbsentOrChanged),
        AdapterState::Degraded { reason, .. } => return Err(gap_from_degraded(*reason)),
    }
    let output = ctx.run(SYSTEM_DF).map_err(gap_from_policy)?;
    if !output.success {
        return Err(InventoryGapReason::ProbeFailed);
    }
    parse_system_df(&output.stdout)
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContextRecord {
    name: String,
    endpoints: ContextEndpoints,
}

#[derive(Deserialize)]
struct ContextEndpoints {
    docker: ContextDockerEndpoint,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContextDockerEndpoint {
    host: String,
    #[serde(rename = "SkipTLSVerify")]
    skip_tls_verify: bool,
}

fn verified_context(output: &[u8], home: &Path) -> bool {
    let Ok(contexts) = serde_json::from_slice::<Vec<ContextRecord>>(output) else {
        return false;
    };
    let Ok([context]) = <Vec<ContextRecord> as TryInto<[ContextRecord; 1]>>::try_into(contexts)
    else {
        return false;
    };
    context.name == "desktop-linux"
        && !context.endpoints.docker.skip_tls_verify
        && context.endpoints.docker.host
            == format!("unix://{}", home.join(".docker/run/docker.sock").display())
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VersionDocument {
    client: VersionPeer,
    server: VersionPeer,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VersionPeer {
    version: String,
    api_version: String,
    #[serde(default)]
    platform: Option<VersionPlatform>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct VersionPlatform {
    name: String,
}

fn parse_verified_version(output: &[u8]) -> Option<String> {
    let version = serde_json::from_slice::<VersionDocument>(output).ok()?;
    let platform = version.server.platform?.name;
    VERIFIED_VERSIONS
        .contains(&(
            version.client.version.as_str(),
            version.client.api_version.as_str(),
            version.server.version.as_str(),
            version.server.api_version.as_str(),
            platform.as_str(),
        ))
        .then(|| {
            format!(
                "client {} api {}; server {} api {}; {platform}",
                version.client.version,
                version.client.api_version,
                version.server.version,
                version.server.api_version
            )
        })
}

fn observed_version(output: &[u8]) -> Option<String> {
    let version = serde_json::from_slice::<VersionDocument>(output).ok()?;
    Some(format!(
        "client {} api {}; server {} api {}; {}",
        version.client.version,
        version.client.api_version,
        version.server.version,
        version.server.api_version,
        version.server.platform?.name
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawUsageRow {
    r#type: String,
    total_count: String,
    active: String,
    size: String,
    reclaimable: String,
}

fn parse_system_df(output: &[u8]) -> Result<Vec<DockerUsageRow>, InventoryGapReason> {
    let text = std::str::from_utf8(output).map_err(|_| InventoryGapReason::InvalidToolOutput)?;
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        let raw = serde_json::from_str::<RawUsageRow>(line)
            .map_err(|_| InventoryGapReason::InvalidToolOutput)?;
        let total_count = raw
            .total_count
            .parse::<u64>()
            .map_err(|_| InventoryGapReason::InvalidToolOutput)?;
        let active_count = raw
            .active
            .parse::<u64>()
            .map_err(|_| InventoryGapReason::InvalidToolOutput)?;
        rounded_bytes(&raw.size).map_err(|_| InventoryGapReason::InvalidToolOutput)?;
        rounded_bytes(&raw.reclaimable).map_err(|_| InventoryGapReason::InvalidToolOutput)?;
        let object_set_id = match raw.r#type.as_str() {
            "Images" => "docker.images",
            "Containers" => "docker.containers",
            "Local Volumes" => "docker.volumes",
            "Build Cache" => "docker.build_cache",
            _ => return Err(InventoryGapReason::InvalidToolOutput),
        };
        if rows
            .insert(
                object_set_id,
                DockerUsageRow {
                    object_set_id,
                    total_count,
                    active_count,
                    size: raw.size,
                    reclaimable: raw.reclaimable,
                },
            )
            .is_some()
        {
            return Err(InventoryGapReason::InvalidToolOutput);
        }
    }
    [
        "docker.images",
        "docker.containers",
        "docker.volumes",
        "docker.build_cache",
    ]
    .into_iter()
    .map(|id| rows.remove(id).ok_or(InventoryGapReason::InvalidToolOutput))
    .collect::<Result<Vec<_>, _>>()
    .and_then(|ordered| {
        if rows.is_empty() {
            Ok(ordered)
        } else {
            Err(InventoryGapReason::InvalidToolOutput)
        }
    })
}

fn degraded_from_policy(error: PolicyError, observed_version: Option<String>) -> AdapterState {
    let reason = match error {
        PolicyError::Disabled(_) => AdapterDegradedReason::Disabled,
        PolicyError::UndeclaredProbe(_)
        | PolicyError::CallLimitExceeded(_)
        | PolicyError::InvocationFailed(_)
        | PolicyError::TimedOut(_) => AdapterDegradedReason::ProbeFailed,
    };
    AdapterState::Degraded {
        observed_version,
        reason,
    }
}

const fn gap_from_degraded(reason: AdapterDegradedReason) -> InventoryGapReason {
    match reason {
        AdapterDegradedReason::UnknownVersion => InventoryGapReason::UnknownVersion,
        AdapterDegradedReason::Disabled => InventoryGapReason::Disabled,
        AdapterDegradedReason::InvalidSelection => InventoryGapReason::InvalidToolOutput,
        AdapterDegradedReason::ProbeFailed | AdapterDegradedReason::NotReady => {
            InventoryGapReason::ProbeFailed
        }
    }
}

fn gap_from_policy(error: PolicyError) -> InventoryGapReason {
    match error {
        PolicyError::Disabled(_) => InventoryGapReason::Disabled,
        PolicyError::TimedOut(_) => InventoryGapReason::TimedOut,
        PolicyError::UndeclaredProbe(_)
        | PolicyError::CallLimitExceeded(_)
        | PolicyError::InvocationFailed(_) => InventoryGapReason::ProbeFailed,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        CONTEXT_INSPECT, SYSTEM_DF, VERSION, parse_system_df, probe, system_df, verified_context,
    };
    use crate::adapters::{AdapterDegradedReason, AdapterState, InventoryGapReason};
    use crate::policy::{PolicyCtx, ProbePolicy, ReadOnlyCommand};

    const CONTEXT: &str = include_str!("../../tests/fixtures/docker/context-desktop-linux.json");
    const VERSION_JSON: &str = include_str!("../../tests/fixtures/docker/version-verified.json");
    const SYSTEM_DF_JSON: &str = include_str!("../../tests/fixtures/docker/system-df.ndjson");

    const CONTEXT_ARGS: &[&str] = &["%s", CONTEXT];
    const VERSION_ARGS: &[&str] = &["%s", VERSION_JSON];
    const SYSTEM_DF_ARGS: &[&str] = &["%s", SYSTEM_DF_JSON];

    fn policies(
        context: &'static [&'static str],
        version: &'static [&'static str],
        df: &'static [&'static str],
    ) -> [ProbePolicy; 3] {
        [
            printf_policy(CONTEXT_INSPECT, context),
            printf_policy(VERSION, version),
            printf_policy(SYSTEM_DF, df),
        ]
    }

    const fn printf_policy(
        id: crate::policy::ProbeId,
        arguments: &'static [&'static str],
    ) -> ProbePolicy {
        ProbePolicy {
            id,
            max_calls_per_scan: 1,
            disable_env: "SIZETRAIL_NO_DOCKER_PROBE_FIXTURE",
            known_side_effects: &[],
            command: ReadOnlyCommand {
                program: "/usr/bin/printf",
                arguments,
                environment: &[],
                remove_environment: &[],
                timeout_millis: 10_000,
            },
        }
    }

    #[test]
    fn verified_local_context_and_version_allow_one_summary_call() {
        let policies = policies(CONTEXT_ARGS, VERSION_ARGS, SYSTEM_DF_ARGS);
        let mut ctx = PolicyCtx::for_test(&policies);
        let state = probe(&mut ctx, Path::new("/Users/fixture"));

        assert!(matches!(state, AdapterState::Ready { .. }));
        let rows = system_df(&mut ctx, &state).expect("verified summary must parse");
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].object_set_id, "docker.images");
        assert_eq!(rows[3].object_set_id, "docker.build_cache");
        assert_eq!(ctx.count(CONTEXT_INSPECT), 1);
        assert_eq!(ctx.count(VERSION), 1);
        assert_eq!(ctx.count(SYSTEM_DF), 1);
    }

    #[test]
    fn remote_context_stops_before_any_daemon_connection() {
        const REMOTE: &str =
            r#"[{"Name":"desktop-linux","Endpoints":{"docker":{"Host":"ssh://builder.example"}}}]"#;
        const REMOTE_ARGS: &[&str] = &["%s", REMOTE];
        let policies = policies(REMOTE_ARGS, VERSION_ARGS, SYSTEM_DF_ARGS);
        let mut ctx = PolicyCtx::for_test(&policies);
        let state = probe(&mut ctx, Path::new("/Users/fixture"));

        assert_eq!(
            state,
            AdapterState::Degraded {
                observed_version: None,
                reason: AdapterDegradedReason::InvalidSelection,
            }
        );
        assert!(system_df(&mut ctx, &state).is_err());
        assert_eq!(ctx.count(CONTEXT_INSPECT), 1);
        assert_eq!(ctx.count(VERSION), 0);
        assert_eq!(ctx.count(SYSTEM_DF), 0);
    }

    #[test]
    fn context_identity_rejects_every_nonlocal_or_tls_variant() {
        for (host, skip_tls_verify) in [
            ("tcp://127.0.0.1:2375", false),
            ("ssh://builder.example", false),
            ("unix:///var/run/docker.sock", false),
            ("unix:///Users/other/.docker/run/docker.sock", false),
            ("unix:///Users/fixture/.docker/run/docker.sock", true),
        ] {
            let context = serde_json::to_vec(&serde_json::json!([{
                "Name": "desktop-linux",
                "Endpoints": {"docker": {"Host": host, "SkipTLSVerify": skip_tls_verify}}
            }]))
            .expect("context fixture must serialize");
            assert!(!verified_context(&context, Path::new("/Users/fixture")));
        }
    }

    #[test]
    fn unknown_version_never_runs_system_df() {
        const UNKNOWN: &str = r#"{"Client":{"Version":"99.0.0","ApiVersion":"9.99"},"Server":{"Platform":{"Name":"Docker Desktop 99.0.0"},"Version":"99.0.0","ApiVersion":"9.99"}}"#;
        const UNKNOWN_ARGS: &[&str] = &["%s", UNKNOWN];
        let policies = policies(CONTEXT_ARGS, UNKNOWN_ARGS, SYSTEM_DF_ARGS);
        let mut ctx = PolicyCtx::for_test(&policies);
        let state = probe(&mut ctx, Path::new("/Users/fixture"));

        assert!(matches!(
            state,
            AdapterState::Degraded {
                reason: AdapterDegradedReason::UnknownVersion,
                ..
            }
        ));
        assert!(system_df(&mut ctx, &state).is_err());
        assert_eq!(ctx.count(CONTEXT_INSPECT), 1);
        assert_eq!(ctx.count(VERSION), 1);
        assert_eq!(ctx.count(SYSTEM_DF), 0);
    }

    #[test]
    fn malformed_or_nonzero_summary_is_a_typed_gap_without_retry() {
        const MALFORMED_ARGS: &[&str] = &["%s", "not-json\n"];
        let malformed = policies(CONTEXT_ARGS, VERSION_ARGS, MALFORMED_ARGS);
        let mut malformed_ctx = PolicyCtx::for_test(&malformed);
        let state = probe(&mut malformed_ctx, Path::new("/Users/fixture"));
        assert!(system_df(&mut malformed_ctx, &state).is_err());
        assert_eq!(malformed_ctx.count(SYSTEM_DF), 1);

        let mut nonzero = policies(CONTEXT_ARGS, VERSION_ARGS, SYSTEM_DF_ARGS);
        nonzero[2].command.program = "/usr/bin/false";
        nonzero[2].command.arguments = &[];
        let mut nonzero_ctx = PolicyCtx::for_test(&nonzero);
        let state = probe(&mut nonzero_ctx, Path::new("/Users/fixture"));
        assert!(system_df(&mut nonzero_ctx, &state).is_err());
        assert_eq!(nonzero_ctx.count(SYSTEM_DF), 1);
    }

    #[test]
    fn disabled_or_timed_out_probes_are_typed_and_never_retried() {
        let mut disabled = policies(CONTEXT_ARGS, VERSION_ARGS, SYSTEM_DF_ARGS);
        disabled[0].disable_env = "PATH";
        let mut disabled_ctx = PolicyCtx::for_test(&disabled);
        assert_eq!(
            probe(&mut disabled_ctx, Path::new("/Users/fixture")),
            AdapterState::Degraded {
                observed_version: None,
                reason: AdapterDegradedReason::Disabled,
            }
        );
        assert_eq!(disabled_ctx.count(CONTEXT_INSPECT), 0);
        assert_eq!(disabled_ctx.count(VERSION), 0);
        assert_eq!(disabled_ctx.count(SYSTEM_DF), 0);

        let mut timed_out = policies(CONTEXT_ARGS, VERSION_ARGS, SYSTEM_DF_ARGS);
        timed_out[2].command.program = "/bin/sleep";
        timed_out[2].command.arguments = &["1"];
        timed_out[2].command.timeout_millis = 1;
        let mut timed_out_ctx = PolicyCtx::for_test(&timed_out);
        let state = probe(&mut timed_out_ctx, Path::new("/Users/fixture"));
        assert_eq!(
            system_df(&mut timed_out_ctx, &state),
            Err(InventoryGapReason::TimedOut)
        );
        assert_eq!(timed_out_ctx.count(SYSTEM_DF), 1);
    }

    #[test]
    fn summary_requires_the_exact_four_typed_rows_and_valid_vendor_numbers() {
        let missing = SYSTEM_DF_JSON
            .lines()
            .take(3)
            .collect::<Vec<_>>()
            .join("\n");
        let duplicate = format!(
            "{SYSTEM_DF_JSON}{}\n",
            SYSTEM_DF_JSON.lines().next().expect("images fixture row")
        );
        let unknown = SYSTEM_DF_JSON.replace("Build Cache", "Networks");
        let negative = SYSTEM_DF_JSON.replacen("\"11\"", "\"-1\"", 1);
        let overflow = SYSTEM_DF_JSON.replacen("\"11\"", "\"18446744073709551616\"", 1);
        let extra = format!("{SYSTEM_DF_JSON}docker diagnostic text\n");

        for invalid in [missing, duplicate, unknown, negative, overflow, extra] {
            assert_eq!(
                parse_system_df(invalid.as_bytes()),
                Err(InventoryGapReason::InvalidToolOutput)
            );
        }
        assert_eq!(
            parse_system_df(&[0xff]),
            Err(InventoryGapReason::InvalidToolOutput)
        );
    }
}
