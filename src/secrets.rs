//! Local secret store: OS keyring with chmod-600 file fallback.

use std::fs;
use std::path::{Path, PathBuf};

pub const KEYRING_SERVICE: &str = "ycallr";

pub fn secrets_dir() -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("YCALLR_SECRETS_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if let Ok(apis) = std::env::var("YCALLR_CONFIG_DIR") {
        let apis_path = PathBuf::from(apis);
        if let Some(parent) = apis_path.parent() {
            return Ok(parent.join("secrets"));
        }
    }
    let config =
        dirs::config_dir().ok_or_else(|| "could not resolve config directory".to_string())?;
    Ok(config.join("ycallr").join("secrets"))
}

pub fn validate_secret_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("secret name cannot be empty".into());
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "secret name '{}' must be ASCII alphanumeric or underscore",
            name
        ));
    }
    Ok(())
}

fn names_path(dir: &Path) -> PathBuf {
    dir.join(".names")
}

fn secret_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(name)
}

fn ensure_secrets_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("failed to create secrets dir: {}", e))?;
    Ok(())
}

fn set_permissions_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

fn update_names_manifest(dir: &Path, name: &str, add: bool) -> Result<(), String> {
    let path = names_path(dir);
    let mut names: Vec<String> = if path.exists() {
        fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };
    if add {
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
            names.sort();
        }
    } else {
        names.retain(|n| n != name);
    }
    if names.is_empty() {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("failed to remove names manifest: {}", e))?;
        }
        return Ok(());
    }
    let body = format!("{}\n", names.join("\n"));
    fs::write(&path, body).map_err(|e| format!("failed to write names manifest: {}", e))?;
    set_permissions_private(&path);
    Ok(())
}

pub fn set_secret_file(name: &str, value: &str) -> Result<(), String> {
    validate_secret_name(name)?;
    if value.trim().is_empty() {
        return Err(format!("secret '{}' cannot be empty", name));
    }
    let dir = secrets_dir()?;
    ensure_secrets_dir(&dir)?;
    let path = secret_path(&dir, name);
    fs::write(&path, value).map_err(|e| format!("failed to write secret file: {}", e))?;
    set_permissions_private(&path);
    update_names_manifest(&dir, name, true)?;
    Ok(())
}

pub fn get_secret_file(name: &str) -> Option<String> {
    let dir = secrets_dir().ok()?;
    let path = secret_path(&dir, name);
    let content = fs::read_to_string(&path).ok()?;
    let value = content.trim_end_matches(['\n', '\r']).to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub fn list_secret_names() -> Result<Vec<String>, String> {
    let dir = secrets_dir()?;
    let path = names_path(&dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let names = fs::read_to_string(&path)
        .map_err(|e| format!("failed to read names manifest: {}", e))?
        .lines()
        .filter(|l| !l.is_empty())
        .map(|s| s.to_string())
        .collect();
    Ok(names)
}

pub fn unset_secret_file(name: &str) -> Result<(), String> {
    validate_secret_name(name)?;
    let dir = secrets_dir()?;
    let path = secret_path(&dir, name);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("failed to remove secret file: {}", e))?;
    }
    update_names_manifest(&dir, name, false)?;
    Ok(())
}

fn keyring_entry(name: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, name).map_err(|e| format!("keyring entry error: {}", e))
}

pub fn set_secret(name: &str, value: &str) -> Result<(), String> {
    validate_secret_name(name)?;
    if value.trim().is_empty() {
        return Err(format!("secret '{}' cannot be empty", name));
    }

    let keyring_stored = match keyring_entry(name) {
        Ok(entry) => {
            entry.set_password(value).is_ok() && entry.get_password().ok().as_deref() == Some(value)
        }
        Err(_) => false,
    };

    if keyring_stored {
        tracing::debug!(name = name, "stored secret in OS keyring");
    } else {
        tracing::debug!(name = name, "keyring unavailable; using file store");
    }

    // Always keep a chmod-600 file copy so reads work when keyring is flaky (CI, SSH, WSL).
    set_secret_file(name, value)
}

pub fn get_secret(name: &str) -> Option<String> {
    if let Ok(entry) = keyring_entry(name) {
        if let Ok(value) = entry.get_password() {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    get_secret_file(name)
}

pub fn unset_secret(name: &str) -> Result<(), String> {
    validate_secret_name(name)?;
    if let Ok(entry) = keyring_entry(name) {
        let _ = entry.delete_credential();
    }
    unset_secret_file(name)?;
    Ok(())
}

pub fn warn_insecure_secret_files() {
    let dir = match secrets_dir() {
        Ok(d) => d,
        Err(_) => return,
    };
    if !dir.is_dir() {
        return;
    }
    let entries = fs::read_dir(&dir).ok();
    let Some(entries) = entries else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&path) {
                if meta.permissions().mode() & 0o077 != 0 {
                    tracing::warn!(
                        path = %path.display(),
                        "secret file is readable by group/others; consider chmod 600"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn with_secrets_dir<F: FnOnce()>(test: F) {
    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("YCALLR_SECRETS_DIR", dir.path());
    test();
    std::env::remove_var("YCALLR_SECRETS_DIR");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_dir_uses_config_parent() {
        use std::sync::Mutex;
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let apis = home.path().join("apis");
        std::fs::create_dir_all(&apis).unwrap();
        std::env::remove_var("YCALLR_SECRETS_DIR");
        std::env::set_var("YCALLR_CONFIG_DIR", &apis);
        let dir = secrets_dir().unwrap();
        assert_eq!(dir, home.path().join("secrets"));
        std::env::remove_var("YCALLR_CONFIG_DIR");
    }

    #[test]
    fn validate_secret_name_rejects_invalid() {
        assert!(validate_secret_name("GITHUB_TOKEN").is_ok());
        assert!(validate_secret_name("bad-name").is_err());
        assert!(validate_secret_name("").is_err());
    }

    #[test]
    fn set_and_get_secret_file_roundtrip() {
        with_secrets_dir(|| {
            set_secret_file("YCALLR_TEST_SECRET_A", "ghp_test").unwrap();
            assert_eq!(
                get_secret_file("YCALLR_TEST_SECRET_A").as_deref(),
                Some("ghp_test")
            );
            let names = list_secret_names().unwrap();
            assert!(names.contains(&"YCALLR_TEST_SECRET_A".to_string()));
            unset_secret_file("YCALLR_TEST_SECRET_A").unwrap();
            assert!(get_secret_file("YCALLR_TEST_SECRET_A").is_none());
        });
    }

    #[test]
    fn warn_insecure_secret_file_when_world_readable() {
        with_secrets_dir(|| {
            let dir = secrets_dir().unwrap();
            let path = dir.join("TOKEN");
            std::fs::write(&path, "x").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));
            }
            warn_insecure_secret_files();
        });
    }

    #[test]
    fn set_secret_writes_file_when_keyring_unavailable_or_fallback() {
        with_secrets_dir(|| {
            set_secret("YCALLR_TEST_SECRET_B", "secret-value").unwrap();
            assert_eq!(
                get_secret("YCALLR_TEST_SECRET_B").as_deref(),
                Some("secret-value")
            );
            unset_secret("YCALLR_TEST_SECRET_B").unwrap();
            assert!(get_secret("YCALLR_TEST_SECRET_B").is_none());
        });
    }
}
