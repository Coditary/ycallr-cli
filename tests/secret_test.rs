//! Integration tests for `ycallr secret`.

use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;

fn ycallr_with_secrets_dir(dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("ycallr").unwrap();
    cmd.env("YCALLR_SECRETS_DIR", dir);
    cmd.env("HOME", dir);
    cmd.env("USERPROFILE", dir);
    cmd
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn with_config_dir(config_dir: &Path) -> Command {
    let mut cmd = Command::cargo_bin("ycallr").unwrap();
    cmd.env("YCALLR_CONFIG_DIR", config_dir);
    cmd.env("HOME", config_dir);
    cmd.env("USERPROFILE", config_dir);
    cmd
}

#[test]
fn secret_help() {
    Command::cargo_bin("ycallr")
        .unwrap()
        .args(["secret", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("secret set"));
}

#[test]
fn secret_set_list_unset_file_backend() {
    let dir = tempfile::tempdir().unwrap();
    ycallr_with_secrets_dir(dir.path())
        .args(["secret", "set", "GITHUB_TOKEN"])
        .write_stdin("ghp_test\n")
        .assert()
        .success();

    ycallr_with_secrets_dir(dir.path())
        .args(["secret", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("GITHUB_TOKEN"));

    ycallr_with_secrets_dir(dir.path())
        .args(["secret", "unset", "GITHUB_TOKEN"])
        .assert()
        .success();

    ycallr_with_secrets_dir(dir.path())
        .args(["secret", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("GITHUB_TOKEN").not());
}

#[test]
fn stored_secret_satisfies_required_env() {
    let home = tempfile::tempdir().unwrap();
    let secrets_dir = home.path().join("secrets");
    std::fs::create_dir_all(&secrets_dir).unwrap();
    std::fs::write(secrets_dir.join("TEST_TOKEN"), "stored-token").unwrap();
    std::fs::write(secrets_dir.join(".names"), "TEST_TOKEN\n").unwrap();

    let apis = home.path().join("apis");
    std::fs::create_dir_all(&apis).unwrap();

    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/ping")
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .create();

    let yaml_text = std::fs::read_to_string(fixture_path("env_api.yaml")).unwrap();
    let yaml_text = yaml_text.replace("https://example.com", &server.url());
    let yaml_path = apis.join("env_api.mock.yaml");
    std::fs::write(&yaml_path, yaml_text).unwrap();

    let mut cmd = with_config_dir(&apis);
    cmd.env("YCALLR_SECRETS_DIR", &secrets_dir);
    cmd.args(["install", yaml_path.to_str().unwrap()])
        .assert()
        .success();

    let mut run = with_config_dir(&apis);
    run.env("YCALLR_SECRETS_DIR", &secrets_dir);
    run.args(["envapi", "ping"])
        .assert()
        .success()
        .stderr(predicate::str::contains("TEST_TOKEN is not set").not());
    mock.assert();
}
