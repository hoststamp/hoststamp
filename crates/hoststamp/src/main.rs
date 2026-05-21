// SPDX-License-Identifier: FSL-1.1-ALv2

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use hoststamp::{
    SERVICE_NAME,
    config::{self, Overrides},
    credits, dictionary,
    generator::{self, GenerateOptions, SuffixHash, SuffixSource},
    notices,
    profile::{self, ProfileConfig, ProfileSlug},
    server,
    storage::{self, ProfileStore, StoredProfile},
};
use std::{
    io::{self, Write},
    net::SocketAddr,
    path::PathBuf,
};

#[derive(Parser, Debug)]
#[command(version, about = "Hoststamp CLI, API server, and local UX.")]
struct Cli {
    /// Print license and attribution information.
    #[arg(long, global = true)]
    credits: bool,

    /// Path to the Hoststamp config file.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Database URL for Hoststamp profiles.
    #[arg(long, global = true, value_name = "URL")]
    database_url: Option<String>,

    /// Profile slug to use for generation defaults.
    #[arg(
        long,
        global = true,
        default_value = profile::DEFAULT_PROFILE_SLUG,
        value_parser = profile::parse_profile_slug
    )]
    profile: ProfileSlug,

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

    /// Print capacity for the selected generation options.
    #[arg(long, global = true)]
    capacity: bool,

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
    /// Inspect or manage Hoststamp configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
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

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    /// Print the resolved bootstrap and profile configuration.
    Show,
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
        Command::Config {
            command: ConfigCommand::Show,
        } => {
            let settings = config::load(Overrides {
                config_path: cli.config.clone(),
                addr: None,
                database_url: cli.database_url.clone(),
            })?;
            let mut store = ProfileStore::open(&settings.database_url)?;
            let profile = store.load_or_seed_profile(&cli.profile, &ProfileConfig::default())?;
            let base = profile.config.to_generate_options(generator::DEFAULT_COUNT);
            let options = cli.generate.options(base)?;
            print_config_show(&settings, &profile, &options);
            Ok(())
        }
        Command::Generate => {
            let settings = config::load(Overrides {
                config_path: cli.config.clone(),
                addr: None,
                database_url: cli.database_url.clone(),
            })?;
            let mut store = ProfileStore::open(&settings.database_url)?;
            let profile = store.load_or_seed_profile(&cli.profile, &ProfileConfig::default())?;
            let base = profile.config.to_generate_options(generator::DEFAULT_COUNT);
            let options = cli.generate.options(base)?;
            if cli.generate.capacity {
                print_capacity_report(&options)?;
                return Ok(());
            }
            let profile = reconcile_atomic_profile(&mut store, profile, &options)?;
            for hostname in generate_with_profile(options, &mut store, &profile)? {
                println!("{hostname}");
            }
            Ok(())
        }
        Command::Serve { addr } => {
            let settings = config::load(Overrides {
                config_path: cli.config.clone(),
                addr,
                database_url: cli.database_url.clone(),
            })?;
            tracing::info!(
                addr = %settings.addr,
                config = ?settings.config_path,
                "starting {SERVICE_NAME}"
            );
            let mut store = ProfileStore::open(&settings.database_url)?;
            let profile = store.load_or_seed_profile(&cli.profile, &ProfileConfig::default())?;
            let base = profile.config.to_generate_options(generator::DEFAULT_COUNT);
            let options = cli.generate.options(base)?;
            if cli.generate.capacity {
                print_capacity_report(&options)?;
                return Ok(());
            }
            let profile = reconcile_atomic_profile(&mut store, profile, &options)?;
            let atomic = server::AtomicContext::new(store, profile.slug);
            server::serve_with_atomic(settings.addr, options, atomic)
                .await
                .context("server failed")
        }
    }
}

fn generate_with_profile(
    options: GenerateOptions,
    store: &mut ProfileStore,
    profile: &StoredProfile,
) -> anyhow::Result<Vec<String>> {
    if options.suffix_enabled && options.suffix_source == SuffixSource::Atomic {
        let profile_id = profile.id;
        let profile_slug = profile.slug.clone();
        let suffix_hash = options.suffix_hash;
        let suffix_length = options.suffix_length;
        return generator::generate_many_with_atomic_suffix(options, || {
            let atomic_value = store.increment_atomic_value(&profile_slug)?;
            generator::compute_atomic_suffix(profile_id, atomic_value, suffix_hash, suffix_length)
        });
    }

    generator::generate_many(options)
}

fn print_capacity_report(options: &GenerateOptions) -> anyhow::Result<()> {
    let report = generator::capacity_report(options)?;

    println!(
        "word1_words\t{}",
        report
            .word1_count
            .map(format_usize)
            .unwrap_or_else(|| "disabled".to_owned())
    );
    println!(
        "word2_words\t{}",
        report
            .word2_count
            .map(format_usize)
            .unwrap_or_else(|| "disabled".to_owned())
    );
    println!(
        "overlapping_words\t{}",
        format_usize(report.overlapping_words)
    );
    println!(
        "unique_word_combinations\t{}",
        format_u128(report.unique_word_combinations)
    );
    if report.suffix_enabled {
        println!(
            "suffix_variants\t{}",
            format_decimal(report.suffix_variants.as_deref().unwrap_or("0"))
        );
        println!("suffix_bits\t{}", report.suffix_bits.unwrap_or(0));
    } else {
        println!("suffix_variants\tdisabled");
        println!("suffix_bits\t0");
    }
    println!("total_variants\t{}", format_decimal(&report.total_variants));

    Ok(())
}

fn format_usize(value: usize) -> String {
    format_decimal(&value.to_string())
}

fn format_u128(value: u128) -> String {
    format_decimal(&value.to_string())
}

fn format_decimal(value: &str) -> String {
    let mut formatted = String::with_capacity(value.len() + value.len() / 3);
    for (index, digit) in value.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted.chars().rev().collect()
}

fn print_config_show(
    settings: &config::Settings,
    profile: &StoredProfile,
    options: &GenerateOptions,
) {
    println!("[bootstrap]");
    println!("server_addr = {:?}", settings.addr.to_string());
    println!(
        "config_path = {:?}",
        settings
            .config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not loaded".to_owned())
    );
    println!(
        "database_url = {:?}",
        format_storage_url(&settings.database_url)
    );
    println!();

    println!("[profile]");
    println!("slug = {:?}", profile.slug.as_str());
    println!("id = {:?}", profile.id.to_string());
    println!("last_atomic_value = {}", profile.last_atomic_value);
    println!("config_hash = {:?}", hex_string(&profile.config_hash));
    println!();

    print_profile_config("profile.config", &profile.config);
    println!();
    print_generate_options("effective.generate", options);
}

fn print_profile_config(prefix: &str, config: &ProfileConfig) {
    println!("[{prefix}.word1]");
    println!("enabled = {}", config.word1.enabled);
    print_lengths("lengths", config.word1.lengths.as_deref());
    print_string_array("categories", &config.word1.categories);
    println!();

    println!("[{prefix}.word2]");
    println!("enabled = {}", config.word2.enabled);
    print_lengths("lengths", config.word2.lengths.as_deref());
    print_string_array("categories", &config.word2.categories);
    println!();

    println!("[{prefix}.suffix]");
    println!("enabled = {}", config.suffix.enabled);
    println!("length = {}", config.suffix.length);
    println!("source = {:?}", config.suffix.source.to_string());
    println!("hash = {:?}", config.suffix.hash.to_string());
}

fn print_generate_options(prefix: &str, options: &GenerateOptions) {
    println!("[{prefix}.word1]");
    println!("enabled = {}", options.word1_enabled);
    print_lengths("lengths", options.word1_lengths.as_deref());
    print_string_array("categories", &options.word1_categories);
    println!();

    println!("[{prefix}.word2]");
    println!("enabled = {}", options.word2_enabled);
    print_lengths("lengths", options.word2_lengths.as_deref());
    print_string_array("categories", &options.word2_categories);
    println!();

    println!("[{prefix}.suffix]");
    println!("enabled = {}", options.suffix_enabled);
    println!("length = {}", options.suffix_length);
    println!("source = {:?}", options.suffix_source.to_string());
    println!("hash = {:?}", options.suffix_hash.to_string());
    println!();

    println!("[{prefix}.request]");
    println!("count = {}", options.count);
}

fn print_lengths(key: &str, lengths: Option<&[usize]>) {
    match lengths {
        Some(lengths) => {
            let values = lengths
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            println!("{key} = [{values}]");
        }
        None => println!("{key} = \"any\""),
    }
}

fn print_string_array(key: &str, values: &[String]) {
    let values = values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    println!("{key} = [{values}]");
}

fn format_storage_url(url: &storage::StorageUrl) -> String {
    match url {
        storage::StorageUrl::Sqlite(path) => format!("sqlite://{}", path.display()),
        storage::StorageUrl::Postgres(_) => "postgres://<redacted>".to_owned(),
    }
}

fn hex_string(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn reconcile_atomic_profile(
    store: &mut ProfileStore,
    profile: StoredProfile,
    options: &GenerateOptions,
) -> anyhow::Result<StoredProfile> {
    if !options.suffix_enabled || options.suffix_source != SuffixSource::Atomic {
        return Ok(profile);
    }

    let desired_config = ProfileConfig::from(options);
    if desired_config == profile.config {
        return Ok(profile);
    }

    confirm_atomic_profile_replacement(&profile)?;
    store.replace_profile_config(&profile.slug, &desired_config)
}

fn confirm_atomic_profile_replacement(profile: &StoredProfile) -> anyhow::Result<()> {
    let mut stderr = io::stderr();
    writeln!(
        stderr,
        "Atomic generation requires a stable profile configuration."
    )?;
    writeln!(
        stderr,
        "Profile {:?} differs from the requested generator options.",
        profile.slug.as_str()
    )?;
    writeln!(
        stderr,
        "Replacing it will create a new profile UUID and reset the atomic counter."
    )?;
    write!(
        stderr,
        "Type the profile slug ({}) to continue: ",
        profile.slug.as_str()
    )?;
    stderr.flush()?;

    let mut first = String::new();
    io::stdin().read_line(&mut first)?;
    if first.trim() != profile.slug.as_str() {
        anyhow::bail!("profile replacement cancelled");
    }

    write!(stderr, "Type replace to confirm profile replacement: ")?;
    stderr.flush()?;

    let mut second = String::new();
    io::stdin().read_line(&mut second)?;
    if !second.trim().eq_ignore_ascii_case("replace") {
        anyhow::bail!("profile replacement cancelled");
    }

    Ok(())
}
