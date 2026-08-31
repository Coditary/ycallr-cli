mod ffi_api;

use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;

use ffi_api::{Api, Command, FfiError};

const EXIT_OK: u8 = 0;
const EXIT_USAGE: u8 = 1;
const EXIT_HTTP: u8 = 2;

fn die_code(err: FfiError, code: u8) -> ! {
    eprintln!("error: {}", err);
    std::process::exit(code as i32);
}

fn die(err: FfiError) -> ! {
    die_code(err, EXIT_USAGE);
}

fn path_to_words(path: &str) -> String {
    path.replace('.', " ")
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
        || source.contains('/') || source.contains('\\')
        || source.ends_with(".yaml")
        || source.ends_with(".yml")
        || Path::new(source).is_absolute()
}

fn install_api(source: &str) {
    if !looks_like_install_path(source) {
        die(FfiError::new(format!(
            "Install requires a path to a YAML file.\n\
             Example: ycallr install ~/.config/ycallr/apis/github.yaml\n\
             Got '{}' — pass the file path, not just the profile name.",
            source
        )));
    }
    let (name, pb_path) = ffi_api::install_profile_file(source).unwrap_or_else(|e| die(e));
    println!("Installed '{}' -> {}", name, pb_path);
}

fn print_installed_apis() {
    let apis = ffi_api::list_installed().unwrap_or_else(|e| die(e));
    if apis.is_empty() {
        println!("No installed API profiles (.pb). Run: ycallr install <path/to/profile.yaml>");
        return;
    }
    for (name, desc) in apis {
        println!("  {:<25} {}", name, desc);
    }
}

fn print_version() {
    println!("ycallr {} (ffi-cli)", env!("CARGO_PKG_VERSION"));
    println!("ycallr-core {}", ycallr_core::VERSION);
}

fn collect_all_paths(api: &Api, prefix: &str, out: &mut Vec<String>) {
    let names = api
        .list_command_names(prefix)
        .unwrap_or_else(|e| die(e));
    for name in names {
        let full = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };
        out.push(full.clone());
        if api.get_command(&full).map(|c| c.is_branch()).unwrap_or(false) {
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

fn print_direct_children(api: &Api, prefix: &str) {
    let names = api
        .list_command_names(prefix)
        .unwrap_or_else(|e| die(e));

    if names.is_empty() {
        println!("(no subcommands)");
        return;
    }

    for name in names {
        let full = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };
        let cmd = api.get_command(&full).unwrap_or_else(|e| die(e));
        let display = path_to_words(&full);
        let method = cmd
            .method()
            .map(|m| format!("{:<7}", m))
            .unwrap_or_else(|| "       ".to_string());
        let desc = cmd.description().unwrap_or_else(|| "no description".to_string());
        let kind = if cmd.is_branch() && cmd.is_leaf() {
            " (callable + group)"
        } else if cmd.is_branch() {
            " (group)"
        } else {
            ""
        };
        println!("  {} {:<32} {}{}", method, display, desc, kind);
    }
}

fn walk_command_tree(api: &Api, prefix: &str, depth: usize) {
    let names = api
        .list_command_names(prefix)
        .unwrap_or_else(|e| die(e));

    for name in names {
        let full = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", prefix, name)
        };
        let cmd = api.get_command(&full).unwrap_or_else(|e| die(e));
        let indent = "  ".repeat(depth);
        let display = path_to_words(&full);
        let method = cmd.method().unwrap_or_else(|| "—".to_string());
        let desc = cmd.description().unwrap_or_else(|| "no description".to_string());
        println!("{}{} {} — {}", indent, method, display, desc);
        if cmd.is_branch() {
            walk_command_tree(api, &full, depth + 1);
        }
    }
}

fn print_api_help(api: &Api) {
    println!("{} v{} — {}", api.name(), api.version(), api.description());
    println!("\nUsage:");
    println!(
        "  ycallr {} <command...> [options]",
        api.name()
    );
    println!("  ycallr {} --list              Direct subcommands", api.name());
    println!("  ycallr {} --tree              Full command tree", api.name());
    println!(
        "\nExample: ycallr {} create issue --owner=rust-lang --repo=rust --title=Bug",
        api.name()
    );
    println!("\nTop-level commands:");
    print_direct_children(api, "");
}

fn print_env_section(api: &Api) {
    let envs = api.env_vars().unwrap_or_else(|e| die(e));
    if envs.is_empty() {
        println!("\nEnvironment: (none declared in profile — check auth/base_url for ${{VAR}})");
        return;
    }
    println!("\nEnvironment (read from OS env, Auto mode):");
    for env in envs {
        let req = if env.required { "required" } else { "optional" };
        println!("  {} ({})", env.name, req);
    }
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
            "  No profile body block — non-path params are sent as a JSON object on {} requests.",
            method
        );
    }
}

fn print_command_help(api: &Api, command_path: &str) {
    let cmd = api.get_command(command_path).unwrap_or_else(|e| die(e));

    let display = path_to_words(command_path);
    let desc = cmd.description().unwrap_or_else(|| "no description".to_string());
    println!("{} {} — {}", api.name(), display, desc);

    let method = cmd.method().unwrap_or_else(|| "N/A".to_string());
    let endpoint = cmd.endpoint().unwrap_or_else(|| "N/A".to_string());
    let url = full_url(&api.base_url(), &endpoint);
    println!("\nMethod: {}", method);
    println!("URL:    {}", url);

    print_env_section(api);

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
            let type_label = param_type_label(declared);
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
                param_type_label(Some(param)),
                req_label
            );
        }
    }

    if cmd.has_body() {
        print_body_help(&cmd);
    } else {
        print_implicit_body_hint(&cmd);
    }
}

fn param_type_label(param: Option<&serde_json::Value>) -> &'static str {
    match param.and_then(|p| p.get("type")).and_then(|t| t.as_str()) {
        Some("number") => "number",
        Some("boolean") => "boolean",
        Some("array") => "array",
        _ => "string",
    }
}

fn prompt_value(description: &str, param_type: Option<&serde_json::Value>) -> String {
    let type_hint = match param_type.and_then(|p| p.get("type")).and_then(|t| t.as_str()) {
        Some("number") => " (number)",
        Some("boolean") => " (true/false)",
        _ => "",
    };
    if std::io::stdin().is_terminal() {
        eprint!("{}{}: ", description, type_hint);
    }
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).expect("Failed to read input");
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
        if is_discovery_flag(arg) || is_run_flag(arg) || arg.starts_with("--") {
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
    for i in (1..=words.len()).rev() {
        let path = words[..i].join(".");
        if words[i - 1].starts_with("--") {
            continue;
        }
        if api.get_command(&path).is_ok() {
            return Ok((path, i));
        }
    }
    let tried = words.join(" ");
    let suggestions = suggest_paths(api, &tried);
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
        tried, api.name(), hint
    )))
}

fn parse_run_flags_and_params(
    flag_args: &[String],
    api: &Api,
    command_path: &str,
    cmd: &Command,
) -> (String, OutputMode, RunFlags) {
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
            _ if let Some((key, value)) = arg.split_once('=') => {
                if key.starts_with("--") {
                    params.insert(key[2..].to_string(), value.to_string());
                } else {
                    die(FfiError::new(format!(
                        "invalid parameter '{}'. Use --key=value",
                        arg
                    )));
                }
            }
            _ if arg.starts_with("--") => {
                die(FfiError::new(format!(
                    "unknown option '{}'. Use --key=value for parameters",
                    arg
                )));
            }
            _ => {
                die(FfiError::new(format!(
                    "unexpected argument '{}'. Parameters use --key=value",
                    arg
                )));
            }
        }
    }

    let params_json = serde_json::to_string(&params).unwrap();
    let missing = api
        .missing_params(command_path, &params_json)
        .unwrap_or_else(|e| die(e));

    if !missing.is_empty() {
        if std::io::stdin().is_terminal() {
            eprintln!("Missing required parameters. Enter values:\n");
        }

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
                die(FfiError::new(format!(
                    "required parameter '{}' cannot be empty",
                    name
                )));
            }
            params.insert(name, value);
        }
    }

    (
        serde_json::to_string(&params).unwrap_or_else(|_| "{}".to_string()),
        output,
        run_flags,
    )
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
    println!("ycallr install — compile a YAML API profile to protobuf\n");
    println!("Usage:");
    println!("  ycallr install <path>             Path to a .yaml/.yml file (or stem without extension)\n");
    println!("Examples:");
    println!("  ycallr install ~/apis/github.yaml");
    println!("  ycallr install ./examples/github_api.yaml");
    println!("  ycallr install ~/.config/ycallr/apis/github\n");
    println!("Compiled profiles are written to:");
    println!("  ~/.config/ycallr/apis/<name>.pb");
    println!("  (profile name comes from the YAML `name:` field)");
}

fn print_help() {
    println!("ycallr — fast API runner (profiles compiled to protobuf)\n");
    println!("Usage:");
    println!("  ycallr --list                     List installed API profiles");
    println!("  ycallr --version                  Show CLI and core version");
    println!("  ycallr install <path>             Compile and install a YAML profile");
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
    println!("Examples:");
    println!("  ycallr install ~/.config/ycallr/apis/github.yaml");
    println!("  ycallr --list");
    println!("  ycallr github create issue --owner=rust-lang --repo=rust --title=Bug");
    println!("  ycallr github create issue --json --status --fail-on-error");
}

fn load_api(name: &str) -> Api {
    Api::load_installed(name).unwrap_or_else(|e| die_code(e, EXIT_USAGE))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        print_help();
        return ExitCode::from(EXIT_OK);
    }

    if args.len() == 1 {
        match args[0].as_str() {
            "--help" | "-h" => {
                print_help();
                return ExitCode::from(EXIT_OK);
            }
            "--version" | "-V" => {
                print_version();
                return ExitCode::from(EXIT_OK);
            }
            "--list" => {
                print_installed_apis();
                return ExitCode::from(EXIT_OK);
            }
            _ => {}
        }
    }

    if args[0] == "install" {
        if args.len() == 2 && (args[1] == "--help" || args[1] == "-h") {
            print_install_help();
            return ExitCode::from(EXIT_OK);
        }
        if args.len() != 2 {
            die(FfiError::new(
                "usage: ycallr install <path/to/profile.yaml>".to_string(),
            ));
        }
        install_api(&args[1]);
        return ExitCode::from(EXIT_OK);
    }

    let api_name = &args[0];
    let api = load_api(api_name);
    let rest = &args[1..];

    let (command_words, flag_args) = split_command_and_flags(rest);
    let show_help = flag_args.iter().any(|a| a == "--help" || a == "-h");
    let show_list = flag_args.iter().any(|a| a == "--list");
    let show_tree = flag_args.iter().any(|a| a == "--tree");

    if command_words.is_empty() {
        if show_tree {
            println!("{} — command tree", api.name());
            walk_command_tree(&api, "", 0);
            return ExitCode::from(EXIT_OK);
        }
        if show_list {
            print_direct_children(&api, "");
            return ExitCode::from(EXIT_OK);
        }
        if show_help {
            print_api_help(&api);
            return ExitCode::from(EXIT_OK);
        }
        print_api_help(&api);
        return ExitCode::from(EXIT_OK);
    }

    let (command_path, consumed) =
        resolve_command_path(&api, &command_words).unwrap_or_else(|e| die(e));

    if consumed < command_words.len() {
        let extra = command_words[consumed..].join(" ");
        die(FfiError::new(format!(
            "unexpected extra path segment(s) '{}' after '{}'. {}",
            extra,
            path_to_words(&command_path),
            if suggest_paths(&api, &command_words.join(".")).is_empty() {
                format!("Try: ycallr {} --list", api.name())
            } else {
                format!(
                    "Did you mean: {}",
                    suggest_paths(&api, &command_words.join("."))
                        .iter()
                        .map(|p| path_to_words(p))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        )));
    }

    if show_tree {
        println!(
            "{} — command tree under '{}'",
            api.name(),
            path_to_words(&command_path)
        );
        walk_command_tree(&api, &command_path, 0);
        return ExitCode::from(EXIT_OK);
    }

    if show_list {
        print_direct_children(&api, &command_path);
        return ExitCode::from(EXIT_OK);
    }

    if show_help {
        print_command_help(&api, &command_path);
        return ExitCode::from(EXIT_OK);
    }

    let cmd = api.get_command(&command_path).unwrap_or_else(|e| die(e));

    if !cmd.is_leaf() {
        die(FfiError::new(format!(
            "'{}' is a command group, not callable. Use --list for subcommands or --help for details.",
            path_to_words(&command_path)
        )));
    }

    let (params_json, output_mode, run_flags) =
        parse_run_flags_and_params(&flag_args, &api, &command_path, &cmd);
    let body_json = api.build_implicit_body(&command_path, &params_json);

    let client = api.create_client().unwrap_or_else(|e| die(e));
    let response = client
        .call(&command_path, &params_json, body_json.as_deref())
        .unwrap_or_else(|e| {
            die_code(
                FfiError::new(format!(
                    "request failed for '{}': {}",
                    path_to_words(&command_path),
                    e
                )),
                EXIT_USAGE,
            );
        });

    let status = response.status();
    if run_flags.show_status {
        eprintln!("{}", status);
    }

    print_response(&response.body_json(), response.message(), output_mode);
    http_exit_code(status, run_flags)
}
