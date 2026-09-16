//! Resolve secrets without requiring plaintext values in the process environment.

use std::collections::HashMap;
use std::io::IsTerminal;

use crate::ffi_api::{Api, FfiError};
use crate::secrets;

pub struct GlobalOptions {
    pub env_files: Vec<String>,
}

/// Extract `--env-file <path>` and other global flags from argv.
pub fn extract_global_options(args: &[String]) -> (Vec<String>, GlobalOptions) {
    let mut remaining = Vec::new();
    let mut opts = GlobalOptions {
        env_files: Vec::new(),
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--env-file" => {
                i += 1;
                if i >= args.len() {
                    break;
                }
                opts.env_files.push(args[i].clone());
            }
            "--verbose" | "-v" => {}
            other => remaining.push(other.to_string()),
        }
        i += 1;
    }

    (remaining, opts)
}

pub fn load_env_file(path: &str) -> Result<HashMap<String, String>, FfiError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| FfiError::new(format!("failed to read --env-file '{}': {}", path, e)))?;

    let mut map = HashMap::new();
    for (line_no, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(FfiError::new(format!(
                "--env-file '{}': line {}: expected KEY=VALUE",
                path,
                line_no + 1
            )));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(FfiError::new(format!(
                "--env-file '{}': line {}: empty key",
                path,
                line_no + 1
            )));
        }
        map.insert(key.to_string(), value.to_string());
    }

    Ok(map)
}

pub fn load_env_files(paths: &[String]) -> Result<HashMap<String, String>, FfiError> {
    let mut merged = HashMap::new();
    for path in paths {
        for (key, value) in load_env_file(path)? {
            merged.insert(key, value);
        }
    }
    Ok(merged)
}

fn env_var_available(name: &str) -> bool {
    ycallr_core::call_engine::read_env_value(name).is_some()
}

fn prompt_secret(name: &str) -> Result<String, FfiError> {
    eprint!("{} (required, input hidden): ", name);
    let value = rpassword::read_password()
        .map_err(|e| FfiError::new(format!("failed to read {}: {}", name, e)))?;
    if value.is_empty() {
        return Err(FfiError::new(format!(
            "required environment variable '{}' cannot be empty",
            name
        )));
    }
    Ok(value)
}

/// Load secrets from the built-in store for names not already set elsewhere.
pub fn load_stored_secrets(names: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for name in names {
        if env_var_available(name) {
            continue;
        }
        if let Some(value) = secrets::get_secret(name) {
            map.insert(name.clone(), value);
        }
    }
    map
}

/// Build env overrides from `--env-file`, built-in store, and interactive prompts (no echo).
pub fn build_env_overrides(
    api: &Api,
    file_envs: &HashMap<String, String>,
) -> Result<HashMap<String, String>, FfiError> {
    let mut overrides = file_envs.clone();
    let envs = api.env_vars()?;

    let required_names: Vec<String> = envs
        .iter()
        .filter(|e| e.required)
        .map(|e| e.name.clone())
        .collect();
    for (key, value) in load_stored_secrets(&required_names) {
        if !overrides.contains_key(&key) && !env_var_available(&key) {
            overrides.insert(key, value);
        }
    }

    for env in envs {
        if !env.required {
            continue;
        }
        if env_var_available(&env.name) || overrides.contains_key(&env.name) {
            continue;
        }
        if std::io::stdin().is_terminal() {
            let name = env.name.clone();
            overrides.insert(name, prompt_secret(&env.name)?);
        }
    }

    Ok(overrides)
}

pub fn warn_insecure_env_file(path: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            if meta.permissions().mode() & 0o077 != 0 {
                tracing::warn!(
                    path = path,
                    "env file is readable by group/others; consider chmod 600"
                );
            }
        }
    }
}

pub fn warn_insecure_env_files(paths: &[String]) {
    for path in paths {
        warn_insecure_env_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_env_file_parses_key_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.env");
        std::fs::write(&path, "# comment\nAPI_TOKEN=abc\n").unwrap();
        let map = load_env_file(path.to_str().unwrap()).unwrap();
        assert_eq!(map.get("API_TOKEN"), Some(&"abc".to_string()));
    }

    #[test]
    fn extract_global_options_splits_env_file() {
        let (args, opts) = extract_global_options(&[
            "--env-file".into(),
            "/tmp/a.env".into(),
            "-v".into(),
            "install".into(),
            "x.yaml".into(),
        ]);
        assert_eq!(opts.env_files, vec!["/tmp/a.env"]);
        assert_eq!(args, vec!["install", "x.yaml"]);
    }

    #[test]
    fn load_env_file_rejects_invalid_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.env");
        std::fs::write(&path, "NOEQUALS\n").unwrap();
        assert!(load_env_file(path.to_str().unwrap()).is_err());
    }

    #[test]
    fn load_stored_secrets_reads_builtin_store() {
        secrets::with_secrets_dir(|| {
            secrets::set_secret("YCALLR_TEST_SECRET_C", "from-store").unwrap();

            let map = load_stored_secrets(&["YCALLR_TEST_SECRET_C".to_string()]);
            assert_eq!(
                map.get("YCALLR_TEST_SECRET_C").map(String::as_str),
                Some("from-store")
            );

            secrets::unset_secret("YCALLR_TEST_SECRET_C").unwrap();
        });
    }

    #[test]
    fn load_env_file_rejects_empty_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.env");
        std::fs::write(&path, "=value\n").unwrap();
        assert!(load_env_file(path.to_str().unwrap()).is_err());
    }

    #[test]
    fn load_env_files_merges_multiple_paths() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.env");
        let b = dir.path().join("b.env");
        std::fs::write(&a, "A=1\n").unwrap();
        std::fs::write(&b, "B=2\n").unwrap();
        let map = load_env_files(&[
            a.to_str().unwrap().to_string(),
            b.to_str().unwrap().to_string(),
        ])
        .unwrap();
        assert_eq!(map.get("A"), Some(&"1".to_string()));
        assert_eq!(map.get("B"), Some(&"2".to_string()));
    }

    #[test]
    fn warn_insecure_env_file_on_world_readable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("open.env");
        std::fs::write(&path, "X=1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));
        }
        warn_insecure_env_file(path.to_str().unwrap());
        warn_insecure_env_files(&[path.to_str().unwrap().to_string()]);
    }
}
