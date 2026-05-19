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
fn credits_print_license_and_attribution_placeholder() {
    let mut cmd = Command::cargo_bin("hoststamp").expect("binary exists");

    cmd.arg("--credits").assert().success().stdout(
        predicate::str::contains("FSL-1.1-ALv2")
            .and(predicate::str::contains("EFF wordlists: not bundled")),
    );
}
