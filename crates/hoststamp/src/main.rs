// SPDX-License-Identifier: FSL-1.1-ALv2

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use hoststamp::{
    SERVICE_NAME,
    config::{self, Overrides},
    credits, dictionary,
    generator::{self, GenerateOptions, ProfileGeneratedHostname},
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

    /// Minimum number of lowercase alphanumeric characters in the suffix.
    #[arg(long, global = true, value_parser = generator::parse_suffix_min_length)]
    suffix_min_length: Option<usize>,

    /// Number of hostnames to generate.
    #[arg(long, global = true, value_parser = generator::parse_count)]
    count: Option<usize>,

    /// Print generated hostnames as JSON with metadata.
    #[arg(long, global = true)]
    json: bool,
}

impl GenerateArgs {
    fn has_generation_request_options(&self) -> bool {
        self.capacity
            || self.no_word1
            || self.word1_lengths.is_some()
            || self.word1_categories.is_some()
            || self.no_word2
            || self.word2_lengths.is_some()
            || self.word2_categories.is_some()
            || self.no_suffix
            || self.suffix_min_length.is_some()
            || self.count.is_some()
    }

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
            suffix_min_length: self.suffix_min_length.unwrap_or(base.suffix_min_length),
            count: self.count.unwrap_or(base.count),
        })
    }
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate hostnames.
    Generate,
    /// Regenerate a profile-backed hostname from an atomic value.
    Regenerate {
        /// Atomic value to regenerate.
        #[arg(long, value_parser = parse_atomic_value)]
        atomic_value: i64,
    },
    /// Inspect or manage Hoststamp configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Inspect or manage Hoststamp profiles.
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
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

#[derive(Subcommand, Debug)]
enum ProfileCommand {
    /// List active profiles.
    List,
    /// Show the selected active profile.
    Show,
    /// Create the selected profile with default generator settings.
    New,
    /// Delete the selected active profile.
    Delete,
    /// Reset the selected profile's stored atomic value.
    ResetAtomicValue {
        /// Stored atomic value to reset to.
        #[arg(long, value_parser = parse_stored_atomic_value)]
        atomic_value: i64,
    },
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
        Command::Profile { command } => {
            let settings = config::load(Overrides {
                config_path: cli.config.clone(),
                addr: None,
                database_url: cli.database_url.clone(),
            })?;
            let mut store = ProfileStore::open(&settings.database_url)?;
            match command {
                ProfileCommand::List => {
                    print_profile_list(&store.list_profiles()?);
                    Ok(())
                }
                ProfileCommand::Show => {
                    let profile = store.load_profile(&cli.profile)?;
                    print_profile_show(&profile);
                    Ok(())
                }
                ProfileCommand::New => {
                    let profile = store.create_profile(&cli.profile, &ProfileConfig::default())?;
                    print_profile_show(&profile);
                    Ok(())
                }
                ProfileCommand::Delete => {
                    let profile = store.load_profile(&cli.profile)?;
                    confirm_profile_delete(&profile)?;
                    store.delete_profile(&cli.profile)?;
                    println!("deleted profile {:?}", cli.profile.as_str());
                    Ok(())
                }
                ProfileCommand::ResetAtomicValue { atomic_value } => {
                    let profile = store.load_profile(&cli.profile)?;
                    confirm_atomic_value_reset(&profile, atomic_value)?;
                    let profile = store.reset_atomic_value(&cli.profile, atomic_value)?;
                    print_profile_show(&profile);
                    Ok(())
                }
            }
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
            generator::validate_generate_options(&options)?;
            let profile = reconcile_atomic_profile(&mut store, profile, &options)?;
            let hostnames = generate_with_profile(options, &mut store, &profile)?;
            if cli.generate.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&server::GenerateResponse { hostnames })?
                );
            } else {
                for generated in hostnames {
                    println!("{}", generated.hostname);
                }
            }
            Ok(())
        }
        Command::Regenerate { atomic_value } => {
            if cli.generate.has_generation_request_options() {
                anyhow::bail!(
                    "regenerate only supports --profile and --atomic-value; generation options are ignored by design"
                );
            }
            let settings = config::load(Overrides {
                config_path: cli.config.clone(),
                addr: None,
                database_url: cli.database_url.clone(),
            })?;
            let mut store = ProfileStore::open(&settings.database_url)?;
            let profile = store.load_or_seed_profile(&cli.profile, &ProfileConfig::default())?;
            if !profile.config.suffix.enabled {
                anyhow::bail!(
                    "profile {:?} cannot regenerate hostnames because suffixes are disabled; atomic values are only tracked when suffixes are enabled",
                    profile.slug.as_str()
                );
            }
            if atomic_value > profile.last_atomic_value {
                anyhow::bail!(
                    "profile {:?} has issued {} atomic values; {} was never generated",
                    profile.slug.as_str(),
                    profile.last_atomic_value,
                    atomic_value
                );
            }
            ensure_profile_dictionary_is_current(&profile)?;
            let options = profile.config.to_generate_options(generator::DEFAULT_COUNT);
            generator::validate_generate_options(&options)?;
            let hostname = generator::generate_profile_hostname(
                &options,
                profile.id,
                &profile.config_hash,
                atomic_value,
            )?;
            if cli.generate.json {
                let generated = server::GeneratedHostname::profile_backed(
                    &profile.slug,
                    ProfileGeneratedHostname {
                        hostname,
                        atomic_value,
                    },
                );
                println!(
                    "{}",
                    serde_json::to_string_pretty(&server::GenerateResponse {
                        hostnames: vec![generated],
                    })?
                );
            } else {
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
            generator::validate_generate_options(&options)?;
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
) -> anyhow::Result<Vec<server::GeneratedHostname>> {
    if options.suffix_enabled {
        let profile_id = profile.id;
        let profile_slug = profile.slug.clone();
        let config_hash = profile.config_hash;
        return generator::generate_profile_many(options, profile_id, &config_hash, || {
            store.increment_atomic_value(&profile_slug)
        })
        .map(|hostnames| {
            hostnames
                .into_iter()
                .map(|hostname| server::GeneratedHostname::profile_backed(&profile_slug, hostname))
                .collect()
        });
    }

    generator::generate_many(options).map(|hostnames| {
        hostnames
            .into_iter()
            .map(server::GeneratedHostname::plain)
            .collect()
    })
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
            "fixed_suffix_variants\t{}",
            format_decimal(report.suffix_variants.as_deref().unwrap_or("0"))
        );
        println!("suffix_bits\t{}", report.suffix_bits.unwrap_or(0));
        if let Some(max_value) = report.random_fallback_max_value {
            println!("random_fallback_min_value\t1");
            println!(
                "random_fallback_max_value\t{}",
                format_decimal(&max_value.to_string())
            );
        } else {
            println!("random_fallback_min_value\tn/a");
            println!("random_fallback_max_value\tn/a");
        }
        if let Some(max_value) = report.atomic_storage_max_value {
            println!("atomic_min_value\t1");
            println!(
                "atomic_storage_max_value\t{}",
                format_decimal(&max_value.to_string())
            );
        } else {
            println!("atomic_min_value\tn/a");
            println!("atomic_storage_max_value\tn/a");
        }
    } else {
        println!("fixed_suffix_variants\tdisabled");
        println!("suffix_bits\t0");
        println!("random_fallback_min_value\tdisabled");
        println!("random_fallback_max_value\tdisabled");
        println!("atomic_min_value\tdisabled");
        println!("atomic_storage_max_value\tdisabled");
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

    print_profile_show(profile);
    println!();
    print_generate_options("effective.generate", options);
}

fn print_profile_list(profiles: &[StoredProfile]) {
    println!("slug\tid\tlast_atomic_value");
    for profile in profiles {
        println!(
            "{}\t{}\t{}",
            profile.slug.as_str(),
            profile.id,
            profile.last_atomic_value
        );
    }
}

fn print_profile_show(profile: &StoredProfile) {
    println!("[profile]");
    println!("slug = {:?}", profile.slug.as_str());
    println!("id = {:?}", profile.id.to_string());
    println!("last_atomic_value = {}", profile.last_atomic_value);
    println!("config_hash = {:?}", hex_string(&profile.config_hash));
    println!();
    print_profile_config("profile.config", &profile.config);
}

fn print_profile_config(prefix: &str, config: &ProfileConfig) {
    println!("[{prefix}]");
    println!(
        "dictionary_fingerprint = {:?}",
        config.dictionary_fingerprint
    );
    println!();

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
    println!("min_length = {}", config.suffix.min_length);
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
    println!("min_length = {}", options.suffix_min_length);
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
    if !options.suffix_enabled {
        return Ok(profile);
    }

    let desired_config = ProfileConfig::from(options);
    if desired_config == profile.config {
        return Ok(profile);
    }

    confirm_atomic_profile_replacement(&profile)?;
    store.replace_profile_config(&profile.slug, &desired_config)
}

fn ensure_profile_dictionary_is_current(profile: &StoredProfile) -> anyhow::Result<()> {
    if profile.config.uses_current_dictionary() {
        return Ok(());
    }

    anyhow::bail!(
        "profile {:?} was created with dictionary artifact {}, but this binary uses {}; regenerate cannot run safely across dictionary changes",
        profile.slug.as_str(),
        profile.config.dictionary_fingerprint,
        dictionary::artifact_sha256()
    )
}

fn parse_atomic_value(value: &str) -> Result<i64, String> {
    let atomic_value = value
        .parse::<i64>()
        .map_err(|source| format!("invalid atomic value {value:?}: {source}"))?;
    if atomic_value < generator::ATOMIC_MIN_VALUE {
        return Err(format!(
            "atomic value must be at least {}",
            generator::ATOMIC_MIN_VALUE
        ));
    }
    Ok(atomic_value)
}

fn parse_stored_atomic_value(value: &str) -> Result<i64, String> {
    let atomic_value = value
        .parse::<i64>()
        .map_err(|source| format!("invalid atomic value {value:?}: {source}"))?;
    if atomic_value < 0 {
        return Err("atomic value must be at least 0".to_owned());
    }
    Ok(atomic_value)
}

fn confirm_profile_delete(profile: &StoredProfile) -> anyhow::Result<()> {
    let mut stderr = io::stderr();
    writeln!(
        stderr,
        "Deleting profile {:?} removes it from active profile selection.",
        profile.slug.as_str()
    )?;
    writeln!(
        stderr,
        "Previously generated names can no longer be regenerated through the active profile slug."
    )?;
    write!(
        stderr,
        "Type the profile slug ({}) to continue: ",
        profile.slug.as_str()
    )?;
    stderr.flush()?;

    let mut first = String::new();
    if io::stdin().read_line(&mut first)? == 0 {
        anyhow::bail!("profile deletion requires interactive confirmation");
    }
    if first.trim() != profile.slug.as_str() {
        anyhow::bail!("profile deletion cancelled");
    }

    write!(stderr, "Type delete to confirm profile deletion: ")?;
    stderr.flush()?;

    let mut second = String::new();
    if io::stdin().read_line(&mut second)? == 0 {
        anyhow::bail!("profile deletion requires interactive confirmation");
    }
    if !second.trim().eq_ignore_ascii_case("delete") {
        anyhow::bail!("profile deletion cancelled");
    }

    Ok(())
}

fn confirm_atomic_value_reset(profile: &StoredProfile, atomic_value: i64) -> anyhow::Result<()> {
    let mut stderr = io::stderr();
    writeln!(
        stderr,
        "Resetting profile {:?} changes its stored atomic value from {} to {}.",
        profile.slug.as_str(),
        profile.last_atomic_value,
        atomic_value
    )?;
    writeln!(
        stderr,
        "Lower values can duplicate previously issued names; higher values skip part of the sequence."
    )?;
    if atomic_value == i64::MAX {
        writeln!(
            stderr,
            "The next profile-backed generation will fail because the atomic counter is exhausted."
        )?;
    } else {
        writeln!(
            stderr,
            "The next profile-backed generation will use atomic value {}.",
            atomic_value + 1
        )?;
    }
    write!(
        stderr,
        "Type the profile slug ({}) to continue: ",
        profile.slug.as_str()
    )?;
    stderr.flush()?;

    let mut first = String::new();
    if io::stdin().read_line(&mut first)? == 0 {
        anyhow::bail!("atomic value reset requires interactive confirmation");
    }
    if first.trim() != profile.slug.as_str() {
        anyhow::bail!("atomic value reset cancelled");
    }

    write!(stderr, "Type reset to confirm atomic value reset: ")?;
    stderr.flush()?;

    let mut second = String::new();
    if io::stdin().read_line(&mut second)? == 0 {
        anyhow::bail!("atomic value reset requires interactive confirmation");
    }
    if !second.trim().eq_ignore_ascii_case("reset") {
        anyhow::bail!("atomic value reset cancelled");
    }

    Ok(())
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
    if io::stdin().read_line(&mut first)? == 0 {
        anyhow::bail!(
            "profile replacement requires interactive confirmation; re-run interactively, use matching profile options, or pass --no-suffix"
        );
    }
    if first.trim() != profile.slug.as_str() {
        anyhow::bail!("profile replacement cancelled");
    }

    write!(stderr, "Type replace to confirm profile replacement: ")?;
    stderr.flush()?;

    let mut second = String::new();
    if io::stdin().read_line(&mut second)? == 0 {
        anyhow::bail!(
            "profile replacement requires interactive confirmation; re-run interactively, use matching profile options, or pass --no-suffix"
        );
    }
    if !second.trim().eq_ignore_ascii_case("replace") {
        anyhow::bail!("profile replacement cancelled");
    }

    Ok(())
}
