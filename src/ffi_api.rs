//! Safe wrappers around `ycallr.h` — the CLI talks to core only through the C ABI.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::ptr;

use ycallr_core::ffi::{
    ycallr_build_implicit_body_json, ycallr_call, ycallr_client_free, ycallr_client_new,
    ycallr_command_get_body_kind, ycallr_command_get_description, ycallr_command_get_endpoint,
    ycallr_command_get_method, ycallr_command_get_params_json, ycallr_command_get_path_params_json,
    ycallr_command_has_body, ycallr_command_is_branch, ycallr_command_is_leaf, ycallr_free_api,
    ycallr_free_command, ycallr_free_response, ycallr_get_base_url, ycallr_get_command,
    ycallr_get_description, ycallr_get_env_json, ycallr_get_last_error, ycallr_get_last_install_result,
    ycallr_get_name, ycallr_get_version, ycallr_install_yaml_file, ycallr_list_installed,
    ycallr_list_subcommands, ycallr_load_installed, ycallr_missing_params_json,
    ycallr_response_get_body_json, ycallr_response_get_message, ycallr_response_get_status,
    ycallr_string_free,
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
        read_const_string(ycallr_get_name(self.handle as *const _))
            .unwrap_or_default()
    }

    pub fn version(&self) -> String {
        read_const_string(ycallr_get_version(self.handle as *const _))
            .unwrap_or_default()
    }

    pub fn description(&self) -> String {
        read_const_string(ycallr_get_description(self.handle as *const _))
            .unwrap_or_default()
    }

    pub fn base_url(&self) -> String {
        read_const_string(ycallr_get_base_url(self.handle as *const _))
            .unwrap_or_default()
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
            let required = v
                .get("required")
                .and_then(|r| r.as_bool())
                .unwrap_or(true);
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
        let ptr = ycallr_missing_params_json(
            self.handle as *const _,
            path_c.as_ptr(),
            params_c.as_ptr(),
        );
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

    pub fn create_client(&self) -> Result<Client, FfiError> {
        let handle = ycallr_client_new(self.handle as *const _, 0, ptr::null());
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
        let body_ptr = body_c
            .as_ref()
            .map(|c| c.as_ptr())
            .unwrap_or(ptr::null());

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
