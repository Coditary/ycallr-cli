//! Safe wrappers around `ycallr.h` — the CLI talks to core only through the C ABI.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::ptr;

use ycallr_core::ffi::{
    ycallr_build_implicit_body_json, ycallr_call, ycallr_client_free, ycallr_client_new,
    ycallr_command_get_body_kind, ycallr_command_get_description, ycallr_command_get_endpoint,
    ycallr_command_get_method, ycallr_command_get_params_json, ycallr_command_get_path_params_json,
    ycallr_command_has_body, ycallr_command_is_branch, ycallr_command_is_leaf, ycallr_free_api,
    ycallr_free_command, ycallr_free_response, ycallr_get_base_url, ycallr_get_command,
    ycallr_get_description, ycallr_get_env_json, ycallr_get_last_error,
    ycallr_get_last_import_result, ycallr_get_last_install_result, ycallr_get_name,
    ycallr_get_version, ycallr_import_openapi_file, ycallr_install_yaml_file,
    ycallr_list_installed, ycallr_list_subcommands, ycallr_load_installed,
    ycallr_missing_params_json, ycallr_response_get_body_json, ycallr_response_get_message,
    ycallr_response_get_status, ycallr_set_base_url, ycallr_string_free,
};

pub type ApiHandle = *mut c_void;
pub type CommandHandle = *mut c_void;
pub type ClientHandle = *mut c_void;
pub type ResponseHandle = *mut c_void;

#[derive(Debug, Clone)]
pub struct EnvVarInfo {
    pub name: String,
    pub required: bool,
}

#[derive(Debug)]
pub struct FfiError(String);

impl FfiError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl std::fmt::Display for FfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn last_error() -> String {
    let ptr = ycallr_get_last_error();
    if ptr.is_null() {
        return "unknown error".to_string();
    }
    unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}

fn cstring(s: &str) -> CString {
    CString::new(s).expect("NUL in string")
}

fn take_string(ptr: *mut c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() };
    ycallr_string_free(ptr);
    Some(s)
}

fn read_const_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() })
    }
}

pub struct Api {
    handle: ApiHandle,
}

impl Api {
    pub fn load_installed(name: &str) -> Result<Self, FfiError> {
        let name = cstring(name);
        let handle = ycallr_load_installed(name.as_ptr());
        if handle.is_null() {
            return Err(FfiError(last_error()));
        }
        Ok(Self {
            handle: handle as ApiHandle,
        })
    }

    pub fn name(&self) -> String {
        read_const_string(ycallr_get_name(self.handle as *const _)).unwrap_or_default()
    }

    pub fn version(&self) -> String {
        read_const_string(ycallr_get_version(self.handle as *const _)).unwrap_or_default()
    }

    pub fn description(&self) -> String {
        read_const_string(ycallr_get_description(self.handle as *const _)).unwrap_or_default()
    }

    pub fn base_url(&self) -> String {
        read_const_string(ycallr_get_base_url(self.handle as *const _)).unwrap_or_default()
    }

    #[allow(dead_code)]
    pub fn set_base_url(&self, url: &str) -> Result<(), FfiError> {
        let url_c = cstring(url);
        let rc = ycallr_set_base_url(self.handle as *mut _, url_c.as_ptr());
        if rc != 0 {
            return Err(FfiError(last_error()));
        }
        Ok(())
    }

    pub fn env_vars(&self) -> Result<Vec<EnvVarInfo>, FfiError> {
        let ptr = ycallr_get_env_json(self.handle as *const _);
        if ptr.is_null() {
            return Err(FfiError(last_error()));
        }
        let json = take_string(ptr).unwrap_or_else(|| "[]".to_string());
        let values: Vec<serde_json::Value> =
            serde_json::from_str(&json).map_err(|e| FfiError(e.to_string()))?;
        let mut out = Vec::new();
        for v in values {
            let name = v
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_default()
                .to_string();
            let required = v.get("required").and_then(|r| r.as_bool()).unwrap_or(true);
            out.push(EnvVarInfo { name, required });
        }
        Ok(out)
    }

    pub fn list_command_names(&self, path: &str) -> Result<Vec<String>, FfiError> {
        let path_c = cstring(path);
        let json_ptr = ycallr_list_subcommands(self.handle as *const _, path_c.as_ptr());
        let json = take_string(json_ptr).unwrap_or_else(|| "[]".to_string());
        serde_json::from_str(&json).map_err(|e| FfiError(e.to_string()))
    }

    pub fn get_command(&self, path: &str) -> Result<Command, FfiError> {
        let path_c = cstring(path);
        let handle = ycallr_get_command(self.handle as *const _, path_c.as_ptr());
        if handle.is_null() {
            return Err(FfiError(last_error()));
        }
        Ok(Command {
            handle: handle as CommandHandle,
        })
    }

    pub fn missing_params(
        &self,
        command_path: &str,
        params_json: &str,
    ) -> Result<Vec<String>, FfiError> {
        let path_c = cstring(command_path);
        let params_c = cstring(params_json);
        let ptr =
            ycallr_missing_params_json(self.handle as *const _, path_c.as_ptr(), params_c.as_ptr());
        if ptr.is_null() {
            return Err(FfiError(last_error()));
        }
        let json = take_string(ptr).unwrap_or_else(|| "[]".to_string());
        serde_json::from_str(&json).map_err(|e| FfiError(e.to_string()))
    }

    pub fn build_implicit_body(&self, command_path: &str, params_json: &str) -> Option<String> {
        let path_c = cstring(command_path);
        let params_c = cstring(params_json);
        let ptr = ycallr_build_implicit_body_json(
            self.handle as *const _,
            path_c.as_ptr(),
            params_c.as_ptr(),
        );
        take_string(ptr)
    }

    #[allow(dead_code)]
    pub fn create_client(&self) -> Result<Client, FfiError> {
        self.create_client_with_envs(&HashMap::new())
    }

    pub fn create_client_with_envs(
        &self,
        envs: &HashMap<String, String>,
    ) -> Result<Client, FfiError> {
        let json = serde_json::to_string(envs).map_err(|e| FfiError(e.to_string()))?;
        let json_c = cstring(&json);
        let handle = ycallr_client_new(self.handle as *const _, 0, json_c.as_ptr());
        if handle.is_null() {
            return Err(FfiError(last_error()));
        }
        Ok(Client {
            handle: handle as ClientHandle,
        })
    }
}

impl Drop for Api {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            ycallr_free_api(self.handle as *mut _);
        }
    }
}

pub struct Command {
    handle: CommandHandle,
}

impl Command {
    pub fn endpoint(&self) -> Option<String> {
        take_string(ycallr_command_get_endpoint(self.handle as *const _))
    }

    pub fn method(&self) -> Option<String> {
        take_string(ycallr_command_get_method(self.handle as *const _))
    }

    pub fn description(&self) -> Option<String> {
        take_string(ycallr_command_get_description(self.handle as *const _))
    }

    pub fn params_json(&self) -> String {
        take_string(ycallr_command_get_params_json(self.handle as *const _))
            .unwrap_or_else(|| "{}".to_string())
    }

    pub fn path_params_json(&self) -> String {
        take_string(ycallr_command_get_path_params_json(self.handle as *const _))
            .unwrap_or_else(|| "[]".to_string())
    }

    pub fn is_leaf(&self) -> bool {
        ycallr_command_is_leaf(self.handle as *const _)
    }

    pub fn is_branch(&self) -> bool {
        ycallr_command_is_branch(self.handle as *const _)
    }

    pub fn has_body(&self) -> bool {
        ycallr_command_has_body(self.handle as *const _)
    }

    pub fn body_kind(&self) -> Option<String> {
        take_string(ycallr_command_get_body_kind(self.handle as *const _))
    }
}

impl Drop for Command {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            ycallr_free_command(self.handle as *mut _);
        }
    }
}

pub struct Client {
    handle: ClientHandle,
}

impl Client {
    pub fn call(
        &self,
        command: &str,
        params_json: &str,
        body_json: Option<&str>,
    ) -> Result<Response, FfiError> {
        let command_c = cstring(command);
        let params_c = cstring(params_json);
        let body_c = body_json.map(cstring);
        let body_ptr = body_c.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null());

        let resp = ycallr_call(
            self.handle as *const _,
            command_c.as_ptr(),
            params_c.as_ptr(),
            body_ptr,
        );
        if resp.is_null() {
            return Err(FfiError(last_error()));
        }
        Ok(Response {
            handle: resp as ResponseHandle,
        })
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            ycallr_client_free(self.handle as *mut _);
        }
    }
}

pub struct Response {
    handle: ResponseHandle,
}

impl Response {
    pub fn status(&self) -> u16 {
        ycallr_response_get_status(self.handle as *const _)
    }

    pub fn body_json(&self) -> String {
        take_string(ycallr_response_get_body_json(self.handle as *const _))
            .unwrap_or_else(|| "{}".to_string())
    }

    pub fn message(&self) -> Option<String> {
        take_string(ycallr_response_get_message(self.handle as *const _))
    }
}

impl Drop for Response {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            ycallr_free_response(self.handle as *mut _);
        }
    }
}

pub fn install_profile_file(path: &str) -> Result<(String, String), FfiError> {
    let path_c = cstring(path);
    let rc = ycallr_install_yaml_file(path_c.as_ptr());
    if rc != 0 {
        return Err(FfiError(last_error()));
    }
    let json = take_string(ycallr_get_last_install_result()).unwrap_or_else(|| "{}".to_string());
    let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| FfiError(e.to_string()))?;
    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or_default()
        .to_string();
    let pb_path = v
        .get("pb_path")
        .and_then(|p| p.as_str())
        .unwrap_or_default()
        .to_string();
    Ok((name, pb_path))
}

#[allow(clippy::too_many_arguments)]
pub fn import_openapi_file(
    source: &str,
    output: Option<&str>,
    name: Option<&str>,
    tag: Option<&str>,
    base_url: Option<&str>,
    nest_by: Option<&str>,
    short_names: bool,
    preset: Option<&str>,
) -> Result<(String, String), FfiError> {
    let source_c = cstring(source);
    let output_c = output.map(cstring);
    let name_c = name.map(cstring);
    let tag_c = tag.map(cstring);
    let base_url_c = base_url.map(cstring);
    let nest_by_c = nest_by.map(cstring);
    let preset_c = preset.map(cstring);
    let short_names_flag: i32 = if short_names { 1 } else { 0 };

    let rc = ycallr_import_openapi_file(
        source_c.as_ptr(),
        output_c.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null()),
        name_c.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null()),
        tag_c.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null()),
        base_url_c
            .as_ref()
            .map(|c| c.as_ptr())
            .unwrap_or(ptr::null()),
        nest_by_c
            .as_ref()
            .map(|c| c.as_ptr())
            .unwrap_or(ptr::null()),
        short_names_flag,
        preset_c.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null()),
    );
    if rc != 0 {
        return Err(FfiError(last_error()));
    }
    let json = take_string(ycallr_get_last_import_result()).unwrap_or_else(|| "{}".to_string());
    let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| FfiError(e.to_string()))?;
    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or_default()
        .to_string();
    let yaml_path = v
        .get("yaml_path")
        .and_then(|p| p.as_str())
        .unwrap_or_default()
        .to_string();
    Ok((name, yaml_path))
}

pub fn list_installed() -> Result<Vec<(String, String)>, FfiError> {
    let json_ptr = ycallr_list_installed();
    if json_ptr.is_null() {
        return Err(FfiError(last_error()));
    }
    let names_json = take_string(json_ptr).unwrap_or_else(|| "[]".to_string());
    let names: Vec<String> =
        serde_json::from_str(&names_json).map_err(|e| FfiError(e.to_string()))?;

    let mut out = Vec::new();
    for name in names {
        let api = Api::load_installed(&name)?;
        let desc = api.description();
        out.push((name, desc));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    fn with_config_dir(dir: &std::path::Path, test: impl FnOnce()) {
        use std::sync::Mutex;
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("YCALLR_CONFIG_DIR", dir);
        std::env::set_var("HOME", dir);
        std::env::set_var("USERPROFILE", dir);
        test();
        std::env::remove_var("YCALLR_CONFIG_DIR");
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");
    }

    #[test]
    fn load_installed_missing_profile() {
        let dir = tempfile::tempdir().unwrap();
        with_config_dir(dir.path(), || {
            assert!(Api::load_installed("missing").is_err());
        });
    }

    #[test]
    fn install_load_and_inspect_profile() {
        let dir = tempfile::tempdir().unwrap();
        with_config_dir(dir.path(), || {
            let yaml = fixture("minimal_api.yaml");
            let (name, _pb) = install_profile_file(yaml.to_str().unwrap()).unwrap();
            assert_eq!(name, "testapi");

            let api = Api::load_installed("testapi").unwrap();
            assert_eq!(api.name(), "testapi");
            assert!(!api.version().is_empty());
            assert!(!api.description().is_empty());
            assert!(!api.base_url().is_empty());
            assert!(api.env_vars().unwrap().is_empty());

            let names = api.list_command_names("").unwrap();
            assert!(names.contains(&"ping".to_string()));

            let cmd = api.get_command("ping").unwrap();
            assert!(cmd.is_leaf());
            assert_eq!(cmd.method().as_deref(), Some("GET"));
            assert_eq!(cmd.endpoint().as_deref(), Some("/ping"));

            let missing = api.missing_params("ping", "{}").unwrap();
            assert!(missing.is_empty());

            let client = api.create_client_with_envs(&HashMap::new()).unwrap();
            let _ = client.call("ping", "{}", None);
        });
    }

    #[test]
    fn list_installed_profiles() {
        let dir = tempfile::tempdir().unwrap();
        with_config_dir(dir.path(), || {
            assert!(list_installed().unwrap().is_empty());
            let yaml = fixture("minimal_api.yaml");
            install_profile_file(yaml.to_str().unwrap()).unwrap();
            let listed = list_installed().unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].0, "testapi");
        });
    }

    #[test]
    fn ffi_error_display() {
        let err = FfiError::new("boom");
        assert_eq!(err.to_string(), "boom");
    }

    #[test]
    fn install_missing_file_errors() {
        assert!(install_profile_file("/no/such/profile.yaml").is_err());
    }

    #[test]
    fn get_command_missing_errors() {
        let dir = tempfile::tempdir().unwrap();
        with_config_dir(dir.path(), || {
            let yaml = fixture("minimal_api.yaml");
            install_profile_file(yaml.to_str().unwrap()).unwrap();
            let api = Api::load_installed("testapi").unwrap();
            assert!(api.get_command("missing").is_err());
        });
    }

    #[test]
    fn import_openapi_minimal_fixture() {
        let spec = fixture("minimal_openapi.json");
        let (name, yaml_path) = import_openapi_file(
            spec.to_str().unwrap(),
            None,
            Some("miniapi"),
            None,
            None,
            None,
            false,
            None,
        )
        .unwrap();
        assert_eq!(name, "miniapi");
        assert!(yaml_path.ends_with(".yaml"));
    }

    #[test]
    fn import_openapi_missing_file_errors() {
        assert!(import_openapi_file(
            "/no/such/spec.json",
            None,
            None,
            None,
            None,
            None,
            false,
            None,
        )
        .is_err());
    }

    #[test]
    fn client_call_and_response_accessors() {
        let dir = tempfile::tempdir().unwrap();
        with_config_dir(dir.path(), || {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("GET", "/ping")
                .with_status(200)
                .with_body(r#"{"ok":true}"#)
                .create();

            let yaml = fixture("env_api.yaml");
            install_profile_file(yaml.to_str().unwrap()).unwrap();
            let api = Api::load_installed("envapi").unwrap();
            api.set_base_url(&server.url()).unwrap();
            let client = api
                .create_client_with_envs(&HashMap::from([(
                    "TEST_TOKEN".to_string(),
                    "tok".to_string(),
                )]))
                .unwrap();
            let response = client.call("ping", "{}", None).unwrap();
            assert_eq!(response.status(), 200);
            let _ = response.body_json();
            let _ = response.message();
            mock.assert();
        });
    }

    #[test]
    fn set_base_url_rejects_empty() {
        let dir = tempfile::tempdir().unwrap();
        with_config_dir(dir.path(), || {
            let yaml = fixture("minimal_api.yaml");
            install_profile_file(yaml.to_str().unwrap()).unwrap();
            let api = Api::load_installed("testapi").unwrap();
            assert!(api.set_base_url("").is_err());
        });
    }

    #[test]
    fn missing_params_detects_required_fields() {
        let dir = tempfile::tempdir().unwrap();
        with_config_dir(dir.path(), || {
            let yaml = fixture("rich_api.yaml");
            install_profile_file(yaml.to_str().unwrap()).unwrap();
            let api = Api::load_installed("richapi").unwrap();
            let missing = api.missing_params("repos.issues.open", "{}").unwrap();
            assert!(missing.contains(&"owner".to_string()));
            assert!(missing.contains(&"repo".to_string()));
        });
    }
}
