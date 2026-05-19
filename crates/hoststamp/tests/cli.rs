// SPDX-License-Identifier: FSL-1.1-ALv2

use assert_cmd::Command;
use predicates::prelude::*;

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
fn help_prints_generation_defaults() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("--word-length <WORD_LENGTH>")
            .and(predicate::str::contains("[default: 5]"))
            .and(predicate::str::contains("--suffix-len <SUFFIX_LEN>"))
            .and(predicate::str::contains("[default: eff_short]")),
    );
}

#[test]
fn credits_print_license_and_eff_attribution() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("--credits").assert().success().stdout(
        predicate::str::contains("FSL-1.1-ALv2")
            .and(predicate::str::contains("EFF Long Wordlist"))
            .and(predicate::str::contains("EFF Short Wordlist #1"))
            .and(predicate::str::contains("EFF Short Wordlist #2"))
            .and(predicate::str::contains(
                "Creative Commons Attribution 3.0 United States",
            ))
            .and(predicate::str::contains("Changes: none")),
    );
}

#[test]
fn generate_prints_name_name_hash_by_default() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");
    let assert = cmd.arg("generate").assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let hostname = output.trim();
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 3);
    assert_ne!(parts[0], parts[1]);
    assert!(parts[..2].iter().all(|part| part.chars().count() == 5));
    assert_eq!(parts[2].len(), 5);
    assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn generate_is_the_default_command() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");
    let assert = cmd.assert().success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let hostname = output.trim();
    let parts = hostname.split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 3);
    assert_ne!(parts[0], parts[1]);
    assert!(parts[..2].iter().all(|part| part.chars().count() == 5));
    assert_eq!(parts[2].len(), 5);
    assert!(parts[2].chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn default_generate_accepts_top_level_flags() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");
    let assert = cmd
        .args(["--count", "2", "--word-length", "4", "--no-suffix-hash"])
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
fn serve_accepts_generation_defaults_after_subcommand() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.args(["serve", "--addr", "127.0.0.1:0", "--word-length", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("word length must be at least 1"));
}

#[test]
fn generate_supports_multiple_hostnames() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");
    let assert = cmd
        .args([
            "generate",
            "--count",
            "3",
            "--dictionary",
            "eff_short_2",
            "--no-suffix-hash",
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
fn generate_filters_words_by_exact_length() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");
    let assert = cmd
        .args(["generate", "--word-length", "4", "--no-suffix-hash"])
        .assert()
        .success();
    let output = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8");
    let parts = output.trim().split('-').collect::<Vec<_>>();

    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|part| part.chars().count() == 4));
}

#[test]
fn generate_errors_when_word_filter_has_no_matches() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.args(["generate", "--word-length", "100"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not contain"));
}

#[test]
fn generate_rejects_count_above_cap() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.args(["generate", "--count", "51"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("count must be between 1 and 50"));
}
