// SPDX-License-Identifier: FSL-1.1-ALv2

use assert_cmd::Command;
use hoststamp::{
    generator::{GenerateOptions, is_base36_suffix},
    profile::{ProfileConfig, ProfileSlug},
    storage::{ProfileStore, StorageUrl},
};
use predicates::prelude::*;
use std::path::Path;
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
        .stdout(predicate::str::contains("hoststamp 0.0.0"));
}

#[test]
fn help_prints_generation_flags() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("--word1-lengths")
            .and(predicate::str::contains("--word1-categories"))
            .and(predicate::str::contains("--word2-lengths"))
            .and(predicate::str::contains("--word2-categories"))
            .and(predicate::str::contains("--no-suffix"))
            .and(predicate::str::contains("--suffix-min-length"))
            .and(predicate::str::contains("--capacity"))
            .and(predicate::str::contains("--json"))
            .and(predicate::str::contains("--profile"))
            .and(predicate::str::contains("--database-url"))
            .and(predicate::str::contains("regenerate"))
            .and(predicate::str::contains("config")),
    );
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
    let assert = cmd
        .args([
            "--count",
            "2",
            "--word1-lengths",
            "4",
            "--word2-lengths",
            "4",
            "--no-suffix",
        ])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let hostnames = output.lines().collect::<Vec<_>>();

    assert_eq!(hostnames.len(), 2);
    assert!(hostnames.iter().all(|hostname| {
        let parts = hostname.split('-').collect::<Vec<_>>();
        parts.len() == 2 && parts.iter().all(|part| part.chars().count() == 4)
    }));
}

#[test]
fn serve_rejects_invalid_suffix_min_length() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.args(["serve", "--addr", "127.0.0.1:0", "--suffix-min-length", "3"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "suffix minimum length must be between",
        ));
}

#[test]
fn generate_supports_multiple_hostnames() {
    let (mut cmd, _tempdir) = command_with_database();
    let assert = cmd
        .args([
            "generate",
            "--count",
            "3",
            "--word1-categories",
            "diceware",
            "--word2-categories",
            "diceware",
            "--no-suffix",
        ])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let hostnames = output.lines().collect::<Vec<_>>();

    assert_eq!(hostnames.len(), 3);
    assert!(hostnames.iter().all(|hostname| {
        let parts = hostname.split('-').collect::<Vec<_>>();
        parts.len() == 2 && parts[0] != parts[1]
    }));
}

#[test]
fn generate_filters_words_by_single_length() {
    let (mut cmd, _tempdir) = command_with_database();
    let assert = cmd
        .args([
            "generate",
            "--word1-lengths",
            "4",
            "--word2-lengths",
            "4",
            "--no-suffix",
        ])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let parts = output.trim().split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|part| part.chars().count() == 4));
}

#[test]
fn generate_accepts_length_set() {
    let (mut cmd, _tempdir) = command_with_database();
    let assert = cmd
        .args([
            "generate",
            "--word1-lengths",
            "4,5,6",
            "--word2-lengths",
            "4,5,6",
            "--no-suffix",
        ])
        .assert()
        .success();
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
    let (mut cmd, _tempdir) = command_with_database();
    let assert = cmd
        .args([
            "generate",
            "--word1-lengths",
            "any",
            "--word2-lengths",
            "any",
            "--no-suffix",
        ])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let parts = output.trim().split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 2);
}

#[test]
fn generate_errors_when_word_filter_has_no_matches() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args(["generate", "--word1-lengths", "100"])
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
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args([
        "--capacity",
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
    ])
    .assert()
    .success()
    .stdout(
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

    cmd.args(["--capacity", "--suffix-min-length", "5"])
        .assert()
        .success()
        .stdout(
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
fn config_show_prints_bootstrap_profile_and_effective_generate_config() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args(["config", "show", "--word1-lengths", "4", "--count", "2"])
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
fn generate_rejects_empty_category_list() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args(["generate", "--word1-categories", ","])
        .assert()
        .failure()
        .stderr(predicate::str::contains("category list must not be empty"));
}

#[test]
fn generate_rejects_unknown_category() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args(["generate", "--word1-categories", "missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown category"));
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
        .args(["regenerate", "--atomic-value", "2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("was never generated"));
}

#[test]
fn regenerate_rejects_generation_options() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args(["regenerate", "--atomic-value", "1", "--count", "2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "regenerate only supports --profile and --atomic-value",
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
fn atomic_profile_config_replacement_requires_confirmation() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let database = tempdir.path().join("hoststamp.db");
    let mut cancelled = command_for_database(&database);

    cancelled
        .args(["generate", "--word1-lengths", "4"])
        .write_stdin("_\nno\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("profile replacement cancelled"));

    let mut confirmed = command_for_database(&database);
    confirmed
        .args(["generate", "--word1-lengths", "4"])
        .write_stdin("_\nreplace\n")
        .assert()
        .success()
        .stderr(predicate::str::contains("reset the atomic counter"));

    let mut reused = command_for_database(&database);
    let assert = reused
        .args(["generate", "--word1-lengths", "4"])
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
fn generate_rejects_suffix_min_length_below_floor() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.args(["generate", "--suffix-min-length", "3"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "suffix minimum length must be between",
        ));
}

#[test]
fn generate_can_disable_word2() {
    let (mut cmd, _tempdir) = command_with_database();
    let assert = cmd
        .args(["generate", "--no-word2", "--no-suffix"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let parts = output.trim().split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].chars().count(), 5);
}

#[test]
fn generate_rejects_all_positions_disabled() {
    let (mut cmd, _tempdir) = command_with_database();

    cmd.args(["generate", "--no-word1", "--no-word2", "--no-suffix"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("at least one position"));
}
