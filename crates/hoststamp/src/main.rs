// SPDX-License-Identifier: FSL-1.1-ALv2

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use hoststamp::{
    SERVICE_NAME,
    config::{self, Overrides},
    credits, dictionary,
    generator::{self, GenerateOptions, SuffixHash, SuffixSource},
    notices, server,
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

#[derive(Args, Debug, Clone)]
struct GenerateArgs {
    /// Print available category names and counts.
    #[arg(long, global = true)]
    list_categories: bool,

    /// Disable the first word position.
    #[arg(long = "no-word1", global = true, action = clap::ArgAction::SetTrue)]
    no_word1: bool,

    /// Allowed lengths for the first word (comma list or "any").
    #[arg(long, global = true, value_name = "LENGTHS")]
    word1_lengths: Option<String>,

    /// Comma-separated categories for the first word position.
    #[arg(long, global = true, value_name = "CATEGORIES")]
    word1_categories: Option<String>,

    /// Disable the second word position.
    #[arg(long = "no-word2", global = true, action = clap::ArgAction::SetTrue)]
    no_word2: bool,

    /// Allowed lengths for the second word (comma list or "any").
    #[arg(long, global = true, value_name = "LENGTHS")]
    word2_lengths: Option<String>,

    /// Comma-separated categories for the second word position.
    #[arg(long, global = true, value_name = "CATEGORIES")]
    word2_categories: Option<String>,

    /// Disable the suffix segment.
    #[arg(long = "no-suffix", global = true, action = clap::ArgAction::SetTrue)]
    no_suffix: bool,

    /// Number of hex characters in the suffix.
    #[arg(long, global = true, value_parser = generator::parse_suffix_length)]
    suffix_length: Option<usize>,

    /// Source for the suffix (random or atomic).
    #[arg(long, global = true, value_parser = generator::parse_suffix_source)]
    suffix_source: Option<SuffixSource>,

    /// Hash algorithm for the suffix.
    #[arg(long, global = true, value_parser = generator::parse_suffix_hash)]
    suffix_hash: Option<SuffixHash>,

    /// Number of hostnames to generate.
    #[arg(long, global = true, value_parser = generator::parse_count)]
    count: Option<usize>,
}

impl GenerateArgs {
    fn options(&self, base: GenerateOptions) -> anyhow::Result<GenerateOptions> {
        let word1_categories = match self.word1_categories.as_deref() {
            Some(value) => generator::parse_categories(value).map_err(anyhow::Error::msg)?,
            None => base.word1_categories.clone(),
        };
        let word2_categories = match self.word2_categories.as_deref() {
            Some(value) => generator::parse_categories(value).map_err(anyhow::Error::msg)?,
            None => base.word2_categories.clone(),
        };
        let word1_lengths = match self.word1_lengths.as_deref() {
            Some(value) => generator::parse_lengths(value).map_err(anyhow::Error::msg)?,
            None => base.word1_lengths.clone(),
        };
        let word2_lengths = match self.word2_lengths.as_deref() {
            Some(value) => generator::parse_lengths(value).map_err(anyhow::Error::msg)?,
            None => base.word2_lengths.clone(),
        };

        Ok(GenerateOptions {
            word1_enabled: if self.no_word1 {
                false
            } else {
                base.word1_enabled
            },
            word1_lengths,
            word1_categories,
            word2_enabled: if self.no_word2 {
                false
            } else {
                base.word2_enabled
            },
            word2_lengths,
            word2_categories,
            suffix_enabled: if self.no_suffix {
                false
            } else {
                base.suffix_enabled
            },
            suffix_length: self.suffix_length.unwrap_or(base.suffix_length),
            suffix_source: self.suffix_source.unwrap_or(base.suffix_source),
            suffix_hash: self.suffix_hash.unwrap_or(base.suffix_hash),
            count: self.count.unwrap_or(base.count),
        })
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
    /// Print generated third-party notices.
    #[command(hide = true)]
    Notices,
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

    if cli.generate.list_categories {
        for category in dictionary::category_names() {
            let count = dictionary::total_words(category).unwrap_or(0);
            println!("{category}\t{count}");
        }
        return Ok(());
    }

    let command = cli.command.unwrap_or(Command::Generate);

    match command {
        Command::Health => {
            println!("{}", serde_json::to_string(&server::health_payload())?);
            Ok(())
        }
        Command::Notices => {
            print!("{}", notices::text());
            Ok(())
        }
        Command::Generate => {
            let settings = config::load(Overrides {
                config_path: cli.config,
                addr: None,
            })?;
            let options = cli.generate.options(settings.generator)?;
            for hostname in generator::generate_many(options)? {
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
            let options = cli.generate.options(settings.generator)?;
            server::serve(settings.addr, options)
                .await
                .context("server failed")
        }
    }
}
