// SPDX-License-Identifier: FSL-1.1-ALv2

use anyhow::Context;
use clap::{CommandFactory, Parser, Subcommand, error::ErrorKind};
use hoststamp::{
    SERVICE_NAME,
    config::{self, Overrides},
    credits, server,
};
use std::{net::SocketAddr, path::PathBuf};

#[derive(Parser, Debug)]
#[command(version, about = "Hoststamp CLI, API server, and local UX.")]
struct Cli {
    /// Print license and attribution information.
    #[arg(long, global = true)]
    credits: bool,

    /// Path to the Hoststamp config file.
    #[arg(long, global = true, env = "HOSTSTAMP_CONFIG", value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the API server and local UX.
    Serve {
        /// Address the server should bind to.
        #[arg(long)]
        addr: Option<SocketAddr>,
    },
    /// Print a local health payload.
    Health,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    if cli.credits {
        println!("{}", credits::text());
        return Ok(());
    }

    let command = cli.command.unwrap_or_else(|| {
        Cli::command()
            .error(
                ErrorKind::MissingSubcommand,
                "a command is required unless --credits is used",
            )
            .exit()
    });

    match command {
        Command::Serve { addr } => {
            let settings = config::load(Overrides {
                config_path: cli.config,
                addr,
            })?;
            tracing::info!(
                addr = %settings.addr,
                config = ?settings.config_path,
                "starting {SERVICE_NAME}"
            );
            server::serve(settings.addr).await.context("server failed")
        }
        Command::Health => {
            println!("{}", serde_json::to_string(&server::health_payload())?);
            Ok(())
        }
    }
}
