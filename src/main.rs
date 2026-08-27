#![deny(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::unwrap_used
)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use sizetrail::model::EnvironmentEnvelope;
use sizetrail::scan::scan;

#[derive(Parser)]
#[command(name = "sizetrail", disable_version_flag = true)]
struct Cli {
    #[arg(long, global = true, hide = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Scan {
        #[arg(long)]
        json: bool,
    },
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
    let cli = Cli::parse();
    let Some(command) = cli.command else {
        println!("{}", Cli::command().render_long_help());
        return Ok(0);
    };

    match command {
        Commands::Scan { json } => {
            let environment = EnvironmentEnvelope::capture(cli.root.as_deref())
                .map_err(|error| error.to_string())?;
            let document = scan(environment);

            if json {
                let rendered =
                    serde_json::to_string(&document).map_err(|error| error.to_string())?;
                println!("{rendered}");
            } else {
                println!("No toolchain adapters are compiled; attribution is unmeasurable.");
            }

            Ok(0)
        }
    }
}
