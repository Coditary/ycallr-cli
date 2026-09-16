mod env_secrets;
mod ffi_api;
mod logging;
mod secret_cmd;
mod secrets;

use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;

use ffi_api::{Api, Command, FfiError};

const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 1;
const EXIT_HTTP: u8 = 2;

fn die_code(err: FfiError, code: u8) -> ExitCode {
    eprintln!("error: {}", err);
    ExitCode::from(code)
}

fn die(err: FfiError) -> ExitCode {
    die_code(err, EXIT_USAGE)
}

fn path_to_words(path: &str) -> String {
    path.replace('.', " ")
}

fn has_subcommands(api: &Api, command_path: &str) -> bool {
    api.list_command_names(command_path)
        .map(|names| !names.is_empty())
        .unwrap_or(false)
}

/// Branch node with children but no callable endpoint at this level.
fn is_group_only(api: &Api, command_path: &str, cmd: &Command) -> bool {
    !cmd.is_leaf() && has_subcommands(api, command_path)
}

fn format_subcommand_hint(api: &Api, command_path: &str) -> String {
    let children = api.list_command_names(command_path).unwrap_or_default();
    if children.is_empty() {
        return String::new();
    }
    let examples: Vec<String> = children
        .iter()
        .take(5)
        .map(|child| path_to_words(&format!("{}.{}", command_path, child)))
        .collect();
    format!(
        "Subcommands of '{}': {}",
        path_to_words(command_path),
        examples.join(", ")
    )
}

fn full_url(base_url: &str, endpoint: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if endpoint.starts_with('/') {
        format!("{}{}", base, endpoint)
    } else if endpoint.is_empty() {
        base.to_string()
    } else {
        format!("{}/{}", base, endpoint)
    }
}

fn looks_like_install_path(source: &str) -> bool {
    source.starts_with('~')
        || source.contains('/')
        || source.contains('\\')
        || source.ends_with(".yaml")
        || source.ends_with(".yml")
        || Path::new(source).is_absolute()
}

fn install_api(source: &str) -> Result<(), ExitCode> {
    if !looks_like_install_path(source) {
        return Err(die(FfiError::new(format!(
            "Install requires a path to a YAML file.\n\
             Example: ycallr install ~/.config/ycallr/apis/github.yaml\n\
             Got '{}': pass the file path, not just the profile name.",
            source
        ))));
    }
    tracing::debug!(source = %source, "installing API profile");
    let (name, pb_path) = ffi_api::install_profile_file(source).map_err(die)?;
    println!("Installed '{}' -> {}", name, pb_path);
    Ok(())
}

fn print_installed_apis() -> Result<(), ExitCode> {
    let apis = ffi_api::list_installed().map_err(die)?;
    if apis.is_empty() {
        println!("No installed API profiles (.pb). Run: ycallr install <path/to/profile.yaml>");
        return Ok(());
    }
    for (name, desc) in apis {
        println!("  {:<25} {}", name, desc);
    }
    Ok(())
}

fn print_version() {
    println!("ycallr {} (ffi-cli)", env!("CARGO_PKG_VERSION"));
    println!("ycallr-core {}", ycallr_core::VERSION);
}

fn collect_all_paths(api: &Api, prefix: &str, out: &mut Vec<String>) {
    let Ok(names) = api.list_command_names(prefix) else {
        return;
    };
    for name in names {
        let full = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };
        out.push(full.clone());
        if api
            .get_command(&full)
            .map(|c| c.is_branch())
            .unwrap_or(false)
        {
            collect_all_paths(api, &full, out);
        }
    }
}

fn suggest_paths(api: &Api, input: &str) -> Vec<String> {
    let mut all = Vec::new();
    collect_all_paths(api, "", &mut all);
    let needle = input.replace(' ', ".");
    let mut scored: Vec<(usize, String)> = all
        .into_iter()
        .map(|p| (levenshtein(&needle, &p), p))
        .filter(|(d, _)| *d <= 3)
        .collect();
    scored.sort_by_key(|(d, p)| (*d, p.clone()));
    scored.into_iter().take(3).map(|(_, p)| p).collect()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur.push((prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn print_direct_children(api: &Api, prefix: &str) -> Result<(), ExitCode> {
    let names = api.list_command_names(prefix).map_err(die)?;

    if names.is_empty() {
        println!("(no subcommands)");
        return Ok(());
    }

    for name in names {
        let full = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };
        let cmd = api.get_command(&full).map_err(die)?;
        let display = path_to_words(&full);
        let method = cmd
            .method()
            .map(|m| format!("{:<7}", m))
            .unwrap_or_else(|| "       ".to_string());
        let desc = cmd
            .description()
            .unwrap_or_else(|| "no description".to_string());
        let kind = if cmd.is_branch() && cmd.is_leaf() {
            " (callable + group)"
        } else if cmd.is_branch() {
            " (group)"
        } else {
            ""
        };
        println!("  {} {:<32} {}{}", method, display, desc, kind);
    }
    Ok(())
}

fn walk_command_tree(api: &Api, prefix: &str, depth: usize) -> Result<(), ExitCode> {
    let names = api.list_command_names(prefix).map_err(die)?;

    for name in names {
        let full = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };
        let cmd = api.get_command(&full).map_err(die)?;
        let indent = "  ".repeat(depth);
        let display = path_to_words(&full);
        let method = cmd.method().unwrap_or_else(|| "-".to_string());
        let desc = cmd
            .description()
            .unwrap_or_else(|| "no description".to_string());
        println!("{}{} {}: {}", indent, method, display, desc);
        if cmd.is_branch() {
            walk_command_tree(api, &full, depth + 1)?;
        }
    }
    Ok(())
}

fn print_api_help(api: &Api) -> Result<(), ExitCode> {
    println!("{} v{}: {}", api.name(), api.version(), api.description());
    println!("\nUsage:");
    println!("  ycallr {} <command...> [options]", api.name());
    println!(
        "  ycallr {} --list              Direct subcommands",
        api.name()
    );
    println!(
        "  ycallr {} --tree              Full command tree",
        api.name()
    );
    println!(
        "\nExample: ycallr {} create issue --owner=rust-lang --repo=rust --title=Bug",
        api.name()
    );
    println!("\nTop-level commands:");
    print_direct_children(api, "")?;
    Ok(())
}

fn print_env_section(api: &Api) -> Result<(), ExitCode> {
    let envs = api.env_vars().map_err(die)?;
    if envs.is_empty() {
        println!(
            "\nEnvironment: (none declared — auth tokens usually come from env; see profile YAML)"
        );
        return Ok(());
    }
    println!("\nEnvironment (secrets — never commit to YAML):");
    println!("  1. File:     export GITHUB_TOKEN_FILE=~/.config/ycallr/token");
    println!("  2. Env file: ycallr --env-file ~/.config/ycallr/secrets.env ...");
    println!("  3. Prompt:   run interactively (hidden input) when nothing else is set");
    println!("  4. Env var:  export GITHUB_TOKEN=...  (visible in process list)");
    for env in envs {
        let req = if env.required { "required" } else { "optional" };
        println!("  export {}=...  ({})", env.name, req);
    }
    Ok(())
}

fn print_body_help(cmd: &Command) {
    if !cmd.has_body() {
        return;
    }

    let kind = cmd.body_kind().unwrap_or_else(|| "configured".to_string());
    match kind.as_str() {
        "json" => {
            println!("\nBody (JSON, from profile):");
            println!("  Pass fields as --key=value. Values replace {{param}} placeholders in the profile JSON template.");
        }
        "form" => {
            println!("\nBody (HTML form, from profile):");
            println!("  Form field names/values come from the profile. Use --key=value for params that fill {{param}} placeholders.");
        }
        "raw" => {
            println!("\nBody (raw, from profile):");
            println!("  Raw payload is defined in the profile (templates/env). Pass --key=value for params referenced in the template.");
        }
        "multipart" => {
            println!("\nBody (multipart, from profile):");
            println!("  File paths and text parts are defined in the profile. Use --key=value for params that fill {{param}} in part metadata.");
        }
        _ => {
            println!("\nBody: configured in the API profile (see YAML source).");
        }
    }
}

fn print_implicit_body_hint(cmd: &Command) {
    if cmd.has_body() {
        return;
    }
    let method = cmd.method().unwrap_or_default();
    if matches!(method.as_str(), "POST" | "PUT" | "PATCH") {
        println!("\nBody (implicit JSON):");
        println!(
            "  No profile body block: non-path params are sent as a JSON object on {} requests.",
            method
        );
    }
}

fn print_command_help(api: &Api, command_path: &str) -> Result<(), ExitCode> {
    let cmd = api.get_command(command_path).map_err(die)?;

    let display = path_to_words(command_path);
    let desc = cmd
        .description()
        .unwrap_or_else(|| "no description".to_string());
    println!("{} {}: {}", api.name(), display, desc);

    if is_group_only(api, command_path, &cmd) {
        println!("\nCommand group (not callable). Subcommands:");
        print_direct_children(api, command_path)?;
        print_env_section(api)?;
        return Ok(());
    }

    if has_subcommands(api, command_path) {
        println!("\nAlso a command group. Subcommands:");
        print_direct_children(api, command_path)?;
    }

    let method = cmd.method().unwrap_or_else(|| "N/A".to_string());
    let endpoint = cmd.endpoint().unwrap_or_else(|| "N/A".to_string());
    let url = full_url(&api.base_url(), &endpoint);
    println!("\nMethod: {}", method);
    println!("URL:    {}", url);

    print_env_section(api)?;

    let path_params: Vec<String> =
        serde_json::from_str(&cmd.path_params_json()).unwrap_or_default();
    let params: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&cmd.params_json()).unwrap_or_default();

    if !path_params.is_empty() {
        println!("\nPath parameters (from endpoint):");
        for name in &path_params {
            let declared = params.get(name);
            let required = declared
                .and_then(|p| p.get("required"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let type_label = format_param_type_label(declared);
            let req_label = if required { " (required)" } else { "" };
            let description = declared
                .and_then(|p| p.get("description"))
                .and_then(|v| v.as_str())
                .unwrap_or("Path parameter");
            println!(
                "  --{:<20} {} [{}]{}",
                name, description, type_label, req_label
            );
        }
    }

    if !params.is_empty() {
        println!("\nParameters:");
        for (name, param) in &params {
            if path_params.contains(name) {
                continue;
            }
            let required = param
                .get("required")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let req_label = if required { " (required)" } else { "" };
            let description = param
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            println!(
                "  --{:<20} {} [{}]{}",
                name,
                description,
                format_param_type_label(Some(param)),
                req_label
            );
        }
    }

    if cmd.has_body() {
        print_body_help(&cmd);
    } else {
        print_implicit_body_hint(&cmd);
    }
    Ok(())
}

fn format_param_type_label(param: Option<&serde_json::Value>) -> String {
    let base = match param.and_then(|p| p.get("type")).and_then(|t| t.as_str()) {
        Some("number") => "number".to_string(),
        Some("boolean") => "boolean".to_string(),
        Some("array") => "array".to_string(),
        _ => "string".to_string(),
    };
    if let Some(values) = param
        .and_then(|p| p.get("enum"))
        .and_then(|v| v.as_array())
        .filter(|arr| !arr.is_empty())
    {
        let rendered = values
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_number().map(|n| n.to_string()))
            })
            .collect::<Vec<_>>()
            .join(", ");
        if !rendered.is_empty() {
            return format!("{base} ({rendered})");
        }
    }
    base
}

fn prompt_value(description: &str, param_type: Option<&serde_json::Value>) -> String {
    let label = format_param_type_label(param_type);
    let type_hint = if label == "string" {
        String::new()
    } else {
        format!(" ({label})")
    };
    if std::io::stdin().is_terminal() {
        eprint!("{}{}: ", description, type_hint);
    }
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .expect("Failed to read input");
    input.trim().to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputMode {
    Default,
    Json,
    Pretty,
}

#[derive(Clone, Copy, Debug, Default)]
struct RunFlags {
    show_status: bool,
    fail_on_error: bool,
    fail_on_warn: bool,
}

fn is_global_flag(arg: &str) -> bool {
    matches!(arg, "--verbose" | "-v")
}

fn is_discovery_flag(arg: &str) -> bool {
    matches!(arg, "--help" | "-h" | "--list" | "--tree")
}

fn is_run_flag(arg: &str) -> bool {
    matches!(
        arg,
        "--json" | "--pretty" | "--status" | "--fail-on-error" | "--fail-on-warn"
    )
}

/// Split argv after the API name. Flags can appear anywhere (e.g. `github --tree create`).
fn split_command_and_flags(args: &[String]) -> (Vec<String>, Vec<String>) {
    let mut command_words = Vec::new();
    let mut flags = Vec::new();
    for arg in args {
        if is_discovery_flag(arg)
            || is_run_flag(arg)
            || is_global_flag(arg)
            || arg.starts_with("--")
        {
            flags.push(arg.clone());
        } else {
            command_words.push(arg.clone());
        }
    }
    (command_words, flags)
}

fn resolve_command_path(api: &Api, words: &[String]) -> Result<(String, usize), FfiError> {
    if words.is_empty() {
        return Err(FfiError::new(format!(
            "missing command for API '{}'. Try: ycallr {} --list",
            api.name(),
            api.name()
        )));
    }

    let mut matched_path = None;
    let mut matched_len = 0;
    for i in 1..=words.len() {
        if words[i - 1].starts_with("--") {
            continue;
        }
        let path = words[..i].join(".");
        if api.get_command(&path).is_ok() {
            matched_path = Some(path);
            matched_len = i;
        }
    }

    if let Some(path) = matched_path {
        return Ok((path, matched_len));
    }

    let tried = words.join(" ");
    let suggestions = suggest_paths(api, &tried.replace(' ', "."));
    let hint = if suggestions.is_empty() {
        format!("Try: ycallr {} --list", api.name())
    } else {
        format!(
            "Did you mean: {}",
            suggestions
                .iter()
                .map(|p| path_to_words(p))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    Err(FfiError::new(format!(
        "unknown command '{}' for API '{}'. {}",
        tried,
        api.name(),
        hint
    )))
}

fn parse_run_flags_and_params(
    flag_args: &[String],
    api: &Api,
    command_path: &str,
    cmd: &Command,
) -> Result<(String, OutputMode, RunFlags), ExitCode> {
    let mut params = HashMap::new();
    let mut output = OutputMode::Default;
    let mut run_flags = RunFlags::default();

    for arg in flag_args {
        match arg.as_str() {
            "--help" | "-h" | "--list" | "--tree" => {}
            "--pretty" => output = OutputMode::Pretty,
            "--json" => {
                if output != OutputMode::Pretty {
                    output = OutputMode::Json;
                }
            }
            "--status" => run_flags.show_status = true,
            "--fail-on-error" => run_flags.fail_on_error = true,
            "--fail-on-warn" => run_flags.fail_on_warn = true,
            "--verbose" | "-v" => {}
            _ if let Some((key, value)) = arg.split_once('=') => {
                if let Some(name) = key.strip_prefix("--") {
                    params.insert(name.to_string(), value.to_string());
                } else {
                    return Err(die(FfiError::new(format!(
                        "invalid parameter '{}'. Use --key=value",
                        arg
                    ))));
                }
            }
            _ if arg.starts_with("--") => {
                return Err(die(FfiError::new(format!(
                    "unknown option '{}'. Use --key=value for parameters",
                    arg
                ))));
            }
            _ => {
                return Err(die(FfiError::new(format!(
                    "unexpected argument '{}'. Parameters use --key=value",
                    arg
                ))));
            }
        }
    }

    let params_json = serde_json::to_string(&params).unwrap();
    let missing = api
        .missing_params(command_path, &params_json)
        .map_err(die)?;

    if !missing.is_empty() {
        if std::io::stdin().is_terminal() {
            eprintln!("Missing required parameters. Enter values:\n");

            let declared: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&cmd.params_json()).unwrap_or_default();

            for name in missing {
                let param = declared.get(&name);
                let description = param
                    .and_then(|p| p.get("description"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Path parameter");
                let value = prompt_value(description, param);
                if value.is_empty() {
                    return Err(die(FfiError::new(format!(
                        "required parameter '{}' cannot be empty",
                        name
                    ))));
                }
                params.insert(name, value);
            }
        } else {
            return Err(die(FfiError::new(format!(
                "missing required parameters: {}. Pass values with --key=value",
                missing.join(", ")
            ))));
        }
    }

    Ok((
        serde_json::to_string(&params).unwrap_or_else(|_| "{}".to_string()),
        output,
        run_flags,
    ))
}

fn print_response(body: &str, message: Option<String>, mode: OutputMode) {
    match mode {
        OutputMode::Default => {
            if let Some(msg) = message {
                println!("{}", msg);
            }
        }
        OutputMode::Json => {
            println!("{}", body);
        }
        OutputMode::Pretty => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| body.to_string())
                );
            } else {
                println!("{}", body);
            }
        }
    }
}

fn http_exit_code(status: u16, flags: RunFlags) -> ExitCode {
    if flags.fail_on_warn && status >= 300 {
        return ExitCode::from(EXIT_HTTP);
    }
    if flags.fail_on_error && status >= 400 {
        return ExitCode::from(EXIT_HTTP);
    }
    ExitCode::from(EXIT_OK)
}

fn print_install_help() {
    println!("ycallr install: compile a YAML API profile to protobuf\n");
    println!("Usage:");
    println!("  ycallr install <path>             Path to a .yaml/.yml file (or stem without extension)\n");
    println!("Examples:");
    println!("  ycallr install ~/apis/github.yaml");
    println!("  ycallr install ./examples/github_api.yaml");
    println!("  ycallr install ~/.config/ycallr/apis/github\n");
    println!("Compiled profiles are written to:");
    println!("  <config>/ycallr/apis/<name>.pb  (default: ~/.config/ycallr/apis on Linux/macOS)");
    println!("  Override with YCALLR_CONFIG_DIR=/path/to/apis");
    println!("  (profile name comes from the YAML `name:` field)");
}

fn print_import_openapi_help() {
    println!("ycallr import-openapi: generate a ycallr YAML profile from OpenAPI 3.x\n");
    println!("Usage:");
    println!("  ycallr import-openapi <openapi-path> [options]\n");
    println!("Options:");
    println!("  --name <name>     Profile name (default: slug from info.title)");
    println!("  --out <path>      Output YAML path (default: next to source)");
    println!("  --tag <tag>       Import only operations with this OpenAPI tag");
    println!("  --base-url <url>  Override base_url (for GHES templates)");
    println!("  --nest-by <mode>  Group commands by path (default) or tag");
    println!("  --short-names     Shorten leaf command names (on by default with --nest-by tag)");
    println!("  --preset <name>   Vendor preset: auto (default), github, none\n");
    println!("With --nest-by tag: issues.create instead of repos.issues.create-issue");
    println!("After import, edit the YAML (responses, headers, env) then run:");
    println!("  ycallr install <generated.yaml>\n");
    println!("Examples:");
    println!("  ycallr import-openapi ./github.openapi.json --name github");
    println!(
        "  ycallr import-openapi ./spec.yaml --tag issues --nest-by tag --out ./github-issues.yaml"
    );
}

fn import_openapi_command(args: &[String]) -> Result<(), ExitCode> {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print_import_openapi_help();
        return Ok(());
    }

    let source = &args[0];
    let mut name = None;
    let mut output = None;
    let mut tag = None;
    let mut base_url = None;
    let mut nest_by = None;
    let mut short_names = false;
    let mut preset = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--name" => {
                i += 1;
                if i >= args.len() {
                    return Err(die(FfiError::new("--name requires a value".to_string())));
                }
                name = Some(args[i].clone());
            }
            "--out" => {
                i += 1;
                if i >= args.len() {
                    return Err(die(FfiError::new("--out requires a path".to_string())));
                }
                output = Some(args[i].clone());
            }
            "--tag" => {
                i += 1;
                if i >= args.len() {
                    return Err(die(FfiError::new("--tag requires a value".to_string())));
                }
                tag = Some(args[i].clone());
            }
            "--base-url" => {
                i += 1;
                if i >= args.len() {
                    return Err(die(FfiError::new("--base-url requires a URL".to_string())));
                }
                base_url = Some(args[i].clone());
            }
            "--nest-by" => {
                i += 1;
                if i >= args.len() {
                    return Err(die(FfiError::new(
                        "--nest-by requires path or tag".to_string(),
                    )));
                }
                nest_by = Some(args[i].clone());
            }
            "--short-names" => {
                short_names = true;
            }
            "--preset" => {
                i += 1;
                if i >= args.len() {
                    return Err(die(FfiError::new(
                        "--preset requires auto, github, or none".to_string(),
                    )));
                }
                preset = Some(args[i].clone());
            }
            other => {
                return Err(die(FfiError::new(format!(
                    "unknown option '{}'. Run: ycallr import-openapi --help",
                    other
                ))));
            }
        }
        i += 1;
    }

    let (profile_name, yaml_path) = ffi_api::import_openapi_file(
        source,
        output.as_deref(),
        name.as_deref(),
        tag.as_deref(),
        base_url.as_deref(),
        nest_by.as_deref(),
        short_names,
        preset.as_deref(),
    )
    .map_err(die)?;

    println!("Imported OpenAPI -> {}", yaml_path);
    println!("Profile name: {}", profile_name);
    println!();
    println!("Next steps:");
    println!("  1. Review/edit the YAML (responses, env, headers)");
    println!("  2. ycallr install {}", yaml_path);
    Ok(())
}

fn print_help() {
    println!("ycallr: fast API runner (profiles compiled to protobuf)\n");
    println!("Usage:");
    println!("  ycallr --list                     List installed API profiles");
    println!("  ycallr --version                  Show CLI and core version");
    println!("  ycallr install <path>             Compile and install a YAML profile");
    println!("  ycallr import-openapi <path>      Generate YAML from OpenAPI 3.x");
    println!("  ycallr secret set <NAME>          Store a secret (keyring or file)");
    println!("  ycallr secret list                List stored secret names");
    println!("  ycallr secret unset <NAME>        Remove a stored secret");
    println!("  ycallr <api> <command...> [opts]  Run a command (space-separated path)\n");
    println!("Discovery:");
    println!("  ycallr <api> --list               Direct subcommands");
    println!("  ycallr <api> --tree               Full command tree");
    println!("  ycallr <api> <cmd> --help         Command details\n");
    println!("Output:");
    println!("  (default)   Response message template");
    println!("  --json      Compact JSON body");
    println!("  --pretty    Pretty-printed JSON body\n");
    println!("Automation:");
    println!("  --status          Print HTTP status to stderr");
    println!("  --fail-on-error   Exit 2 on HTTP status >= 400");
    println!("  --fail-on-warn    Exit 2 on HTTP status >= 300\n");
    println!("Logging:");
    println!("  --verbose, -v     Debug logs to stderr (ycallr + ycallr-core)");
    println!("  RUST_LOG=...      Standard tracing filter (overrides --verbose)\n");
    println!("Configuration:");
    println!(
        "  YCALLR_CONFIG_DIR   Directory for installed .pb profiles (default: platform config dir)"
    );
    println!("  YCALLR_SECRETS_DIR  Directory for built-in secret file fallback");
    println!("  --env-file <path>   Load secrets from KEY=VALUE file (chmod 600 recommended)\n");
    println!("Examples:");
    println!("  ycallr install ~/.config/ycallr/apis/github.yaml");
    println!("  ycallr --list");
    println!("  ycallr github create issue --owner=rust-lang --repo=rust --title=Bug");
    println!("  ycallr github create issue --json --status --fail-on-error");
}

fn load_api(name: &str) -> Result<Api, ExitCode> {
    Api::load_installed(name).map_err(|e| die_code(e, EXIT_USAGE))
}

pub fn run() -> ExitCode {
    run_with_args(std::env::args().skip(1).collect())
}

pub fn run_with_args(raw_args: Vec<String>) -> ExitCode {
    logging::init_from_args(&raw_args);
    match run_with_args_inner(raw_args) {
        Ok(code) => code,
        Err(code) => code,
    }
}

fn run_with_args_inner(raw_args: Vec<String>) -> Result<ExitCode, ExitCode> {
    let (args, global_opts) = env_secrets::extract_global_options(&raw_args);
    env_secrets::warn_insecure_env_files(&global_opts.env_files);
    let file_envs = env_secrets::load_env_files(&global_opts.env_files).map_err(die)?;

    if args.is_empty() {
        print_help();
        return Ok(ExitCode::from(EXIT_OK));
    }

    if args.len() == 1 {
        match args[0].as_str() {
            "--help" | "-h" => {
                print_help();
                return Ok(ExitCode::from(EXIT_OK));
            }
            "--version" | "-V" => {
                print_version();
                return Ok(ExitCode::from(EXIT_OK));
            }
            "--list" => {
                print_installed_apis()?;
                return Ok(ExitCode::from(EXIT_OK));
            }
            _ => {}
        }
    }

    if args[0] == "secret" {
        secrets::warn_insecure_secret_files();
        secret_cmd::run_secret_command(&args[1..]).map_err(die)?;
        return Ok(ExitCode::from(EXIT_OK));
    }

    if args[0] == "install" {
        if args.len() == 2 && (args[1] == "--help" || args[1] == "-h") {
            print_install_help();
            return Ok(ExitCode::from(EXIT_OK));
        }
        if args.len() != 2 {
            return Err(die(FfiError::new(
                "usage: ycallr install <path/to/profile.yaml>".to_string(),
            )));
        }
        install_api(&args[1])?;
        return Ok(ExitCode::from(EXIT_OK));
    }

    if args[0] == "import-openapi" {
        import_openapi_command(&args[1..])?;
        return Ok(ExitCode::from(EXIT_OK));
    }

    let api_name = &args[0];
    tracing::debug!(api = %api_name, "loading installed API profile");
    let api = load_api(api_name)?;
    let rest = &args[1..];

    let (command_words, flag_args) = split_command_and_flags(rest);
    let show_help = flag_args.iter().any(|a| a == "--help" || a == "-h");
    let show_list = flag_args.iter().any(|a| a == "--list");
    let show_tree = flag_args.iter().any(|a| a == "--tree");

    if command_words.is_empty() {
        if show_tree {
            println!("{}: command tree", api.name());
            walk_command_tree(&api, "", 0)?;
            return Ok(ExitCode::from(EXIT_OK));
        }
        if show_list {
            print_direct_children(&api, "")?;
            return Ok(ExitCode::from(EXIT_OK));
        }
        if show_help {
            print_api_help(&api)?;
            return Ok(ExitCode::from(EXIT_OK));
        }
        print_api_help(&api)?;
        return Ok(ExitCode::from(EXIT_OK));
    }

    let (command_path, consumed) = resolve_command_path(&api, &command_words).map_err(die)?;

    if consumed < command_words.len() {
        let extra = command_words[consumed..].join(" ");
        let full_attempt = command_words.join(".");
        let suggestions = suggest_paths(&api, &full_attempt);
        let sub_hint = format_subcommand_hint(&api, &command_path);
        let hint = if !suggestions.is_empty() {
            format!(
                "Did you mean: {}",
                suggestions
                    .iter()
                    .map(|p| path_to_words(p))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else if !sub_hint.is_empty() {
            sub_hint
        } else {
            format!("Try: ycallr {} --list", api.name())
        };
        return Err(die(FfiError::new(format!(
            "unexpected extra path segment(s) '{}' after '{}'. {}",
            extra,
            path_to_words(&command_path),
            hint
        ))));
    }

    if show_tree {
        println!(
            "{}: command tree under '{}'",
            api.name(),
            path_to_words(&command_path)
        );
        walk_command_tree(&api, &command_path, 0)?;
        return Ok(ExitCode::from(EXIT_OK));
    }

    if show_list {
        print_direct_children(&api, &command_path)?;
        return Ok(ExitCode::from(EXIT_OK));
    }

    if show_help {
        print_command_help(&api, &command_path)?;
        return Ok(ExitCode::from(EXIT_OK));
    }

    let cmd = api.get_command(&command_path).map_err(die)?;

    if !cmd.is_leaf() {
        let hint = format_subcommand_hint(&api, &command_path);
        let details = if hint.is_empty() {
            "Use --list for subcommands or --help for details.".to_string()
        } else {
            format!("{}. Use --help for details.", hint)
        };
        return Err(die(FfiError::new(format!(
            "'{}' is a command group, not callable. {}",
            path_to_words(&command_path),
            details
        ))));
    }

    let (params_json, output_mode, run_flags) =
        parse_run_flags_and_params(&flag_args, &api, &command_path, &cmd)?;
    let body_json = api.build_implicit_body(&command_path, &params_json);

    let env_overrides = env_secrets::build_env_overrides(&api, &file_envs).map_err(die)?;
    let client = api.create_client_with_envs(&env_overrides).map_err(die)?;
    tracing::debug!(
        api = %api.name(),
        command = %command_path,
        "calling API command"
    );
    let response = client
        .call(&command_path, &params_json, body_json.as_deref())
        .map_err(|e| {
            die_code(
                FfiError::new(format!(
                    "request failed for '{}': {}",
                    path_to_words(&command_path),
                    e
                )),
                EXIT_USAGE,
            )
        })?;

    let status = response.status();
    if run_flags.show_status {
        eprintln!("{}", status);
    }

    print_response(&response.body_json(), response.message(), output_mode);
    Ok(http_exit_code(status, run_flags))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn levenshtein_distance() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("", "abc"), 3);
    }

    #[test]
    fn path_to_words_replaces_dots() {
        assert_eq!(path_to_words("repos.issues.create"), "repos issues create");
    }

    #[test]
    fn full_url_joins_base_and_endpoint() {
        assert_eq!(
            full_url("https://api.example.com", "/v1/ping"),
            "https://api.example.com/v1/ping"
        );
        assert_eq!(
            full_url("https://api.example.com/", "/v1/ping"),
            "https://api.example.com/v1/ping"
        );
        assert_eq!(
            full_url("https://api.example.com", "v1/ping"),
            "https://api.example.com/v1/ping"
        );
    }

    #[test]
    fn looks_like_install_path_detects_paths() {
        assert!(looks_like_install_path("~/apis/github.yaml"));
        assert!(looks_like_install_path("./profile.yaml"));
        assert!(looks_like_install_path("/tmp/profile.yml"));
        assert!(!looks_like_install_path("github"));
    }

    #[test]
    fn split_command_and_flags_separates_segments() {
        let args = vec![
            "create".into(),
            "issue".into(),
            "--json".into(),
            "--owner=rust-lang".into(),
        ];
        let (cmd, flags) = split_command_and_flags(&args);
        assert_eq!(cmd, vec!["create", "issue"]);
        assert_eq!(flags, vec!["--json", "--owner=rust-lang"]);
    }

    #[test]
    fn discovery_and_run_flags() {
        assert!(is_discovery_flag("--help"));
        assert!(is_discovery_flag("--tree"));
        assert!(!is_discovery_flag("--json"));
        assert!(is_run_flag("--fail-on-error"));
        assert!(!is_run_flag("--list"));
        assert!(is_global_flag("--verbose"));
        assert!(is_global_flag("-v"));
    }

    #[test]
    fn http_exit_code_respects_flags() {
        let fail_error = RunFlags {
            show_status: false,
            fail_on_error: true,
            fail_on_warn: false,
        };
        assert_eq!(http_exit_code(404, fail_error), ExitCode::from(EXIT_HTTP));
        assert_eq!(http_exit_code(200, fail_error), ExitCode::from(EXIT_OK));

        let fail_warn = RunFlags {
            show_status: false,
            fail_on_error: false,
            fail_on_warn: true,
        };
        assert_eq!(http_exit_code(301, fail_warn), ExitCode::from(EXIT_HTTP));
    }

    #[test]
    fn format_param_type_label_includes_enum() {
        let param = serde_json::json!({
            "type": "string",
            "enum": ["open", "closed"]
        });
        assert_eq!(
            format_param_type_label(Some(&param)),
            "string (open, closed)"
        );
    }

    #[test]
    fn print_response_modes() {
        print_response("{}", Some("ok".into()), OutputMode::Default);
        print_response(r#"{"a":1}"#, None, OutputMode::Json);
        print_response(r#"{"a":1}"#, None, OutputMode::Pretty);
        print_response("not-json", None, OutputMode::Pretty);
    }

    #[test]
    fn run_with_args_help() {
        assert_eq!(
            run_with_args(vec!["--help".into()]),
            ExitCode::from(EXIT_OK)
        );
    }

    fn test_fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    fn with_isolated_config(test: impl FnOnce(PathBuf, PathBuf)) {
        use std::sync::Mutex;
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let apis = home.path().join("apis");
        let secrets = home.path().join("secrets");
        std::fs::create_dir_all(&apis).unwrap();
        std::env::set_var("HOME", home.path());
        std::env::set_var("USERPROFILE", home.path());
        std::env::set_var("YCALLR_CONFIG_DIR", &apis);
        std::env::set_var("YCALLR_SECRETS_DIR", &secrets);
        test(apis, secrets);
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");
        std::env::remove_var("YCALLR_CONFIG_DIR");
        std::env::remove_var("YCALLR_SECRETS_DIR");
    }

    #[test]
    fn rich_api_tree_list_and_help() {
        with_isolated_config(|_apis, _secrets| {
            let yaml = test_fixture("rich_api.yaml");
            assert_eq!(
                run_with_args(vec!["install".into(), yaml.to_str().unwrap().into(),]),
                ExitCode::from(EXIT_OK)
            );
            assert_eq!(
                run_with_args(vec!["richapi".into(), "--tree".into()]),
                ExitCode::from(EXIT_OK)
            );
            assert_eq!(
                run_with_args(vec!["richapi".into(), "--list".into()]),
                ExitCode::from(EXIT_OK)
            );
            assert_eq!(
                run_with_args(vec!["richapi".into(), "repos".into(), "--list".into()]),
                ExitCode::from(EXIT_OK)
            );
            assert_eq!(
                run_with_args(vec![
                    "richapi".into(),
                    "repos".into(),
                    "issues".into(),
                    "--help".into(),
                ]),
                ExitCode::from(EXIT_OK)
            );
            assert_eq!(
                run_with_args(vec![
                    "richapi".into(),
                    "create-item".into(),
                    "--help".into()
                ]),
                ExitCode::from(EXIT_OK)
            );
            assert_eq!(
                run_with_args(vec!["richapi".into(), "repos".into(), "--help".into()]),
                ExitCode::from(EXIT_OK)
            );
        });
    }

    #[test]
    fn rich_api_missing_params_non_interactive() {
        with_isolated_config(|_apis, _secrets| {
            let yaml = test_fixture("rich_api.yaml");
            run_with_args(vec!["install".into(), yaml.to_str().unwrap().into()]);
            assert_eq!(
                run_with_args(vec![
                    "richapi".into(),
                    "repos".into(),
                    "issues".into(),
                    "open".into(),
                ]),
                ExitCode::from(EXIT_USAGE)
            );
        });
    }

    #[test]
    fn rich_api_call_with_env_file() {
        with_isolated_config(|apis, _secrets| {
            let mut server = mockito::Server::new();
            let mock = server
                .mock("GET", "/repos")
                .with_status(200)
                .with_body(r#"{"repos":[]}"#)
                .create();

            let rich_yaml =
                std::fs::read_to_string(test_fixture("rich_api.yaml")).expect("read rich_api");
            let rich_yaml = rich_yaml.replace("https://example.com", &server.url());
            let yaml = apis.join("rich_mock.yaml");
            std::fs::write(&yaml, rich_yaml).unwrap();

            let env_file = apis.join("test.env");
            std::fs::write(&env_file, "API_KEY=test-key\nOPTIONAL_FLAG=yes\n").unwrap();
            run_with_args(vec!["install".into(), yaml.to_str().unwrap().into()]);
            assert_eq!(
                run_with_args(vec![
                    "--env-file".into(),
                    env_file.to_str().unwrap().into(),
                    "richapi".into(),
                    "repos".into(),
                    "list".into(),
                ]),
                ExitCode::from(EXIT_OK)
            );
            mock.assert();
        });
    }

    #[test]
    fn body_kinds_help_covers_body_text() {
        with_isolated_config(|_apis, _secrets| {
            let yaml = test_fixture("body_kinds_api.yaml");
            run_with_args(vec!["install".into(), yaml.to_str().unwrap().into()]);
            for cmd in ["json-cmd", "form-cmd", "raw-cmd", "multipart-cmd"] {
                assert_eq!(
                    run_with_args(vec!["bodyapi".into(), cmd.into(), "--help".into()]),
                    ExitCode::from(EXIT_OK)
                );
            }
        });
    }

    #[test]
    fn parse_run_flags_rejects_invalid_tokens() {
        with_isolated_config(|_apis, _secrets| {
            let yaml = test_fixture("minimal_api.yaml");
            run_with_args(vec!["install".into(), yaml.to_str().unwrap().into()]);
            let api = ffi_api::Api::load_installed("testapi").unwrap();
            let cmd = api.get_command("ping").unwrap();
            assert!(parse_run_flags_and_params(&["=bad".into()], &api, "ping", &cmd).is_err());
            assert!(parse_run_flags_and_params(&["--unknown".into()], &api, "ping", &cmd).is_err());
            assert!(parse_run_flags_and_params(&["bare".into()], &api, "ping", &cmd).is_err());
        });
    }

    #[test]
    fn suggest_paths_finds_close_match() {
        with_isolated_config(|_apis, _secrets| {
            let yaml = test_fixture("rich_api.yaml");
            run_with_args(vec!["install".into(), yaml.to_str().unwrap().into()]);
            let api = ffi_api::Api::load_installed("richapi").unwrap();
            let suggestions = suggest_paths(&api, "repos.issu");
            assert!(!suggestions.is_empty());
            let hint = format_subcommand_hint(&api, "repos");
            assert!(hint.contains("Subcommands"));
        });
    }

    #[test]
    fn import_openapi_all_options() {
        with_isolated_config(|apis, _secrets| {
            let spec = test_fixture("minimal_openapi.json");
            let out = apis.join("out.yaml");
            assert_eq!(
                run_with_args(vec![
                    "import-openapi".into(),
                    spec.to_str().unwrap().into(),
                    "--name".into(),
                    "optapi".into(),
                    "--out".into(),
                    out.to_str().unwrap().into(),
                    "--tag".into(),
                    "default".into(),
                    "--base-url".into(),
                    "https://api.example.com".into(),
                    "--nest-by".into(),
                    "tag".into(),
                    "--short-names".into(),
                    "--preset".into(),
                    "none".into(),
                ]),
                ExitCode::from(EXIT_OK)
            );
        });
    }

    #[test]
    fn group_command_not_callable_returns_usage() {
        with_isolated_config(|_apis, _secrets| {
            let yaml = test_fixture("rich_api.yaml");
            run_with_args(vec!["install".into(), yaml.to_str().unwrap().into()]);
            assert_eq!(
                run_with_args(vec!["richapi".into(), "repos".into(), "issues".into()]),
                ExitCode::from(EXIT_USAGE)
            );
        });
    }

    #[test]
    fn full_url_handles_empty_endpoint() {
        assert_eq!(
            full_url("https://api.example.com/", ""),
            "https://api.example.com"
        );
    }

    #[test]
    fn format_param_type_label_number_and_boolean() {
        assert_eq!(
            format_param_type_label(Some(&serde_json::json!({"type": "number"}))),
            "number"
        );
        assert_eq!(
            format_param_type_label(Some(&serde_json::json!({"type": "boolean"}))),
            "boolean"
        );
        assert_eq!(
            format_param_type_label(Some(&serde_json::json!({"type": "array"}))),
            "array"
        );
    }
}
