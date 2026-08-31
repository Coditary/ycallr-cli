use std::collections::HashMap;
use std::io::IsTerminal;
use ycallr_core::models::{ApiDefinition, Command as ApiCommand, HttpMethod, ParamType};
use ycallr_core::{yaml_parser, YcallrClient};

fn load_api(api_name: &str) -> ApiDefinition {
    let yaml_path = dirs::home_dir()
        .expect("Could not find home directory")
        .join(".config")
        .join("ycallr")
        .join("apis")
        .join(format!("{}.yaml", api_name));

    yaml_parser::parse_yaml_file(&yaml_path)
        .unwrap_or_else(|e| panic!("Failed to load API '{}': {}", api_name, e))
}

fn list_apis() {
    let apis_dir = dirs::home_dir()
        .unwrap()
        .join(".config/ycallr/apis");
    let entries: Vec<_> = std::fs::read_dir(&apis_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "yaml")
                .unwrap_or(false)
        })
        .collect();
    for entry in entries {
        let name = entry
            .path()
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let api = load_api(&name);
        println!("  {:<25} {}", name, api.description);
    }
}

fn print_api_help(api: &ApiDefinition) {
    println!("{} v{} - {}", api.name, api.version, api.description);
    println!("\nCommands:");
    for (name, cmd) in &api.commands {
        let desc = cmd.description.as_deref().unwrap_or("no description");
        println!("  {:<25} {}", name, desc);
    }
    println!("\nUsage: ycallr {} <command> [options]", api.name);
}

fn print_command_help(api: &ApiDefinition, command_name: &str) {
    let cmd = api
        .commands
        .get(command_name)
        .unwrap_or_else(|| panic!("Command '{}' not found in {}", command_name, api.name));

    let desc = cmd.description.as_deref().unwrap_or("no description");
    println!("{} {} - {}", api.name, command_name, desc);
    println!(
        "\nMethod: {}",
        cmd.method
            .as_ref()
            .map(|m| m.as_str())
            .unwrap_or("N/A")
    );
    println!("Endpoint: {}", cmd.endpoint.as_deref().unwrap_or("N/A"));

    if !cmd.params.is_empty() {
        println!("\nParameters:");
        for (name, param) in &cmd.params {
            let required = if param.required { " (required)" } else { "" };
            println!(
                "  --{:<20} {} [{}]{}",
                name,
                param.description,
                format_param_type(&param.param_type),
                required
            );
        }
    }
}

fn format_param_type(t: &ParamType) -> &'static str {
    match t {
        ParamType::String => "string",
        ParamType::Number => "number",
        ParamType::Boolean => "boolean",
        ParamType::Array => "array",
    }
}

fn prompt_param(_name: &str, param: &ycallr_core::models::Parameter) -> String {
    let type_hint = match param.param_type {
        ParamType::Number => " (number)",
        ParamType::Boolean => " (true/false)",
        _ => "",
    };
    if std::io::stdin().is_terminal() {
        eprint!("{}{}: ", param.description, type_hint);
    }
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).expect("Failed to read input");
    input.trim().to_string()
}

fn parse_params(args: &[String], cmd: &ApiCommand) -> (HashMap<String, String>, bool) {
    let mut params = HashMap::new();
    let mut json = false;
    for arg in args {
        if arg == "--json" || arg == "--help" || arg == "-h" {
            json = json || *arg == "--json";
        } else if let Some((key, value)) = arg.split_once('=') {
            if key.starts_with("--") {
                params.insert(key[2..].to_string(), value.to_string());
            }
        } else if arg.starts_with("--") {
            eprintln!("Invalid format: '{}'. Use --key=value", arg);
            std::process::exit(1);
        }
    }

    let missing: Vec<_> = cmd
        .params
        .iter()
        .filter(|(name, p)| p.required && !params.contains_key(*name))
        .collect();

    if missing.is_empty() {
        return (params, json);
    }

    if std::io::stdin().is_terminal() {
        eprintln!("Missing required parameters. Enter values:\n");
    }

    for (name, param) in &missing {
        let value = prompt_param(name, param);
        if value.is_empty() {
            eprintln!("Required parameter '{}' cannot be empty", name);
            std::process::exit(1);
        }
        params.insert(name.to_string(), value);
    }

    (params, json)
}

fn build_body(cmd: &ApiCommand, params: &HashMap<String, String>) -> Option<serde_json::Value> {
    if let Some(body_config) = &cmd.body {
        if let Some(json_template) = &body_config.json {
            let mut body = json_template.clone();
            for (key, value) in params {
                if let serde_json::Value::String(s) = &body {
                    if s == &format!("{{{{{}}}}}", key) {
                        body = serde_json::Value::String(value.clone());
                    }
                }
            }
            return Some(body);
        }
        return None;
    }

    match cmd.method.as_ref() {
        Some(HttpMethod::POST) | Some(HttpMethod::PUT) | Some(HttpMethod::PATCH) => {
            let body: serde_json::Map<String, serde_json::Value> = params
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            Some(serde_json::Value::Object(body))
        }
        _ => None,
    }
}

fn print_help() {
    println!("ycallr - CLI frontend for ycallr-core\n");
    println!("Usage: ycallr <api> <command> [options]\n");
    println!("Commands:");
    list_apis();
    println!("\nOptions:");
    println!("  --help, -h    Show this help message");
    println!("  --json        Output raw JSON response body\n");
    println!("Examples:");
    println!("  ycallr github --help              Show GitHub API commands");
    println!("  ycallr github create-issue --help Show create-issue help");
    println!(
        "  ycallr github create-issue --owner rust-lang --repo rust --title \"Bug\""
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print_help();
        return;
    }

    let api_name = &args[0];

    let has_help = args.iter().any(|a| a == "--help" || a == "-h");

    if args.len() == 1 {
        let api = load_api(api_name);
        print_api_help(&api);
        return;
    }

    let command_name = if args[1] == "--help" || args[1] == "-h" {
        let api = load_api(api_name);
        print_api_help(&api);
        return;
    } else {
        &args[1]
    };

    if has_help {
        let api = load_api(api_name);
        print_command_help(&api, command_name);
        return;
    }

    let api = load_api(api_name);

    let cmd = api
        .commands
        .get(command_name)
        .unwrap_or_else(|| panic!("Command '{}' not found in {}", command_name, api.name));

    let (params, json) = parse_params(&args[2..], cmd);
    let body = build_body(cmd, &params);

    let client = YcallrClient::new(api).expect("Failed to create client");

    let response = client
        .call(command_name, &params, body.as_ref())
        .unwrap_or_else(|e| panic!("Failed to execute '{}': {}", command_name, e));

    if json {
        println!("{}", serde_json::to_string_pretty(&response.body).unwrap());
    } else if let Some(msg) = &response.message {
        println!("{}", msg);
    } else {
        println!("{}", serde_json::to_string_pretty(&response.body).unwrap());
    }
}
