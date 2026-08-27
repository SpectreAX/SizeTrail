#![forbid(clippy::disallowed_methods)]
#![forbid(
    clippy::let_underscore_must_use,
    clippy::let_underscore_untyped,
    clippy::unwrap_used
)]
#![deny(clippy::disallowed_types)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Arg, ArgAction, Command, value_parser};
use sizetrail::capacity;
use sizetrail::model::EnvironmentEnvelope;
use sizetrail::scan::scan;

fn command() -> Command {
    Command::new("sizetrail")
        .disable_version_flag(true)
        .arg(
            Arg::new("root")
                .long("root")
                .global(true)
                .hide(true)
                .value_parser(value_parser!(PathBuf)),
        )
        .subcommand(
            Command::new("scan").arg(Arg::new("json").long("json").action(ArgAction::SetTrue)),
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
            let environment = EnvironmentEnvelope::capture(
                matches.get_one::<PathBuf>("root").map(PathBuf::as_path),
            )
            .map_err(|error| error.to_string())?;
            let root = matches
                .get_one::<PathBuf>("root")
                .cloned()
                .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
                .ok_or_else(|| "HOME is unavailable and --root was not supplied".to_owned())?;
            let document = scan(environment, capacity::measure(&root));

            if arguments.get_flag("json") {
                let rendered =
                    serde_json::to_string(&document).map_err(|error| error.to_string())?;
                println!("{rendered}");
            } else {
                println!("No toolchain adapters are compiled; attribution is unmeasurable.");
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
        _ => unreachable!("clap only returns declared subcommands"),
    }
}
