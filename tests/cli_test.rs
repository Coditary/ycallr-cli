//! Integration tests for the `ycallr` binary.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

fn ycallr_cmd() -> Command {
    Command::cargo_bin("ycallr").unwrap()
}

fn with_home(home: &Path) -> Command {
    let mut cmd = ycallr_cmd();
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd
}

fn with_config_dir(config_dir: &Path) -> Command {
    let mut cmd = ycallr_cmd();
    cmd.env("YCALLR_CONFIG_DIR", config_dir);
    cmd.env("HOME", config_dir);
    cmd.env("USERPROFILE", config_dir);
    cmd
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn version_flag_prints_versions() {
    ycallr_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("ycallr"))
        .stdout(predicate::str::contains("ycallr-core"));
}

#[test]
fn help_flag_shows_usage() {
    ycallr_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("ycallr install"))
        .stdout(predicate::str::contains("import-openapi"));
}

#[test]
fn no_args_shows_help() {
    ycallr_cmd()
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn list_with_empty_config() {
    let home = tempfile::tempdir().unwrap();
    with_home(home.path())
        .arg("--list")
        .assert()
        .success()
        .stdout(predicate::str::contains("No installed API profiles"));
}

#[test]
fn install_requires_path() {
    ycallr_cmd()
        .args(["install"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("usage: ycallr install"));
}

#[test]
fn install_rejects_profile_name_without_path() {
    ycallr_cmd()
        .args(["install", "github"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Install requires a path"));
}

#[test]
fn install_help() {
    ycallr_cmd()
        .args(["install", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ycallr install"));
}

#[test]
fn install_and_list_profile() {
    let home = tempfile::tempdir().unwrap();
    let yaml = fixture_path("minimal_api.yaml");

    with_home(home.path())
        .args(["install", yaml.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Installed 'testapi'"));

    with_home(home.path())
        .arg("--list")
        .assert()
        .success()
        .stdout(predicate::str::contains("testapi"))
        .stdout(predicate::str::contains("Minimal test API"));
}

#[test]
fn unknown_api_shows_error() {
    let home = tempfile::tempdir().unwrap();
    with_home(home.path())
        .args(["nonexistent", "--list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error:"));
}

#[test]
fn api_list_subcommands_after_install() {
    let home = tempfile::tempdir().unwrap();
    let yaml = fixture_path("minimal_api.yaml");

    with_home(home.path())
        .args(["install", yaml.to_str().unwrap()])
        .assert()
        .success();

    with_home(home.path())
        .args(["testapi", "--list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ping"));
}

#[test]
fn install_uses_ycallr_config_dir() {
    let config = tempfile::tempdir().unwrap();
    let yaml = fixture_path("minimal_api.yaml");

    with_config_dir(config.path())
        .args(["install", yaml.to_str().unwrap()])
        .assert()
        .success();

    with_config_dir(config.path())
        .arg("--list")
        .assert()
        .success()
        .stdout(predicate::str::contains("testapi"));
}

#[test]
fn verbose_flag_shows_debug_logs_on_install() {
    let home = tempfile::tempdir().unwrap();
    let yaml = fixture_path("minimal_api.yaml");

    with_home(home.path())
        .args(["-v", "install", yaml.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("installing API profile"));
}

#[test]
fn import_openapi_help() {
    ycallr_cmd()
        .args(["import-openapi", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("import-openapi"));
}
