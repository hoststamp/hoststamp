// SPDX-License-Identifier: FSL-1.1-ALv2

use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
use hoststamp_api as server;
use hoststamp_core::{
    SERVICE_NAME, auth,
    config::{self, Overrides},
    credits, dictionary,
    generator::{self, GenerateOptions, ProfileGeneratedHostname},
    notices,
    profile::{self, ProfileAccess, ProfileConfig, ProfileSlug},
    storage::{self, ProfileStore, StoredProfile, StoredProfileToken},
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

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

    /// Print capacity for the selected profile configuration.
    #[arg(long, global = true)]
    capacity: bool,

    /// Number of hostnames to generate.
    #[arg(long, global = true, value_parser = generator::parse_count)]
    count: Option<usize>,

    /// Print generated hostnames as JSON with metadata.
    #[arg(long, global = true)]
    json: bool,
}

impl GenerateArgs {
    fn has_unsupported_regenerate_options(&self) -> bool {
        self.capacity
    }

    fn options(&self, base: GenerateOptions) -> GenerateOptions {
        GenerateOptions {
            count: self.count.unwrap_or(base.count),
            ..base
        }
    }
}

#[derive(Args, Debug, Clone)]
struct ConfigSetArgs {
    /// Enable or disable the first word position.
    #[arg(long)]
    word1_enabled: Option<bool>,

    /// Allowed lengths for the first word (comma list or "any").
    #[arg(long, value_name = "LENGTHS")]
    word1_lengths: Option<String>,

    /// Comma-separated categories for the first word position.
    #[arg(long, value_name = "CATEGORIES")]
    word1_categories: Option<String>,

    /// Enable or disable the second word position.
    #[arg(long)]
    word2_enabled: Option<bool>,

    /// Allowed lengths for the second word (comma list or "any").
    #[arg(long, value_name = "LENGTHS")]
    word2_lengths: Option<String>,

    /// Comma-separated categories for the second word position.
    #[arg(long, value_name = "CATEGORIES")]
    word2_categories: Option<String>,

    /// Enable or disable the suffix segment.
    #[arg(long)]
    suffix_enabled: Option<bool>,

    /// Minimum number of lowercase alphanumeric characters in the suffix.
    #[arg(long, value_parser = generator::parse_suffix_min_length)]
    suffix_min_length: Option<usize>,
}

impl ConfigSetArgs {
    fn is_empty(&self) -> bool {
        self.word1_enabled.is_none()
            && self.word1_lengths.is_none()
            && self.word1_categories.is_none()
            && self.word2_enabled.is_none()
            && self.word2_lengths.is_none()
            && self.word2_categories.is_none()
            && self.suffix_enabled.is_none()
            && self.suffix_min_length.is_none()
    }

    fn apply(&self, config: ProfileConfig) -> anyhow::Result<ProfileConfig> {
        let mut options = config.to_generate_options(generator::DEFAULT_COUNT);
        if let Some(enabled) = self.word1_enabled {
            options.word1_enabled = enabled;
        }
        if let Some(value) = self.word1_lengths.as_deref() {
            options.word1_lengths = generator::parse_lengths(value).map_err(anyhow::Error::msg)?;
        }
        if let Some(value) = self.word1_categories.as_deref() {
            options.word1_categories =
                generator::parse_categories(value).map_err(anyhow::Error::msg)?;
        }
        if let Some(enabled) = self.word2_enabled {
            options.word2_enabled = enabled;
        }
        if let Some(value) = self.word2_lengths.as_deref() {
            options.word2_lengths = generator::parse_lengths(value).map_err(anyhow::Error::msg)?;
        }
        if let Some(value) = self.word2_categories.as_deref() {
            options.word2_categories =
                generator::parse_categories(value).map_err(anyhow::Error::msg)?;
        }
        if let Some(enabled) = self.suffix_enabled {
            options.suffix_enabled = enabled;
        }
        if let Some(min_length) = self.suffix_min_length {
            options.suffix_min_length = min_length;
        }
        ProfileConfig::try_from_options(&options)
    }
}

#[derive(Args, Debug, Clone)]
struct RandomArgs {
    #[command(flatten)]
    config: ConfigSetArgs,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate hostnames.
    Generate,
    /// Generate stateless random hostnames.
    Random(RandomArgs),
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

        /// Server surfaces to expose.
        #[arg(long, value_enum, default_value_t = ServeMode::All)]
        mode: ServeMode,
    },
    /// Print generated third-party notices.
    #[command(hide = true)]
    Notices,
    /// Print a local health payload.
    Health,
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    /// Create a bootstrap config file without overwriting an existing file.
    Init,
    /// Print the resolved bootstrap and profile configuration.
    Show,
    /// Persist selected generator settings on the active profile.
    Set(ConfigSetArgs),
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
    /// Set the selected profile's API access mode.
    SetAccess {
        /// Profile API access mode.
        #[arg(long, value_parser = parse_profile_access)]
        access: ProfileAccess,
    },
    /// Manage profile-scoped API tokens.
    Token {
        #[command(subcommand)]
        command: ProfileTokenCommand,
    },
    /// Reset the selected profile's stored atomic value.
    ResetAtomicValue {
        /// Stored atomic value to reset to.
        #[arg(long, value_parser = parse_stored_atomic_value)]
        atomic_value: i64,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ServeMode {
    /// Serve API routes and the local UX.
    All,
    /// Serve API routes without the local UX.
    Api,
    /// Serve the local UX without API routes.
    Ux,
}

impl From<ServeMode> for server::AppMode {
    fn from(mode: ServeMode) -> Self {
        match mode {
            ServeMode::All => Self::All,
            ServeMode::Api => Self::Api,
            ServeMode::Ux => Self::Ux,
        }
    }
}

#[derive(Subcommand, Debug)]
enum ProfileTokenCommand {
    /// Create a profile token and print the secret once.
    Create {
        /// Human-readable token name.
        #[arg(long)]
        name: String,
        /// Optional Unix timestamp in milliseconds when this token expires.
        #[arg(long, value_parser = parse_token_expiration)]
        expires_at_ms: Option<i64>,
    },
    /// List profile tokens.
    List,
    /// Revoke a profile token.
    Revoke {
        /// Token ID to revoke.
        token_id: String,
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
        Command::Config { command } => match command {
            ConfigCommand::Init => {
                let path = resolve_config_init_path(cli.config.clone())?;
                write_initial_config(&path)?;
                println!("created config file {}", path.display());
                Ok(())
            }
            ConfigCommand::Show => {
                let (settings, mut store) =
                    load_profile_store(cli.config.clone(), cli.database_url.clone())?;
                let profile =
                    store.load_or_seed_profile(&cli.profile, &ProfileConfig::default())?;
                let base = profile.config.to_generate_options(generator::DEFAULT_COUNT);
                let options = cli.generate.options(base);
                print_config_show(&settings, &profile, &options);
                Ok(())
            }
            ConfigCommand::Set(args) => {
                let (_settings, mut store) =
                    load_profile_store(cli.config.clone(), cli.database_url.clone())?;
                if args.is_empty() {
                    anyhow::bail!("config set requires at least one setting");
                }
                let profile =
                    store.load_or_seed_profile(&cli.profile, &ProfileConfig::default())?;
                let desired_config = args.apply(profile.config.clone())?;
                let options = desired_config.to_generate_options(generator::DEFAULT_COUNT);
                generator::validate_generate_options(&options)?;
                if desired_config == profile.config {
                    print_profile_show(&profile);
                    return Ok(());
                }
                confirm_profile_config_replacement(&profile)?;
                let profile = store.replace_profile_config(&profile.slug, &desired_config)?;
                print_profile_show(&profile);
                Ok(())
            }
        },
        Command::Profile { command } => {
            let (settings, mut store) =
                load_profile_store(cli.config.clone(), cli.database_url.clone())?;
            match command {
                ProfileCommand::List => {
                    print_profile_list(&store.list_profiles()?);
                    Ok(())
                }
                ProfileCommand::Show => {
                    let profile = if cli.profile.as_str() == profile::DEFAULT_PROFILE_SLUG {
                        store.load_or_seed_profile(&cli.profile, &ProfileConfig::default())?
                    } else {
                        store.load_profile(&cli.profile)?
                    };
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
                ProfileCommand::SetAccess { access } => {
                    let profile = store.set_profile_access(&cli.profile, access)?;
                    print_profile_show(&profile);
                    Ok(())
                }
                ProfileCommand::Token { command } => match command {
                    ProfileTokenCommand::Create {
                        name,
                        expires_at_ms,
                    } => {
                        let hash_key = settings.auth.token_hash_key.as_ref().ok_or_else(|| {
                            anyhow::anyhow!(
                                "{} is required to create profile tokens",
                                auth::PROFILE_TOKEN_HASH_KEY_ENV
                            )
                        })?;
                        let profile =
                            store.load_or_seed_profile(&cli.profile, &ProfileConfig::default())?;
                        let generated = auth::generate_profile_token();
                        let token_hash = auth::profile_token_hash(hash_key, &generated.secret)?;
                        let token = store.create_profile_token(
                            profile.id,
                            &generated.token_id,
                            &name,
                            token_hash,
                            expires_at_ms,
                        )?;
                        print_profile_token(&token);
                        println!("token = {:?}", generated.token);
                        Ok(())
                    }
                    ProfileTokenCommand::List => {
                        let profile = store.load_profile(&cli.profile)?;
                        print_profile_token_list(&store.list_profile_tokens(profile.id)?);
                        Ok(())
                    }
                    ProfileTokenCommand::Revoke { token_id } => {
                        let profile = store.load_profile(&cli.profile)?;
                        let token = store.revoke_profile_token(profile.id, &token_id)?;
                        print_profile_token(&token);
                        Ok(())
                    }
                },
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
            let options = cli.generate.options(base);
            if cli.generate.capacity {
                print_capacity_report(&options)?;
                return Ok(());
            }
            ensure_profile_generation_contract_is_current(&profile)?;
            generator::validate_generate_options(&options)?;
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
        Command::Random(args) => {
            let options = cli.generate.options(
                args.config
                    .apply(ProfileConfig::default())?
                    .to_generate_options(generator::DEFAULT_COUNT),
            );
            if cli.generate.capacity {
                print_capacity_report(&options)?;
                return Ok(());
            }
            generator::validate_generate_options(&options)?;
            let hostnames = generator::generate_many(options)?
                .into_iter()
                .map(server::GeneratedHostname::plain)
                .collect::<Vec<_>>();
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
            if cli.generate.has_unsupported_regenerate_options() {
                anyhow::bail!(
                    "regenerate only supports --profile, --atomic-value, --count, and --json"
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
            let count = cli.generate.count.unwrap_or(generator::DEFAULT_COUNT);
            generator::validate_count(count)?;
            let count_offset = i64::try_from(count - 1)?;
            let final_atomic_value = atomic_value
                .checked_add(count_offset)
                .ok_or_else(|| anyhow::anyhow!("atomic value range is too large"))?;
            if final_atomic_value > profile.last_atomic_value {
                anyhow::bail!(
                    "profile {:?} has issued {} atomic values; requested range {}..={} includes values that were never generated",
                    profile.slug.as_str(),
                    profile.last_atomic_value,
                    atomic_value,
                    final_atomic_value
                );
            }
            ensure_profile_generation_contract_is_current(&profile)?;
            let options = profile.config.to_generate_options(count);
            generator::validate_generate_options(&options)?;
            let hostnames = (atomic_value..=final_atomic_value)
                .map(|atomic_value| {
                    let hostname = generator::generate_profile_hostname(
                        &options,
                        profile.id,
                        &profile.config_hash,
                        atomic_value,
                    )?;
                    Ok(server::GeneratedHostname::profile_backed(
                        &profile.slug,
                        ProfileGeneratedHostname {
                            hostname,
                            atomic_value,
                        },
                    ))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
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
        Command::Serve { addr, mode } => {
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
            let options = cli.generate.options(base);
            if cli.generate.capacity {
                print_capacity_report(&options)?;
                return Ok(());
            }
            ensure_profile_generation_contract_is_current(&profile)?;
            generator::validate_generate_options(&options)?;
            let atomic = server::AtomicContext::new(store, profile.slug);
            server::serve_with_atomic_and_mode(
                settings.addr,
                options,
                atomic,
                settings.auth,
                mode.into(),
            )
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

fn load_profile_store(
    config_path: Option<PathBuf>,
    database_url: Option<String>,
) -> anyhow::Result<(config::Settings, ProfileStore)> {
    let settings = config::load(Overrides {
        config_path,
        addr: None,
        database_url,
    })?;
    let store = ProfileStore::open(&settings.database_url)?;
    Ok((settings, store))
}

const INITIAL_CONFIG_TEMPLATE: &str = r#"# Hoststamp bootstrap configuration.
#
# Generator profile settings live in the Hoststamp profile database. This file
# only controls bootstrap settings: server bind address, storage location, and
# API authentication.
#
# Generate 32-character secret values with OpenSSL:
#
#   openssl rand -base64 24
#
# Keep these values in your shell, service manager, container secret store, or
# another secret manager. Do not commit them to source control.

[server]
# addr = "127.0.0.1:8080"

[storage]
# Defaults to hoststamp.db next to this config file.
# url = "sqlite:///home/hoststamp/.config/hoststamp/hoststamp.db"

[api.auth]
# Disabled by default for local development. Set to true before exposing the
# API beyond a trusted local environment.
required = false

# For local single-user setups, uncomment and set direct secret values here.
# For shared systems, keep secrets in environment variables or a secret manager.
# If secrets are stored here, keep this file private with chmod 600.
# admin_token = "replace-with-openssl-output"
# token_hash_key = "replace-with-openssl-output"

# Environment variables override direct secret values when both are present.
admin_token_env = "HOSTSTAMP_ADMIN_TOKEN"
token_hash_key_env = "HOSTSTAMP_TOKEN_HASH_KEY"

# Example:
#   export HOSTSTAMP_ADMIN_TOKEN="$(openssl rand -base64 24)"
#   export HOSTSTAMP_TOKEN_HASH_KEY="$(openssl rand -base64 24)"
"#;

fn resolve_config_init_path(config_arg: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(path) = config_arg {
        return Ok(path);
    }
    if let Some(path) = std::env::var_os(config::CONFIG_ENV) {
        return Ok(PathBuf::from(path));
    }
    config::default_user_config_path().ok_or_else(|| {
        anyhow::anyhow!(
            "cannot determine default config path; pass --config or set {}",
            config::CONFIG_ENV
        )
    })
}

fn write_initial_config(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        anyhow::bail!(
            "config file already exists: {}; refusing to overwrite",
            path.display()
        );
    }

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to create config file {}", path.display()))?;
    file.write_all(INITIAL_CONFIG_TEMPLATE.as_bytes())
        .with_context(|| format!("failed to write config file {}", path.display()))?;
    Ok(())
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
    println!("[api.auth]");
    println!("required = {}", settings.auth.required);
    println!(
        "admin_token_configured = {}",
        settings.auth.admin_token.is_some()
    );
    println!(
        "token_hash_key_configured = {}",
        settings.auth.token_hash_key.is_some()
    );
    println!();

    print_profile_show(profile);
    println!();
    print_generate_options("effective.generate", options);
}

fn print_profile_list(profiles: &[StoredProfile]) {
    println!("slug\tid\taccess\tlast_atomic_value");
    for profile in profiles {
        println!(
            "{}\t{}\t{}\t{}",
            profile.slug.as_str(),
            profile.id,
            profile.access,
            profile.last_atomic_value
        );
    }
}

fn print_profile_show(profile: &StoredProfile) {
    println!("[profile]");
    println!("slug = {:?}", profile.slug.as_str());
    println!("id = {:?}", profile.id.to_string());
    println!("access = {:?}", profile.access.to_string());
    println!("last_atomic_value = {}", profile.last_atomic_value);
    println!("config_hash = {:?}", hex_string(&profile.config_hash));
    println!();
    print_profile_config("profile.config", &profile.config);
}

fn print_profile_token_list(tokens: &[StoredProfileToken]) {
    println!("token_id\tname\tcreated_at_ms\texpires_at_ms\tlast_used_at_ms\trevoked_at_ms");
    for token in tokens {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            token.token_id,
            token.name,
            token.created_at_ms,
            token
                .expires_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_owned()),
            token
                .last_used_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_owned()),
            token
                .revoked_at_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_owned())
        );
    }
}

fn print_profile_token(token: &StoredProfileToken) {
    println!("[profile.token]");
    println!("token_id = {:?}", token.token_id);
    println!("profile_id = {:?}", token.profile_id.to_string());
    println!("name = {:?}", token.name);
    println!("created_at_ms = {}", token.created_at_ms);
    println!(
        "expires_at_ms = {}",
        token
            .expires_at_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_owned())
    );
    println!(
        "last_used_at_ms = {}",
        token
            .last_used_at_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_owned())
    );
    println!(
        "revoked_at_ms = {}",
        token
            .revoked_at_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_owned())
    );
}

fn print_profile_config(prefix: &str, config: &ProfileConfig) {
    println!("[{prefix}]");
    println!("engine = {:?}", config.engine.to_string());
    println!("dictionary_version = {}", config.dictionary_version);
    println!(
        "dictionary_version_hash = {:?}",
        config.dictionary_version_hash
    );
    println!("blocklist_version = {}", config.blocklist_version);
    println!(
        "blocklist_version_hash = {:?}",
        config.blocklist_version_hash
    );
    println!();

    println!("[{prefix}.word1]");
    println!("enabled = {}", config.word1.enabled);
    print_lengths("lengths", config.word1.lengths.as_deref());
    print_string_array("categories", &config.word1.categories);
    println!("pool_hash = {:?}", config.word1.pool_hash);
    println!();

    println!("[{prefix}.word2]");
    println!("enabled = {}", config.word2.enabled);
    print_lengths("lengths", config.word2.lengths.as_deref());
    print_string_array("categories", &config.word2.categories);
    println!("pool_hash = {:?}", config.word2.pool_hash);
    println!();

    println!("[{prefix}.suffix]");
    println!("enabled = {}", config.suffix.enabled);
    println!("min_length = {}", config.suffix.min_length);
}

fn print_generate_options(prefix: &str, options: &GenerateOptions) {
    println!("[{prefix}]");
    println!("engine = {:?}", options.engine.to_string());
    println!("dictionary_version = {}", options.dictionary_version);
    println!("blocklist_version = {}", options.blocklist_version);
    println!();

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

fn ensure_profile_generation_contract_is_current(profile: &StoredProfile) -> anyhow::Result<()> {
    if profile.config.uses_current_generation_contract() {
        return Ok(());
    }

    anyhow::bail!(
        "profile {:?} was created with a generation engine, dictionary/blocklist versions, or resolved word pools that do not match this binary; profile-backed generation cannot run safely across generation contract changes",
        profile.slug.as_str()
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

fn parse_token_expiration(value: &str) -> Result<i64, String> {
    let expires_at_ms = value
        .parse::<i64>()
        .map_err(|source| format!("invalid token expiration {value:?}: {source}"))?;
    if expires_at_ms <= 0 {
        return Err(
            "token expiration must be a positive Unix timestamp in milliseconds".to_owned(),
        );
    }
    Ok(expires_at_ms)
}

fn parse_profile_access(value: &str) -> Result<ProfileAccess, String> {
    value.parse()
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

fn confirm_profile_config_replacement(profile: &StoredProfile) -> anyhow::Result<()> {
    let mut stderr = io::stderr();
    writeln!(
        stderr,
        "Atomic generation requires a stable profile configuration."
    )?;
    writeln!(
        stderr,
        "Profile {:?} differs from the requested profile configuration.",
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
            "profile replacement requires interactive confirmation; re-run interactively or use matching profile config settings"
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
            "profile replacement requires interactive confirmation; re-run interactively or use matching profile config settings"
        );
    }
    if !second.trim().eq_ignore_ascii_case("replace") {
        anyhow::bail!("profile replacement cancelled");
    }

    Ok(())
}
