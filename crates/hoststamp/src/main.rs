// SPDX-License-Identifier: FSL-1.1-ALv2

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use hoststamp::{
    SERVICE_NAME,
    config::{self, Overrides},
    credits,
    generator::{self, Dictionary, GenerateOptions},
    server,
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

    #[command(flatten)]
    generate: GenerateArgs,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Args, Debug, Clone, Copy)]
struct GenerateArgs {
    /// Number of words in each hostname.
    #[arg(long, global = true, default_value_t = generator::DEFAULT_WORDS, value_parser = generator::parse_words)]
    words: usize,

    /// Exact word length to use.
    #[arg(long, global = true, default_value_t = generator::DEFAULT_WORD_LENGTH, value_parser = generator::parse_word_length)]
    word_length: usize,

    /// Dictionary to use: eff_short, eff_short_2, or eff_large.
    #[arg(long, global = true, default_value_t = Dictionary::Short, value_parser = generator::parse_dictionary)]
    dictionary: Dictionary,

    /// Number of hostnames to generate.
    #[arg(long, global = true, default_value_t = generator::DEFAULT_COUNT, value_parser = generator::parse_count)]
    count: usize,

    /// Disable the suffix hash segment.
    #[arg(long, global = true)]
    no_suffix_hash: bool,

    /// Number of hex characters to include in the suffix hash.
    #[arg(long, global = true, default_value_t = generator::DEFAULT_SUFFIX_LEN, value_parser = generator::parse_suffix_len)]
    suffix_len: usize,
}

impl GenerateArgs {
    fn options(self) -> GenerateOptions {
        GenerateOptions {
            words: self.words,
            word_length: Some(self.word_length),
            dictionary: self.dictionary,
            suffix_hash: !self.no_suffix_hash,
            suffix_len: self.suffix_len,
        }
    }
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate random hostnames.
    Generate,
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

    let command = cli.command.unwrap_or(Command::Generate);

    match command {
        Command::Generate => {
            let count = cli.generate.count;
            let options = cli.generate.options();

            for hostname in generator::generate_many(options, count)? {
                println!("{hostname}");
            }

            Ok(())
        }
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
            server::serve(settings.addr, cli.generate.options())
                .await
                .context("server failed")
        }
        Command::Health => {
            println!("{}", serde_json::to_string(&server::health_payload())?);
            Ok(())
        }
    }
}
