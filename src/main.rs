#![forbid(clippy::disallowed_methods)]
#![forbid(
    clippy::let_underscore_must_use,
    clippy::let_underscore_untyped,
    clippy::unwrap_used
)]
#![deny(clippy::disallowed_types)]

use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::{fs, io};

use clap::{Arg, ArgAction, Command, value_parser};
use clap_complete::{Shell, generate};
use sizetrail::capacity;
use sizetrail::fsx::Root;
use sizetrail::model::EnvironmentEnvelope;
use sizetrail::policy::{PolicyCtx, SIDE_EFFECT_REGISTRY};
use sizetrail::rules::builtin_rules;
use sizetrail::scan::{
    excluded_adapter_report, scan, unmeasurable_adapter_report, xcode_report,
    xcode_report_with_sink,
};

fn command() -> Command {
    Command::new("sizetrail")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::new("root")
                .long("root")
                .global(true)
                .hide(true)
                .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            Arg::new("debug")
                .long("debug")
                .global(true)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no-color")
                .long("no-color")
                .global(true)
                .action(ArgAction::SetTrue),
        )
        .subcommand(
            Command::new("scan")
                .arg(Arg::new("json").long("json").action(ArgAction::SetTrue))
                .arg(
                    Arg::new("no-xcode")
                        .long("no-xcode")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("no-homebrew")
                        .long("no-homebrew")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("no-docker")
                        .long("no-docker")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("exclude")
                        .long("exclude")
                        .action(ArgAction::Append)
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("explain")
                .arg(Arg::new("finding-id").required(true))
                .arg(
                    Arg::new("json")
                        .long("json")
                        .action(ArgAction::SetTrue)
                        .conflicts_with("path"),
                )
                .arg(Arg::new("path").long("path").action(ArgAction::SetTrue))
                .arg(
                    Arg::new("from")
                        .long("from")
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("doctor")
                .arg(Arg::new("json").long("json").action(ArgAction::SetTrue))
                .arg(
                    Arg::new("no-xcode")
                        .long("no-xcode")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("no-homebrew")
                        .long("no-homebrew")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("no-docker")
                        .long("no-docker")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("exclude")
                        .long("exclude")
                        .action(ArgAction::Append)
                        .value_parser(value_parser!(PathBuf)),
                ),
        )
        .subcommand(
            Command::new("rules").arg(Arg::new("json").long("json").action(ArgAction::SetTrue)),
        )
        .subcommand(
            Command::new("completion").arg(
                Arg::new("shell")
                    .required(true)
                    .value_parser(["bash", "zsh", "fish"]),
            ),
        )
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("sizetrail: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<u8, String> {
    let matches = command().get_matches();
    let Some((subcommand, arguments)) = matches.subcommand() else {
        println!("{}", command().render_long_help());
        return Ok(0);
    };

    match subcommand {
        "scan" => {
            let root = matches
                .get_one::<PathBuf>("root")
                .cloned()
                .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
                .ok_or_else(|| "HOME is unavailable and --root was not supplied".to_owned())?;
            let no_xcode = arguments.get_flag("no-xcode");
            let json_output = arguments.get_flag("json");
            let requested_excludes = arguments
                .get_many::<PathBuf>("exclude")
                .map(|values| values.cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            if no_xcode && !requested_excludes.is_empty() {
                eprintln!("sizetrail: --exclude matches no enabled scan root");
                return Ok(2);
            }
            let opened = Root::open(&root);
            let excludes = if no_xcode {
                Vec::new()
            } else if let Ok(scan_root) = opened.as_ref() {
                match validate_excludes(scan_root, &root, &requested_excludes) {
                    Ok(excludes) => excludes,
                    Err(message) => {
                        eprintln!("sizetrail: {message}");
                        return Ok(2);
                    }
                }
            } else if requested_excludes.is_empty() {
                Vec::new()
            } else {
                eprintln!(
                    "sizetrail: --exclude could not be validated because root initialization failed"
                );
                return Ok(2);
            };
            let mut environment =
                EnvironmentEnvelope::capture(Some(&root)).map_err(|error| error.to_string())?;
            let capacity = opened
                .as_ref()
                .map_or_else(|_| capacity::measure(&root), capacity::measure_root);
            let xcode = if no_xcode {
                excluded_adapter_report("xcode")
            } else if let Ok(scan_root) = opened.as_ref() {
                let mut ctx = PolicyCtx::for_scan();
                if json_output {
                    xcode_report(scan_root, &mut ctx, &excludes)
                } else {
                    xcode_report_with_sink(scan_root, &mut ctx, &excludes, |finding| {
                        println!(
                            "{}\t{}\t{}",
                            finding.id, finding.normalized_path, finding.summary
                        );
                    })
                }
            } else {
                unmeasurable_adapter_report("xcode")
            };
            if let Some(version) = &xcode.tool_version {
                environment
                    .tool_versions
                    .insert("xcode".to_owned(), version.clone());
            }
            let document = scan(environment, capacity, vec![xcode]);

            if json_output {
                let rendered =
                    serde_json::to_string(&document).map_err(|error| error.to_string())?;
                println!("{rendered}");
            } else {
                if document.payload.findings.is_empty() {
                    println!("No Xcode storage findings were observed.");
                }
            }

            Ok(
                if document
                    .payload
                    .regions
                    .iter()
                    .any(|region| region.status == sizetrail::model::RegionStatus::Unmeasurable)
                {
                    3
                } else {
                    0
                },
            )
        }
        "rules" => {
            let rules = builtin_rules().map_err(|_| "compiled rule set is invalid".to_owned())?;
            if arguments.get_flag("json") {
                println!(
                    "{}",
                    serde_json::to_string(&rules).map_err(|error| error.to_string())?
                );
            } else {
                for rule in rules {
                    println!("{}\t{}", rule.id, rule.title);
                }
            }
            Ok(0)
        }
        "completion" => {
            let shell = match arguments.get_one::<String>("shell").map(String::as_str) {
                Some("bash") => Shell::Bash,
                Some("zsh") => Shell::Zsh,
                Some("fish") => Shell::Fish,
                _ => return Ok(2),
            };
            let mut application = command();
            generate(shell, &mut application, "sizetrail", &mut io::stdout());
            Ok(0)
        }
        "doctor" => doctor(&matches, arguments),
        "explain" => explain(&matches, arguments),
        _ => unreachable!("clap only returns declared subcommands"),
    }
}

fn doctor(matches: &clap::ArgMatches, arguments: &clap::ArgMatches) -> Result<u8, String> {
    let root = matches
        .get_one::<PathBuf>("root")
        .cloned()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .ok_or_else(|| "HOME is unavailable and --root was not supplied".to_owned())?;
    let no_xcode = arguments.get_flag("no-xcode");
    let requested_excludes = arguments
        .get_many::<PathBuf>("exclude")
        .map(|values| values.cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    if no_xcode && !requested_excludes.is_empty() {
        eprintln!("sizetrail: --exclude matches no enabled scan root");
        return Ok(2);
    }
    let opened = Root::open(&root);
    let root_ready = opened.is_ok();
    let xcode = if no_xcode {
        excluded_adapter_report("xcode")
    } else if let Ok(scan_root) = opened.as_ref() {
        let excludes = match validate_excludes(scan_root, &root, &requested_excludes) {
            Ok(excludes) => excludes,
            Err(message) => {
                eprintln!("sizetrail: {message}");
                return Ok(2);
            }
        };
        let mut ctx = PolicyCtx::for_scan();
        xcode_report(scan_root, &mut ctx, &excludes)
    } else {
        unmeasurable_adapter_report("xcode")
    };
    let ready = root_ready
        && matches!(
            xcode.status,
            sizetrail::model::RegionStatus::Complete
                | sizetrail::model::RegionStatus::NotPresent
                | sizetrail::model::RegionStatus::ExcludedByUser
        );
    let report = serde_json::json!({
        "schema_version": sizetrail::model::SCHEMA_VERSION,
        "tool_version": sizetrail::model::TOOL_VERSION,
        "root": {"status": if root_ready { "readable" } else { "unmeasurable" }},
        "launcher_hint": launcher_hint(),
        "side_effect_policy": side_effect_policy(),
        "remediation": (!ready).then(|| serde_json::json!({
            "settings_command": "open 'x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_AllFiles'",
            "execution": "user_only",
            "caveat": "This opens the privacy settings page; it does not prove that Full Disk Access is the cause or the remedy."
        })),
        "xcode": xcode,
    });
    if arguments.get_flag("json") {
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "root: {}\nxcode: {}",
            report["root"]["status"].as_str().unwrap_or("unmeasurable"),
            report["xcode"]["status"].as_str().unwrap_or("unmeasurable")
        );
    }
    Ok(if ready { 0 } else { 3 })
}

fn side_effect_policy() -> Vec<serde_json::Value> {
    SIDE_EFFECT_REGISTRY
        .iter()
        .map(|policy| {
            serde_json::json!({
                "probe_id": policy.id.as_str(),
                "max_calls_per_scan": policy.max_calls_per_scan,
                "disable_environment": policy.disable_env,
                "known_side_effects": policy.known_side_effects,
                "program": policy.command.program,
                "arguments": policy.command.arguments,
                "timeout_millis": policy.command.timeout_millis,
            })
        })
        .collect()
}

fn launcher_hint() -> serde_json::Value {
    if let Some(candidate) = std::env::var_os("TERM_PROGRAM") {
        serde_json::json!({
            "context": if std::env::var_os("SSH_CONNECTION").is_some() { "ssh" } else { "interactive_tty" },
            "candidate": candidate.to_string_lossy(),
            "confidence": "unverified",
            "evidence": ["TERM_PROGRAM"]
        })
    } else {
        serde_json::json!({
            "context": if std::env::var_os("SSH_CONNECTION").is_some() { "ssh" } else { "unknown" },
            "confidence": "none",
            "evidence": []
        })
    }
}

fn explain(matches: &clap::ArgMatches, arguments: &clap::ArgMatches) -> Result<u8, String> {
    let id = arguments
        .get_one::<String>("finding-id")
        .ok_or_else(|| "finding id is required".to_owned())?;
    if !id.starts_with("f1:") {
        return Err("unknown finding ID algorithm version".to_owned());
    }
    if let Some(source) = arguments.get_one::<PathBuf>("from") {
        let document = read_snapshot(source)?;
        if document["schema_version"] != sizetrail::model::SCHEMA_VERSION {
            return Err("unknown or newer report schema version".to_owned());
        }
        let finding = document["payload"]["findings"]
            .as_array()
            .and_then(|findings| findings.iter().find(|finding| finding["id"] == *id))
            .ok_or_else(|| "finding is absent from the supplied report".to_owned())?;
        return render_explanation(
            finding,
            arguments,
            "snapshot_only",
            document["environment"]["generated_at_unix_seconds"].as_u64(),
        );
    }
    if !id.starts_with("f1:xcode:") {
        return Err("finding belongs to an adapter that is not compiled".to_owned());
    }
    let root = matches
        .get_one::<PathBuf>("root")
        .cloned()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .ok_or_else(|| "HOME is unavailable and --root was not supplied".to_owned())?;
    let environment =
        EnvironmentEnvelope::capture(Some(&root)).map_err(|error| error.to_string())?;
    let capacity = capacity::measure(&root);
    let report = match Root::open(&root) {
        Ok(scan_root) => {
            let mut ctx = PolicyCtx::for_scan();
            xcode_report(&scan_root, &mut ctx, &[])
        }
        Err(_) => unmeasurable_adapter_report("xcode"),
    };
    let rescan_complete = report.status == sizetrail::model::RegionStatus::Complete;
    let rescan_status = report.status;
    let document = scan(environment, capacity, vec![report]);
    let Some(finding) = document
        .payload
        .findings
        .iter()
        .find(|finding| finding.id == *id)
    else {
        let missing = serde_json::json!({
            "status": if rescan_complete { "not_found_after_rescan" } else { "rescan_unmeasurable" },
            "provenance": "live",
            "finding_id": id,
            "adapter_status": rescan_status,
            "coverage_gaps": document.payload.coverage_gaps,
            "note": "This was a new measurement; the finding may have changed since the prior scan."
        });
        if arguments.get_flag("json") {
            println!("{missing}");
        } else {
            if rescan_complete {
                println!("finding no longer exists after a complete live remeasurement");
            } else {
                println!("live remeasurement was incomplete; finding existence is unknown");
            }
        }
        return Ok(3);
    };
    let value = serde_json::to_value(finding).map_err(|error| error.to_string())?;
    render_explanation(&value, arguments, "live", None)
}

fn read_snapshot(source: &Path) -> Result<serde_json::Value, String> {
    let text = if source.as_os_str() == "-" {
        let mut text = String::new();
        io::Read::read_to_string(&mut io::stdin(), &mut text).map_err(|error| error.to_string())?;
        text
    } else {
        let absolute = if source.is_absolute() {
            source.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|error| error.to_string())?
                .join(source)
        };
        let parent = absolute
            .parent()
            .ok_or_else(|| "report path has no parent".to_owned())?;
        let root = Root::open(parent).map_err(|_| "report parent is not readable".to_owned())?;
        let physical = root.path().join(
            absolute
                .file_name()
                .ok_or_else(|| "report path has no file name".to_owned())?,
        );
        let measured = root
            .measure_object(&physical)
            .map_err(|_| "report file is not safely readable".to_owned())?;
        if measured.dataless {
            return Err("dataless report files are not materialized".to_owned());
        }
        fs::read_to_string(physical).map_err(|error| error.to_string())?
    };
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

fn render_explanation(
    finding: &serde_json::Value,
    arguments: &clap::ArgMatches,
    provenance: &str,
    report_time: Option<u64>,
) -> Result<u8, String> {
    if arguments.get_flag("path") {
        println!(
            "{}",
            finding["normalized_path"]
                .as_str()
                .ok_or_else(|| "finding has no normalized path".to_owned())?
        );
    } else if arguments.get_flag("json") {
        println!(
            "{}",
            serde_json::json!({
                "provenance": provenance,
                "report_time_unix_seconds": report_time,
                "finding": finding,
            })
        );
    } else {
        println!("provenance: {provenance}");
        if provenance == "live" {
            println!("This is a new measurement and may differ from the prior scan.");
        } else {
            println!("This is snapshot-only evidence; the current path may have changed.");
        }
        println!(
            "{}",
            serde_json::to_string_pretty(finding).map_err(|error| error.to_string())?
        );
    }
    Ok(0)
}

fn validate_excludes(
    root: &Root,
    requested_root: &Path,
    requested: &[PathBuf],
) -> Result<Vec<PathBuf>, String> {
    let mut excludes = Vec::new();
    for input in requested {
        if input
            .as_os_str()
            .to_string_lossy()
            .chars()
            .any(|character| matches!(character, '*' | '?' | '[' | ']'))
        {
            return Err(format!(
                "exclude does not accept glob syntax: {}",
                input.display()
            ));
        }
        let candidate = if input.is_absolute() {
            if let Ok(relative) = input.strip_prefix(requested_root) {
                root.path().join(relative)
            } else {
                input.clone()
            }
        } else {
            root.path().join(input)
        };
        if candidate
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
            || !candidate.starts_with(root.path())
        {
            return Err(format!(
                "exclude must be a normalized path under the scan root: {}",
                input.display()
            ));
        }
        let xcode_root = root.path().join("Library/Developer/Xcode");
        let simulator_root = root.path().join("Library/Developer/CoreSimulator");
        if !overlaps(&candidate, &xcode_root) && !overlaps(&candidate, &simulator_root) {
            return Err(format!(
                "exclude matches no Xcode scan root: {}",
                input.display()
            ));
        }
        match root.path_exists_without_descending(&candidate) {
            Ok(true) => excludes.push(candidate),
            Ok(false) => return Err(format!("exclude does not exist: {}", input.display())),
            Err(_) => {
                return Err(format!(
                    "exclude could not be validated: {}",
                    input.display()
                ));
            }
        }
    }
    excludes.sort();
    excludes.dedup();
    Ok(excludes)
}

fn overlaps(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}
