//! In-process CLI tests (coverage for `run_with_args`).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;

use ycallr::run_with_args;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

struct TestEnv {
    home: tempfile::TempDir,
    apis: PathBuf,
    secrets: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let apis = home.path().join("apis");
        let secrets = home.path().join("secrets");
        std::fs::create_dir_all(&apis).unwrap();
        Self {
            home,
            apis,
            secrets,
        }
    }

    fn set_env(&self) {
        std::env::set_var("HOME", self.home.path());
        std::env::set_var("USERPROFILE", self.home.path());
        std::env::set_var("YCALLR_CONFIG_DIR", &self.apis);
        std::env::set_var("YCALLR_SECRETS_DIR", &self.secrets);
    }

    fn clear_env(&self) {
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");
        std::env::remove_var("YCALLR_CONFIG_DIR");
        std::env::remove_var("YCALLR_SECRETS_DIR");
    }

    fn run(&self, args: &[&str]) -> ExitCode {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        self.set_env();
        let code = run_with_args(args.iter().map(|s| s.to_string()).collect());
        self.clear_env();
        code
    }
}

fn ok(code: ExitCode) -> bool {
    code == ExitCode::SUCCESS
}

fn usage_error(code: ExitCode) -> bool {
    code == ExitCode::from(1)
}

struct MockInstall {
    _server: mockito::ServerGuard,
    mock: mockito::Mock,
}

impl MockInstall {
    fn assert(&self) {
        self.mock.assert();
    }
}

fn install_with_mock_base(
    env: &TestEnv,
    fixture: &str,
    mock_path: &str,
    expected_calls: usize,
) -> MockInstall {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", mock_path)
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .expect(expected_calls)
        .create();
    let yaml_text = std::fs::read_to_string(fixture_path(fixture)).unwrap();
    let yaml_text = yaml_text.replace("https://example.com", &server.url());
    let yaml_path = env.apis.join(format!("{}.mock.yaml", fixture));
    std::fs::write(&yaml_path, yaml_text).unwrap();
    assert!(ok(env.run(&["install", yaml_path.to_str().unwrap()])));
    MockInstall {
        _server: server,
        mock,
    }
}

#[test]
fn inprocess_help_and_version() {
    let env = TestEnv::new();
    assert!(ok(env.run(&[])));
    assert!(ok(env.run(&["--help"])));
    assert!(ok(env.run(&["--version"])));
}

#[test]
fn inprocess_list_empty() {
    let env = TestEnv::new();
    assert!(ok(env.run(&["--list"])));
}

#[test]
fn inprocess_install_and_list() {
    let env = TestEnv::new();
    let yaml = fixture_path("minimal_api.yaml");
    assert!(ok(env.run(&["install", yaml.to_str().unwrap()])));
    assert!(ok(env.run(&["--list"])));
    assert!(ok(env.run(&["testapi", "--list"])));
    assert!(ok(env.run(&["testapi", "--help"])));
    assert!(ok(env.run(&["testapi", "--tree"])));
    assert!(ok(env.run(&["testapi", "ping", "--help"])));
}

#[test]
fn inprocess_install_errors() {
    let env = TestEnv::new();
    assert!(usage_error(env.run(&["install"])));
    assert!(usage_error(env.run(&["install", "github"])));
    assert!(ok(env.run(&["install", "--help"])));
}

#[test]
fn inprocess_unknown_api() {
    let env = TestEnv::new();
    assert!(usage_error(env.run(&["missing", "--list"])));
}

#[test]
fn inprocess_secret_commands() {
    let env = TestEnv::new();
    assert!(ok(env.run(&["secret", "--help"])));
    assert!(ok(env.run(&["secret", "list"])));
    assert!(usage_error(env.run(&["secret", "set"])));
    assert!(usage_error(env.run(&["secret", "nope"])));
}

#[test]
fn inprocess_import_openapi_help() {
    let env = TestEnv::new();
    assert!(ok(env.run(&["import-openapi", "--help"])));
}

#[test]
fn inprocess_env_api_with_stored_secret() {
    let env = TestEnv::new();
    std::fs::create_dir_all(&env.secrets).unwrap();
    std::fs::write(env.secrets.join("TEST_TOKEN"), "stored-token").unwrap();
    std::fs::write(env.secrets.join(".names"), "TEST_TOKEN\n").unwrap();

    let mock = install_with_mock_base(&env, "env_api.yaml", "/ping", 1);
    assert!(ok(env.run(&["envapi", "ping"])));
    mock.assert();
}

#[test]
fn inprocess_parse_flag_errors() {
    let env = TestEnv::new();
    let yaml = fixture_path("minimal_api.yaml");
    assert!(ok(env.run(&["install", yaml.to_str().unwrap()])));
    assert!(usage_error(env.run(&["testapi", "ping", "--badflag"])));
}

#[test]
fn inprocess_verbose_install() {
    let env = TestEnv::new();
    let yaml = fixture_path("minimal_api.yaml");
    assert!(ok(env.run(&["-v", "install", yaml.to_str().unwrap()])));
}

#[test]
fn inprocess_ping_output_flags() {
    let env = TestEnv::new();
    let mock = install_with_mock_base(&env, "minimal_api.yaml", "/ping", 4);
    assert!(ok(env.run(&["testapi", "ping", "--json"])));
    assert!(ok(env.run(&["testapi", "ping", "--pretty"])));
    assert!(ok(env.run(&["testapi", "ping", "--status"])));
    assert!(ok(env.run(&["testapi", "ping", "--fail-on-error"])));
    mock.assert();
}

#[test]
fn inprocess_command_resolution_errors() {
    let env = TestEnv::new();
    let yaml = fixture_path("minimal_api.yaml");
    assert!(ok(env.run(&["install", yaml.to_str().unwrap()])));
    assert!(usage_error(env.run(&["testapi", "missing"])));
    assert!(usage_error(env.run(&["testapi", "ping", "extra", "path"])));
}

#[test]
fn inprocess_import_openapi_option_errors() {
    let env = TestEnv::new();
    let spec = fixture_path("minimal_openapi.json");
    assert!(usage_error(env.run(&[
        "import-openapi",
        spec.to_str().unwrap(),
        "--name"
    ])));
    assert!(usage_error(env.run(&[
        "import-openapi",
        spec.to_str().unwrap(),
        "--unknown",
    ])));
}

#[test]
fn inprocess_import_openapi_success() {
    let env = TestEnv::new();
    let spec = fixture_path("minimal_openapi.json");
    assert!(ok(env.run(&[
        "import-openapi",
        spec.to_str().unwrap(),
        "--name",
        "miniapi",
    ])));
}

#[test]
fn inprocess_secret_list_after_file_store() {
    let env = TestEnv::new();
    std::fs::create_dir_all(&env.secrets).unwrap();
    std::fs::write(env.secrets.join("YCALLR_TEST_SECRET_D"), "stdin-secret").unwrap();
    std::fs::write(env.secrets.join(".names"), "YCALLR_TEST_SECRET_D\n").unwrap();
    assert!(ok(env.run(&["secret", "list"])));
    assert!(ok(env.run(&["secret", "unset", "YCALLR_TEST_SECRET_D",])));
}
