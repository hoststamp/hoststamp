// SPDX-License-Identifier: FSL-1.1-ALv2

use assert_cmd::Command;
use hoststamp_core::{
    generator::{GenerateOptions, is_base36_suffix},
    profile::{ProfileConfig, ProfileSlug},
    storage::{ProfileStore, StorageUrl, config_hash},
};
use predicates::prelude::*;
use rusqlite::{Connection, params};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use tempfile::TempDir;

fn command_with_database() -> (Command, TempDir) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let cmd = command_for_database(&database);
    (cmd, tempdir)
}

fn command_for_database(database: &Path) -> Command {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");
    cmd.env(
        "HOSTSTAMP_DATABASE_URL",
        format!("sqlite://{}", database.display()),
    );
    cmd
}

fn future_timestamp_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_millis();
    i64::try_from(millis).expect("timestamp") + 60_000
}

fn set_default_config(database: &Path, args: &[&str]) {
    let mut cmd = command_for_database(database);
    cmd.arg("config")
        .arg("set")
        .args(args)
        .write_stdin("_\nreplace\n")
        .assert()
        .success();
}

#[test]
fn health_prints_status_payload() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("health").assert().success().stdout(
        predicate::str::contains(r#""status":"ok""#)
            .and(predicate::str::contains(r#""service":"hoststamp""#)),
    );
}

#[test]
fn version_prints_cli_name_and_version() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "hoststamp {}",
            env!("CARGO_PKG_VERSION")
        )));
}

#[test]
fn help_prints_generation_flags() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("--capacity")
            .and(predicate::str::contains("--json"))
            .and(predicate::str::contains("--profile"))
            .and(predicate::str::contains("--database-url"))
            .and(predicate::str::contains("random"))
            .and(predicate::str::contains("regenerate"))
            .and(predicate::str::contains("lookup"))
            .and(predicate::str::contains("profile"))
            .and(predicate::str::contains("config"))
            .and(predicate::str::contains("completions"))
            .and(predicate::str::contains("man")),
    );
}

#[test]
fn completions_print_supported_shell_scripts() {
    for (shell, expected) in [
        ("bash", "_hoststamp()"),
        ("zsh", "#compdef hoststamp"),
        ("fish", "complete -c hoststamp"),
    ] {
        let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");
        cmd.args(["completions", shell]).assert().success().stdout(
            predicate::str::contains(expected)
                .and(predicate::str::contains("profile"))
                .and(predicate::str::contains("serve")),
        );
    }
}

#[test]
fn completions_reject_unsupported_shells() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.args(["completions", "powershell"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn man_prints_generated_roff_page() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("man").assert().success().stdout(
        predicate::str::contains(".TH hoststamp 1")
            .and(predicate::str::contains(".SH SUBCOMMANDS"))
            .and(predicate::str::contains("hoststamp\\-completions(1)"))
            .and(predicate::str::contains("hoststamp\\-man(1)")),
    );
}

#[test]
fn config_set_help_prints_profile_config_flags() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.args(["config", "set", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--word1-lengths")
                .and(predicate::str::contains("--word1-categories"))
                .and(predicate::str::contains("--word1-enabled"))
                .and(predicate::str::contains("--word2-lengths"))
                .and(predicate::str::contains("--word2-categories"))
                .and(predicate::str::contains("--word2-enabled"))
                .and(predicate::str::contains("--suffix-enabled"))
                .and(predicate::str::contains("--suffix-min-length")),
        );
}

#[test]
fn config_init_writes_bootstrap_template() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let config_path = tempdir.path().join("hoststamp").join("config.toml");
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("--config")
        .arg(&config_path)
        .args(["config", "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "created config file {}",
            config_path.display()
        )));

    let contents = fs::read_to_string(&config_path).expect("config");
    assert!(contents.contains("[server]"));
    assert!(contents.contains("[storage]"));
    assert!(contents.contains("[api.auth]"));
    assert!(contents.contains("openssl rand -base64 24"));
    assert!(contents.contains("chmod 600"));
    assert!(contents.contains("HOSTSTAMP_ADMIN_TOKEN"));
    assert!(contents.contains("HOSTSTAMP_TOKEN_HASH_KEY"));

    #[cfg(unix)]
    assert_eq!(
        fs::metadata(&config_path)
            .expect("config metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn config_init_uses_env_config_path() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let config_path = tempdir.path().join("hoststamp").join("config.toml");
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.env("HOSTSTAMP_CONFIG", &config_path)
        .args(["config", "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "created config file {}",
            config_path.display()
        )));

    assert!(config_path.exists());
}

#[test]
fn config_init_refuses_to_overwrite_existing_file() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let config_path = tempdir.path().join("config.toml");
    fs::write(&config_path, "existing = true\n").expect("existing config");
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("--config")
        .arg(&config_path)
        .args(["config", "init"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("refusing to overwrite"));

    assert_eq!(
        fs::read_to_string(&config_path).expect("config"),
        "existing = true\n"
    );
}

#[test]
fn serve_help_prints_mode_option() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.args(["serve", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--mode"));
}

#[test]
fn credits_print_license_and_source_attribution() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("--credits").assert().success().stdout(
        predicate::str::contains("FSL-1.1-ALv2")
            .and(predicate::str::contains("EFF large Diceware wordlist"))
            .and(predicate::str::contains("golang-petname"))
            .and(predicate::str::contains("Sqids default blocklist"))
            .and(predicate::str::contains("CC-BY-3.0-US"))
            .and(predicate::str::contains("SHA-256:")),
    );
}

#[test]
fn hidden_notices_command_prints_generated_notices() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("notices").assert().success().stdout(
        predicate::str::starts_with("# Third-Party Notices")
            .and(predicate::str::contains("EFF large Diceware wordlist")),
    );
}

#[test]
fn list_categories_prints_category_counts() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("--list-categories").assert().success().stdout(
        predicate::str::contains("adjective\t")
            .and(predicate::str::contains("animal\t"))
            .and(predicate::str::contains("diceware\t")),
    );
}

#[test]
fn generate_prints_word_word_hash_by_default() {
    let (mut cmd, _tempdir) = command_with_database();
    let assert = cmd.arg("generate").assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let hostname = output.trim();
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 3);
    assert_ne!(parts[0], parts[1]);
    assert!(parts[..2].iter().all(|part| part.chars().count() == 5));
    assert!(parts[2].len() >= 5);
    assert!(is_base36_suffix(parts[2]));
}

#[test]
fn generate_is_the_default_command() {
    let (mut cmd, _tempdir) = command_with_database();
    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let hostname = output.trim();
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 3);
    assert_ne!(parts[0], parts[1]);
    assert!(parts[..2].iter().all(|part| part.chars().count() == 5));
    assert!(parts[2].len() >= 5);
    assert!(is_base36_suffix(parts[2]));
}

#[test]
fn default_generate_accepts_top_level_flags() {
    let (mut cmd, _tempdir) = command_with_database();
    let assert = cmd.args(["--count", "2"]).assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let hostnames = output.lines().collect::<Vec<_>>();

    assert_eq!(hostnames.len(), 2);
    assert!(hostnames.iter().all(|hostname| {
        let parts = hostname.split('-').collect::<Vec<_>>();
        parts.len() == 3 && parts[..2].iter().all(|part| part.chars().count() == 5)
    }));
}

#[test]
fn generate_supports_multiple_hostnames() {
    let (mut cmd, _tempdir) = command_with_database();
    let assert = cmd.args(["generate", "--count", "3"]).assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let hostnames = output.lines().collect::<Vec<_>>();

    assert_eq!(hostnames.len(), 3);
    assert!(hostnames.iter().all(|hostname| {
        let parts = hostname.split('-').collect::<Vec<_>>();
        parts.len() == 3 && parts[0] != parts[1]
    }));
}

#[test]
fn random_prints_stateless_word_word_hash_by_default() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");
    let assert = cmd.arg("random").assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let hostname = output.trim();
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 3);
    assert_ne!(parts[0], parts[1]);
    assert!(parts[..2].iter().all(|part| part.chars().count() == 5));
    assert!(parts[2].len() >= 5);
    assert!(is_base36_suffix(parts[2]));
}

#[test]
fn random_accepts_ad_hoc_generation_options() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");
    let assert = cmd
        .args([
            "random",
            "--word1-lengths",
            "4",
            "--word2-lengths",
            "4",
            "--suffix-enabled",
            "false",
        ])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let parts = output.trim().split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|part| part.chars().count() == 4));
}

#[test]
fn random_json_omits_profile_metadata() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");
    let assert = cmd
        .args(["random", "--count", "2", "--json"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");
    let hostnames = payload["hostnames"].as_array().expect("hostnames");

    assert_eq!(hostnames.len(), 2);
    assert!(hostnames[0].get("profile").is_none());
    assert!(hostnames[0].get("atomic_value").is_none());
    assert!(
        hostnames[0]["hostname"]
            .as_str()
            .expect("hostname")
            .contains('-')
    );
}

#[test]
fn generate_filters_words_by_single_length() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    set_default_config(
        &database,
        &[
            "--word1-lengths",
            "4",
            "--word2-lengths",
            "4",
            "--suffix-enabled",
            "false",
        ],
    );

    let mut cmd = command_for_database(&database);
    let assert = cmd.arg("generate").assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let parts = output.trim().split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|part| part.chars().count() == 4));
}

#[test]
fn generate_accepts_length_set() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    set_default_config(
        &database,
        &[
            "--word1-lengths",
            "4,5,6",
            "--word2-lengths",
            "4,5,6",
            "--suffix-enabled",
            "false",
        ],
    );

    let mut cmd = command_for_database(&database);
    let assert = cmd.arg("generate").assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let parts = output.trim().split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|part| {
        let n = part.chars().count();
        (4..=6).contains(&n)
    }));
}

#[test]
fn generate_accepts_any_length() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    set_default_config(
        &database,
        &[
            "--word1-lengths",
            "any",
            "--word2-lengths",
            "any",
            "--suffix-enabled",
            "false",
        ],
    );

    let mut cmd = command_for_database(&database);
    let assert = cmd.arg("generate").assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let parts = output.trim().split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 2);
}

#[test]
fn config_set_errors_when_word_filter_has_no_matches() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args(["config", "set", "--word1-lengths", "100"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("do not contain"));
}

#[test]
fn generate_rejects_count_above_cap() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.args(["generate", "--count", "51"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("count must be between 1 and 50"));
}

#[test]
fn capacity_reports_word_and_suffix_space() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    set_default_config(
        &database,
        &[
            "--word1-categories",
            "diceware",
            "--word2-categories",
            "diceware",
            "--word1-lengths",
            "5",
            "--word2-lengths",
            "5",
            "--suffix-min-length",
            "5",
        ],
    );

    let mut cmd = command_for_database(&database);
    cmd.arg("--capacity").assert().success().stdout(
        predicate::str::contains("word1_words\t")
            .and(predicate::str::contains("word2_words\t"))
            .and(predicate::str::contains("overlapping_words\t"))
            .and(predicate::str::contains("unique_word_combinations\t"))
            .and(predicate::str::contains(
                "fixed_suffix_variants\t60,466,176",
            ))
            .and(predicate::str::contains("suffix_bits\t25"))
            .and(predicate::str::contains(
                "random_fallback_max_value\t30,233,088",
            ))
            .and(predicate::str::contains(
                "atomic_storage_max_value\t9,223,372,036,854,775,807",
            ))
            .and(predicate::str::contains("total_variants\t")),
    );
}

#[test]
fn capacity_reports_suffix_number_bounds() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.arg("--capacity").assert().success().stdout(
        predicate::str::contains("fixed_suffix_variants\t60,466,176")
            .and(predicate::str::contains("suffix_bits\t25"))
            .and(predicate::str::contains("random_fallback_min_value\t1"))
            .and(predicate::str::contains(
                "random_fallback_max_value\t30,233,088",
            ))
            .and(predicate::str::contains("atomic_min_value\t1"))
            .and(predicate::str::contains(
                "atomic_storage_max_value\t9,223,372,036,854,775,807",
            )),
    );
}

#[test]
fn capacity_json_reports_structured_capacity() {
    let (mut cmd, _tempdir) = command_with_database();

    let assert = cmd.args(["--capacity", "--json"]).assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");

    assert_eq!(payload["suffix_enabled"], true);
    assert_eq!(payload["suffix_min_length"], 5);
    assert_eq!(payload["suffix_variants"], "60466176");
    assert_eq!(payload["suffix_bits"], 25);
    assert_eq!(payload["random_fallback_max_value"], 30233088);
    assert_eq!(
        payload["atomic_storage_max_value"],
        9_223_372_036_854_775_807i64
    );
}

#[test]
fn capacity_reports_disabled_suffix_bounds() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    set_default_config(&database, &["--suffix-enabled", "false"]);

    let mut cmd = command_for_database(&database);
    cmd.arg("--capacity").assert().success().stdout(
        predicate::str::contains("fixed_suffix_variants\tdisabled")
            .and(predicate::str::contains("suffix_bits\t0"))
            .and(predicate::str::contains(
                "random_fallback_min_value\tdisabled",
            ))
            .and(predicate::str::contains(
                "random_fallback_max_value\tdisabled",
            ))
            .and(predicate::str::contains("atomic_min_value\tdisabled"))
            .and(predicate::str::contains(
                "atomic_storage_max_value\tdisabled",
            )),
    );
}

#[test]
fn config_show_prints_bootstrap_profile_and_effective_generate_config() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    set_default_config(&database, &["--word1-lengths", "4"]);

    let mut cmd = command_for_database(&database);
    cmd.args(["config", "show", "--count", "2"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("[bootstrap]")
                .and(predicate::str::contains("[profile]"))
                .and(predicate::str::contains("[profile.config.word1]"))
                .and(predicate::str::contains("[effective.generate.word1]"))
                .and(predicate::str::contains("lengths = [4]"))
                .and(predicate::str::contains("[effective.generate.request]"))
                .and(predicate::str::contains("count = 2")),
        );
}

#[test]
fn profile_show_seeds_default_profile() {
    let (mut cmd, _tempdir) = command_with_database();
    cmd.args(["profile", "--profile", "_", "show"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("[profile]")
                .and(predicate::str::contains(r#"slug = "_""#))
                .and(predicate::str::contains(r#"access = "private""#))
                .and(predicate::str::contains("last_atomic_value = 0"))
                .and(predicate::str::contains("[profile.config.word1]")),
        );
}

#[test]
fn profile_commands_create_show_list_and_delete_profiles() {
    let (mut create, tempdir) = command_with_database();
    create
        .args(["--profile", "team-a", "profile", "new"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("[profile]")
                .and(predicate::str::contains(r#"slug = "team-a""#))
                .and(predicate::str::contains("last_atomic_value = 0")),
        );

    let database = tempdir.path().join("hoststamp.db");
    let mut list = command_for_database(&database);
    list.arg("profile").arg("list").assert().success().stdout(
        predicate::str::contains("slug\tid\taccess\tlast_atomic_value")
            .and(predicate::str::contains("team-a\t"))
            .and(predicate::str::contains("\tprivate\t"))
            .and(predicate::str::contains("\t0")),
    );

    let mut show = command_for_database(&database);
    show.args(["--profile", "team-a", "profile", "show"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains(r#"slug = "team-a""#)
                .and(predicate::str::contains("[profile.config.word1]")),
        );

    let mut delete = command_for_database(&database);
    delete
        .args(["--profile", "team-a", "profile", "delete"])
        .write_stdin("team-a\ndelete\n")
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"deleted profile "team-a""#))
        .stderr(predicate::str::contains("confirm profile deletion"));

    let mut deleted_show = command_for_database(&database);
    deleted_show
        .args(["--profile", "team-a", "profile", "show"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "profile \"team-a\" does not exist",
        ));
}

#[test]
fn profile_export_import_preserves_profile_identity() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let source_database = tempdir.path().join("source.db");
    let target_database = tempdir.path().join("target.db");
    let export_path = tempdir.path().join("team-a.json");

    let mut create = command_for_database(&source_database);
    create
        .args(["--profile", "team-a", "profile", "new"])
        .assert()
        .success();

    let mut access = command_for_database(&source_database);
    access
        .args([
            "--profile",
            "team-a",
            "profile",
            "set-access",
            "--access",
            "public",
        ])
        .assert()
        .success();

    let mut generate = command_for_database(&source_database);
    generate
        .args(["--profile", "team-a", "generate", "--count", "2"])
        .assert()
        .success();

    let mut export = command_for_database(&source_database);
    let assert = export
        .args(["--profile", "team-a", "profile", "export"])
        .assert()
        .success();
    let exported = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&exported).expect("json");
    let exported_id = payload["id"].as_str().expect("id").to_owned();

    assert_eq!(payload["format"], "hoststamp-profile-v1");
    assert_eq!(payload["slug"], "team-a");
    assert_eq!(payload["access"], "public");
    assert_eq!(payload["last_atomic_value"], 2);
    assert_eq!(payload["config_hash"].as_str().expect("hash").len(), 64);

    fs::write(&export_path, exported).expect("export file");

    let mut import = command_for_database(&target_database);
    import
        .args(["profile", "import", export_path.to_str().expect("path")])
        .assert()
        .success()
        .stdout(
            predicate::str::contains(format!(r#"id = "{exported_id}""#))
                .and(predicate::str::contains(r#"slug = "team-a""#))
                .and(predicate::str::contains(r#"access = "public""#))
                .and(predicate::str::contains("last_atomic_value = 2")),
        );

    let mut next = command_for_database(&target_database);
    let assert = next
        .args(["--profile", "team-a", "generate", "--json"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");
    assert_eq!(payload["hostnames"][0]["profile"], "team-a");
    assert_eq!(payload["hostnames"][0]["atomic_value"], 3);
}

#[test]
fn profile_import_replacement_requires_confirmation() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let source_database = tempdir.path().join("source.db");
    let target_database = tempdir.path().join("target.db");
    let export_path = tempdir.path().join("team-a.json");

    let mut create = command_for_database(&source_database);
    create
        .args(["--profile", "team-a", "profile", "new"])
        .assert()
        .success();

    let mut export = command_for_database(&source_database);
    let assert = export
        .args(["--profile", "team-a", "profile", "export"])
        .assert()
        .success();
    let exported = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    fs::write(&export_path, &exported).expect("export file");

    let mut import = command_for_database(&target_database);
    import
        .args(["profile", "import", export_path.to_str().expect("path")])
        .assert()
        .success();

    let mut replacement: serde_json::Value = serde_json::from_str(&exported).expect("json");
    replacement["last_atomic_value"] = serde_json::json!(9);
    fs::write(
        &export_path,
        serde_json::to_string_pretty(&replacement).expect("json"),
    )
    .expect("replacement file");

    let mut cancelled = command_for_database(&target_database);
    cancelled
        .args(["profile", "import", export_path.to_str().expect("path")])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "profile import replacement requires interactive confirmation",
        ));

    let mut wrong_slug = command_for_database(&target_database);
    wrong_slug
        .args(["profile", "import", export_path.to_str().expect("path")])
        .write_stdin("wrong-slug\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "profile import replacement cancelled",
        ));

    let mut wrong_action = command_for_database(&target_database);
    wrong_action
        .args(["profile", "import", export_path.to_str().expect("path")])
        .write_stdin("team-a\nno\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "profile import replacement cancelled",
        ));

    let mut confirmed = command_for_database(&target_database);
    confirmed
        .args(["profile", "import", export_path.to_str().expect("path")])
        .write_stdin("team-a\nreplace\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("last_atomic_value = 9"))
        .stderr(predicate::str::contains("confirm profile import"));
}

#[test]
fn profile_import_rejects_invalid_export_hash() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let source_database = tempdir.path().join("source.db");
    let target_database = tempdir.path().join("target.db");
    let export_path = tempdir.path().join("team-a.json");

    let mut create = command_for_database(&source_database);
    create
        .args(["--profile", "team-a", "profile", "new"])
        .assert()
        .success();

    let mut export = command_for_database(&source_database);
    let assert = export
        .args(["--profile", "team-a", "profile", "export"])
        .assert()
        .success();
    let exported = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let mut payload: serde_json::Value = serde_json::from_str(&exported).expect("json");
    payload["config_hash"] = serde_json::json!("bad-hash");
    fs::write(
        &export_path,
        serde_json::to_string_pretty(&payload).expect("json"),
    )
    .expect("export file");

    let mut import = command_for_database(&target_database);
    import
        .args(["profile", "import", export_path.to_str().expect("path")])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "profile import config_hash does not match config",
        ));
}

#[test]
fn profile_reset_atomic_value_sets_the_stored_counter() {
    let (mut generate, tempdir) = command_with_database();
    generate.arg("generate").assert().success();

    let database = tempdir.path().join("hoststamp.db");
    let mut reset = command_for_database(&database);
    reset
        .args(["profile", "reset-atomic-value", "--atomic-value", "10"])
        .write_stdin("_\nreset\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("last_atomic_value = 10"))
        .stderr(
            predicate::str::contains("changes its stored atomic value from 1 to 10").and(
                predicate::str::contains(
                    "The next profile-backed generation will use atomic value 11",
                ),
            ),
        );

    let mut next = command_for_database(&database);
    let assert = next.args(["generate", "--json"]).assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");
    assert_eq!(payload["hostnames"][0]["atomic_value"], 11);
}

#[test]
fn profile_access_and_token_commands_manage_api_auth() {
    let (mut create, tempdir) = command_with_database();
    create
        .args(["--profile", "team-a", "profile", "new"])
        .assert()
        .success();

    let database = tempdir.path().join("hoststamp.db");
    let mut access = command_for_database(&database);
    access
        .args([
            "--profile",
            "team-a",
            "profile",
            "set-access",
            "--access",
            "public",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(r#"access = "public""#));

    let mut create_token = command_for_database(&database);
    create_token.env("HOSTSTAMP_TOKEN_HASH_KEY", "hash-key");
    let expires_at_ms = future_timestamp_ms();
    let expires_at_ms_arg = expires_at_ms.to_string();
    let assert = create_token
        .args([
            "--profile",
            "team-a",
            "profile",
            "token",
            "create",
            "--name",
            "deploy",
            "--expires-at-ms",
            &expires_at_ms_arg,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("token = \"hspt_"))
        .stdout(predicate::str::contains(format!(
            "expires_at_ms = {expires_at_ms}"
        )));
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let token_id = output
        .lines()
        .find_map(|line| line.strip_prefix("token_id = \""))
        .and_then(|value| value.strip_suffix('"'))
        .expect("token id")
        .to_owned();

    let mut list = command_for_database(&database);
    list.args(["--profile", "team-a", "profile", "token", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("deploy"))
        .stdout(predicate::str::contains(expires_at_ms.to_string()));

    let mut revoke = command_for_database(&database);
    revoke
        .args([
            "--profile",
            "team-a",
            "profile",
            "token",
            "revoke",
            &token_id,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("revoked_at_ms = "));
}

#[test]
fn profile_token_create_rejects_non_positive_expiration() {
    let (mut create, tempdir) = command_with_database();
    create
        .args(["--profile", "team-a", "profile", "new"])
        .assert()
        .success();

    let database = tempdir.path().join("hoststamp.db");
    let mut create_token = command_for_database(&database);
    create_token
        .env("HOSTSTAMP_TOKEN_HASH_KEY", "hash-key")
        .args([
            "--profile",
            "team-a",
            "profile",
            "token",
            "create",
            "--name",
            "deploy",
            "--expires-at-ms",
            "0",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "token expiration must be a positive",
        ));
}

#[test]
fn config_set_rejects_empty_category_list() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args(["config", "set", "--word1-categories", ","])
        .assert()
        .failure()
        .stderr(predicate::str::contains("category list must not be empty"));
}

#[test]
fn config_set_rejects_unknown_category() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args(["config", "set", "--word1-categories", "missing"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("unknown category")
                .and(predicate::str::contains("panicked").not()),
        );
}

#[test]
fn generate_uses_profile_backed_suffix_by_default() {
    let (mut cmd, _tempdir) = command_with_database();

    let assert = cmd.args(["generate", "--count", "2"]).assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let hostnames = output.lines().collect::<Vec<_>>();

    assert_eq!(hostnames.len(), 2);
    assert!(hostnames.iter().all(|hostname| {
        let parts = hostname.split('-').collect::<Vec<_>>();
        parts.len() == 3 && parts[2].len() >= 5 && is_base36_suffix(parts[2])
    }));
}

#[test]
fn generate_json_prints_atomic_metadata() {
    let (mut cmd, _tempdir) = command_with_database();

    let assert = cmd
        .args(["generate", "--count", "2", "--json"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");
    let hostnames = payload["hostnames"].as_array().expect("hostnames");

    assert_eq!(hostnames.len(), 2);
    assert_eq!(hostnames[0]["profile"], "_");
    assert_eq!(hostnames[0]["atomic_value"], 1);
    assert_eq!(hostnames[1]["profile"], "_");
    assert_eq!(hostnames[1]["atomic_value"], 2);
    assert!(
        hostnames[0]["hostname"]
            .as_str()
            .expect("hostname")
            .contains('-')
    );
}

#[test]
fn events_command_lists_filtered_audit_events() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");

    let mut create = command_for_database(&database);
    create
        .args(["--profile", "team-a", "profile", "new"])
        .assert()
        .success();

    let mut generate = command_for_database(&database);
    generate
        .args(["--profile", "team-a", "generate", "--count", "2"])
        .assert()
        .success();

    let mut token = command_for_database(&database);
    token
        .env("HOSTSTAMP_TOKEN_HASH_KEY", "hash-key")
        .args([
            "--profile",
            "team-a",
            "profile",
            "token",
            "create",
            "--name",
            "deploy",
        ])
        .assert()
        .success();

    let mut events = command_for_database(&database);
    events
        .args(["events", "--profile-slug", "team-a"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("created_at_ms\tsource\taction")
                .and(predicate::str::contains("profile.create"))
                .and(predicate::str::contains("generate"))
                .and(predicate::str::contains("profile.token.create"))
                .and(predicate::str::contains("deploy"))
                .and(predicate::str::contains("1-2")),
        );

    let mut filtered = command_for_database(&database);
    let assert = filtered
        .args([
            "--json",
            "events",
            "--profile-slug",
            "team-a",
            "--action",
            "generate",
        ])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");
    let events = payload["events"].as_array().expect("events");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["source"], "cli");
    assert_eq!(events[0]["action"], "generate");
    assert_eq!(events[0]["profile_slug"], "team-a");
    assert_eq!(events[0]["atomic_start"], 1);
    assert_eq!(events[0]["atomic_end"], 2);
    assert_eq!(events[0]["metadata"]["count"], 2);
}

#[test]
fn backup_export_prints_profiles_token_metadata_and_events_without_secrets() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");

    let mut create = command_for_database(&database);
    create
        .args(["--profile", "team-a", "profile", "new"])
        .assert()
        .success();

    let mut generate = command_for_database(&database);
    generate
        .args(["--profile", "team-a", "generate", "--count", "2"])
        .assert()
        .success();

    let mut token = command_for_database(&database);
    token
        .env("HOSTSTAMP_TOKEN_HASH_KEY", "hash-key")
        .args([
            "--profile",
            "team-a",
            "profile",
            "token",
            "create",
            "--name",
            "deploy",
        ])
        .assert()
        .success();

    let mut export = command_for_database(&database);
    let assert = export.args(["backup", "export"]).assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");

    assert_eq!(payload["format"], "hoststamp-backup-v1");
    assert!(payload["exported_at_ms"].as_i64().expect("exported_at_ms") > 0);

    let profiles = payload["profiles"].as_array().expect("profiles");
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0]["slug"], "team-a");
    assert_eq!(profiles[0]["last_atomic_value"], 2);

    let profile_tokens = payload["profile_tokens"]
        .as_array()
        .expect("profile tokens");
    assert_eq!(profile_tokens.len(), 1);
    assert_eq!(profile_tokens[0]["name"], "deploy");
    assert!(profile_tokens[0].get("profile_token").is_none());
    assert!(profile_tokens[0].get("token_hash").is_none());

    let events = payload["events"].as_array().expect("events");
    for action in ["profile.create", "generate", "profile.token.create"] {
        assert!(
            events.iter().any(|event| event["action"] == action),
            "missing action {action}"
        );
    }
    assert!(
        !events
            .iter()
            .any(|event| event["action"] == "backup.export")
    );
    assert!(!output.contains("hspt_"));
    assert!(!output.contains("hash-key"));

    let mut audit = command_for_database(&database);
    let assert = audit
        .args(["--json", "events", "--action", "backup.export"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");
    let backup_events = payload["events"].as_array().expect("events");
    assert_eq!(backup_events.len(), 1);
    let backup_event = backup_events.iter().next().expect("backup event");
    assert_eq!(backup_event["metadata"]["profile_count"], 1);
    assert_eq!(backup_event["metadata"]["profile_token_count"], 1);
    assert_eq!(backup_event["metadata"]["event_count"], 3);
}

#[test]
fn regenerate_recreates_profile_hostname_without_incrementing_counter() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut generate = command_for_database(&database);

    let assert = generate
        .args(["generate", "--count", "2"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let generated = output.lines().collect::<Vec<_>>();
    assert_eq!(generated.len(), 2);

    let mut regenerate_first = command_for_database(&database);
    regenerate_first
        .args(["regenerate", "--atomic-value", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains(generated[0]));

    let mut regenerate_second = command_for_database(&database);
    regenerate_second
        .args(["regenerate", "--atomic-value", "2"])
        .assert()
        .success()
        .stdout(predicate::str::contains(generated[1]));

    let mut show = command_for_database(&database);
    show.args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("last_atomic_value = 2"));
}

#[test]
fn regenerate_supports_count() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut generate = command_for_database(&database);

    let assert = generate
        .args(["generate", "--count", "3"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let generated = output.lines().collect::<Vec<_>>();
    assert_eq!(generated.len(), 3);

    let mut regenerate = command_for_database(&database);
    let assert = regenerate
        .args([
            "regenerate",
            "--atomic-value",
            "2",
            "--count",
            "2",
            "--json",
        ])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");
    let hostnames = payload["hostnames"].as_array().expect("hostnames");

    assert_eq!(hostnames.len(), 2);
    assert_eq!(hostnames[0]["hostname"], generated[1]);
    assert_eq!(hostnames[0]["atomic_value"], 2);
    assert_eq!(hostnames[1]["hostname"], generated[2]);
    assert_eq!(hostnames[1]["atomic_value"], 3);
}

#[test]
fn profile_history_supports_regenerating_replaced_profiles_by_id() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut generate = command_for_database(&database);

    let assert = generate
        .args(["--profile", "team-a", "generate"])
        .assert()
        .success();
    let generated = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let generated = generated.trim().to_owned();

    let mut replace = command_for_database(&database);
    replace
        .args([
            "--profile",
            "team-a",
            "config",
            "set",
            "--word1-lengths",
            "4",
        ])
        .write_stdin("team-a\nreplace\n")
        .assert()
        .success();

    let store = ProfileStore::open(&StorageUrl::Sqlite(database.clone())).expect("store");
    let slug = "team-a".parse::<ProfileSlug>().expect("slug");
    let history = store.list_profile_history(&slug).expect("history");
    assert_eq!(history.len(), 2);
    assert!(history[0].replaced_at_ms.is_some());
    assert_eq!(history[0].replaced_by_id, Some(history[1].id));
    assert!(history[1].replaced_at_ms.is_none());
    let retired_id = history[0].id.to_string();
    let replacement_id = history[1].id.to_string();

    let mut history_cmd = command_for_database(&database);
    history_cmd
        .args(["--profile", "team-a", "profile", "history"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("slug\tid\tstate\taccess\tlast_atomic_value")
                .and(predicate::str::contains(&retired_id))
                .and(predicate::str::contains(&replacement_id))
                .and(predicate::str::contains("\treplaced\t"))
                .and(predicate::str::contains("\tactive\t")),
        );

    let mut regenerate = command_for_database(&database);
    regenerate
        .args([
            "regenerate",
            "--profile-id",
            &retired_id,
            "--atomic-value",
            "1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(generated));
}

#[test]
fn regenerate_json_prints_atomic_metadata() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut generate = command_for_database(&database);

    let assert = generate.arg("generate").assert().success();
    let generated = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let generated = generated.trim();

    let mut regenerate = command_for_database(&database);
    let assert = regenerate
        .args(["regenerate", "--atomic-value", "1", "--json"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");
    let hostnames = payload["hostnames"].as_array().expect("hostnames");

    assert_eq!(hostnames.len(), 1);
    assert_eq!(hostnames[0]["hostname"], generated);
    assert_eq!(hostnames[0]["profile"], "_");
    assert_eq!(hostnames[0]["atomic_value"], 1);
}

#[test]
fn lookup_validates_profile_backed_hostname() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut generate = command_for_database(&database);

    let assert = generate
        .args(["generate", "--count", "2"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let generated = output.lines().collect::<Vec<_>>();
    assert_eq!(generated.len(), 2);

    let mut lookup = command_for_database(&database);
    lookup
        .args(["lookup", generated[1]])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("valid = true")
                .and(predicate::str::contains(r#"profile = "_""#))
                .and(predicate::str::contains("atomic_value = 2")),
        );
}

#[test]
fn lookup_json_reports_tampered_hostname_as_invalid() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut generate = command_for_database(&database);

    let assert = generate.arg("generate").assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let generated = output.trim();
    let mut parts = generated.split('-').collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    parts[0] = "zzzzz";
    let tampered = parts.join("-");

    let mut lookup = command_for_database(&database);
    let assert = lookup
        .args(["lookup", &tampered, "--json"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");

    assert_eq!(payload["profile"], "_");
    assert_eq!(payload["atomic_value"], 1);
    assert_eq!(payload["valid"], false);
}

#[test]
fn validate_accepts_profile_backed_hostname() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut generate = command_for_database(&database);

    let assert = generate
        .args(["generate", "--count", "2"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let generated = output.lines().collect::<Vec<_>>();
    assert_eq!(generated.len(), 2);

    let mut validate = command_for_database(&database);
    validate
        .args(["validate", generated[1]])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("hostname\tvalid\tprofile\tatomic_value")
                .and(predicate::str::contains(generated[1]))
                .and(predicate::str::contains("true\t_\t2")),
        );
}

#[test]
fn validate_rejects_invalid_argument_shapes() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let input = tempdir.path().join("hostnames.txt");
    fs::write(&input, "brief-cobra-db50d\n").expect("write input");

    let mut both = command_for_database(&database);
    both.args([
        "validate",
        "brief-cobra-db50d",
        "--file",
        input.to_str().expect("path"),
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains(
        "validate requires exactly one hostname or --file <path>",
    ));

    let mut neither = command_for_database(&database);
    neither
        .arg("validate")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "validate requires exactly one hostname or --file <path>",
        ));

    let mut count = command_for_database(&database);
    count
        .args(["validate", "brief-cobra-db50d", "--count", "2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "validate only supports --profile, --file, and --json",
        ));

    let mut capacity = command_for_database(&database);
    capacity
        .args(["validate", "brief-cobra-db50d", "--capacity"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "validate only supports --profile, --file, and --json",
        ));
}

#[test]
fn validate_rejects_empty_file_input() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let input = tempdir.path().join("hostnames.txt");
    fs::write(&input, "\n  \n").expect("write input");

    let mut validate = command_for_database(&database);
    validate
        .args(["validate", "--file", input.to_str().expect("path")])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not contain any hostnames"));
}

#[test]
fn validate_file_json_reports_invalid_hostnames_and_fails() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut generate = command_for_database(&database);

    let assert = generate.arg("generate").assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let generated = output.trim();
    let mut parts = generated.split('-').collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    parts[0] = "zzzzz";
    let tampered = parts.join("-");
    let input = tempdir.path().join("hostnames.txt");
    fs::write(&input, format!("{generated}\n\n{tampered}\n")).expect("write input");

    let mut validate = command_for_database(&database);
    let assert = validate
        .args([
            "validate",
            "--file",
            input.to_str().expect("path"),
            "--json",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "validation failed for 1 hostname(s)",
        ));
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let payload: serde_json::Value = serde_json::from_str(&output).expect("json");
    let results = payload["results"].as_array().expect("results");

    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["hostname"], generated);
    assert_eq!(results[0]["profile"], "_");
    assert_eq!(results[0]["atomic_value"], 1);
    assert_eq!(results[0]["valid"], true);
    assert_eq!(results[1]["hostname"], tampered);
    assert_eq!(results[1]["profile"], "_");
    assert_eq!(results[1]["atomic_value"], 1);
    assert_eq!(results[1]["valid"], false);
}

#[test]
fn lookup_requires_profile_backed_suffixes() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut store = ProfileStore::open(&StorageUrl::Sqlite(database.clone())).expect("store");
    let config = ProfileConfig::from(&GenerateOptions {
        suffix_enabled: false,
        ..GenerateOptions::default()
    });
    store
        .load_or_seed_profile(&ProfileSlug::default_profile(), &config)
        .expect("profile");

    let mut cmd = command_for_database(&database);
    cmd.args(["lookup", "brief-cobra-db50d"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "atomic values are only tracked when suffixes are enabled",
        ));
}

#[test]
fn lookup_rejects_unsupported_options() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");

    let mut count = command_for_database(&database);
    count
        .args(["lookup", "brief-cobra-db50d", "--count", "3"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "lookup only supports --profile and --json",
        ));

    let mut capacity = command_for_database(&database);
    capacity
        .args(["lookup", "brief-cobra-db50d", "--capacity"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "lookup only supports --profile and --json",
        ));
}

#[test]
fn regenerate_rejects_invalid_atomic_value() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args(["regenerate", "--atomic-value", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("atomic value must be at least 1"));
}

#[test]
fn regenerate_rejects_values_that_have_not_been_issued() {
    let (mut generate, tempdir) = command_with_database();
    generate.arg("generate").assert().success();

    let database = tempdir.path().join("hoststamp.db");
    let mut regenerate = command_for_database(&database);
    regenerate
        .args(["regenerate", "--atomic-value", "1", "--count", "2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("never generated"));
}

#[test]
fn regenerate_rejects_generation_options() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args(["regenerate", "--atomic-value", "1", "--capacity"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "regenerate only supports --profile, --profile-id, --atomic-value, --count, and --json",
        ));
}

#[test]
fn regenerate_requires_profile_backed_suffixes() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut store = ProfileStore::open(&StorageUrl::Sqlite(database.clone())).expect("store");
    let config = ProfileConfig::from(&GenerateOptions {
        suffix_enabled: false,
        ..GenerateOptions::default()
    });
    store
        .load_or_seed_profile(&ProfileSlug::default_profile(), &config)
        .expect("profile");

    let mut cmd = command_for_database(&database);
    cmd.args(["regenerate", "--atomic-value", "1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "atomic values are only tracked when suffixes are enabled",
        ));
}

#[test]
fn config_set_replacement_requires_confirmation() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut cancelled = command_for_database(&database);

    cancelled
        .args([
            "config",
            "set",
            "--profile",
            "team-a",
            "--word1-lengths",
            "4",
        ])
        .write_stdin("team-a\nno\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("profile replacement cancelled"));

    let mut confirmed = command_for_database(&database);
    confirmed
        .args([
            "config",
            "set",
            "--profile",
            "team-a",
            "--word1-lengths",
            "4",
        ])
        .write_stdin("team-a\nreplace\n")
        .assert()
        .success()
        .stderr(
            predicate::str::contains("reset the atomic counter")
                .and(predicate::str::contains("[profile.config.replacement]"))
                .and(predicate::str::contains("replacement_profile_id = \"new\""))
                .and(predicate::str::contains(
                    "replacement_last_atomic_value = 0",
                ))
                .and(predicate::str::contains(
                    "existing_profile_tokens = \"invalidated\"",
                ))
                .and(predicate::str::contains("[profile.config.diff]"))
                .and(predicate::str::contains("field\tcurrent\treplacement"))
                .and(predicate::str::contains("word1.lengths\t[5]\t[4]"))
                .and(predicate::str::contains("word1.pool_hash\t").not()),
        );

    let mut reused = command_for_database(&database);
    let assert = reused
        .args(["--profile", "team-a", "generate"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let parts = output.trim().split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].chars().count(), 4);
    assert!(parts[2].len() >= 5);
}

#[test]
fn generate_rejects_stale_dictionary_pool_hash() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let slug = ProfileSlug::default_profile();
    let stale_config = ProfileConfig {
        word1: hoststamp_core::profile::WordProfileConfig {
            pool_hash: Some("old".to_owned()),
            ..ProfileConfig::default().word1
        },
        ..ProfileConfig::default()
    };
    let mut store = ProfileStore::open(&StorageUrl::Sqlite(database.clone())).expect("store");
    store
        .load_or_seed_profile(&slug, &stale_config)
        .expect("profile");
    drop(store);

    let stale_json = serde_json::to_string(&stale_config).expect("json");
    let stale_hash = config_hash(&stale_config).expect("hash");
    Connection::open(&database)
        .expect("connection")
        .execute(
            "UPDATE hoststamp_profiles
             SET config_json = ?1, config_hash = ?2
             WHERE slug = ?3 AND replaced_at_ms IS NULL",
            params![stale_json, stale_hash.as_slice(), slug.as_str()],
        )
        .expect("stale profile");

    let mut cmd = command_for_database(&database);
    cmd.arg("generate")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "profile-backed generation cannot run safely across generation contract changes",
        ));
}

#[test]
fn config_set_rejects_suffix_min_length_below_floor() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.args(["config", "set", "--suffix-min-length", "2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "suffix minimum length must be between",
        ));
}

#[test]
fn generate_can_disable_word2() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    set_default_config(
        &database,
        &["--word2-enabled", "false", "--suffix-enabled", "false"],
    );

    let mut cmd = command_for_database(&database);
    let assert = cmd.arg("generate").assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let parts = output.trim().split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].chars().count(), 5);
}

#[test]
fn config_set_rejects_all_positions_disabled() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args([
        "config",
        "set",
        "--word1-enabled",
        "false",
        "--word2-enabled",
        "false",
        "--suffix-enabled",
        "false",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("at least one position"));
}
