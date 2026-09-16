use std::io::IsTerminal;

use crate::ffi_api::FfiError;
use crate::secrets;

pub fn run_secret_command(args: &[String]) -> Result<(), FfiError> {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print_secret_help();
        return Ok(());
    }
    match args[0].as_str() {
        "set" => secret_set(&args[1..]),
        "list" => secret_list(),
        "unset" => secret_unset(&args[1..]),
        other => Err(FfiError::new(format!(
            "unknown secret subcommand '{}'. Try: ycallr secret --help",
            other
        ))),
    }
}

fn print_secret_help() {
    println!("ycallr secret: store API credentials locally\n");
    println!("Usage:");
    println!("  ycallr secret set <NAME>     Save secret (hidden prompt)");
    println!("  ycallr secret list           List stored secret names");
    println!("  ycallr secret unset <NAME>   Remove a stored secret\n");
    println!(
        "Storage: OS keyring (service '{}'), file fallback under secrets dir.",
        secrets::KEYRING_SERVICE
    );
    println!("Override dir: YCALLR_SECRETS_DIR");
}

pub(crate) fn read_secret_input<R: std::io::Read>(
    name: &str,
    mut reader: R,
) -> Result<String, FfiError> {
    let mut buf = String::new();
    reader
        .read_to_string(&mut buf)
        .map_err(|e| FfiError::new(format!("failed to read secret from stdin: {}", e)))?;
    let value = buf.trim_end_matches(['\n', '\r']).to_string();
    if value.is_empty() {
        return Err(FfiError::new(format!(
            "required environment variable '{}' cannot be empty",
            name
        )));
    }
    Ok(value)
}

pub(crate) fn store_secret(name: &str, value: &str) -> Result<(), FfiError> {
    secrets::set_secret(name, value).map_err(FfiError::new)?;
    println!("Saved secret '{}'.", name);
    Ok(())
}

fn secret_set(args: &[String]) -> Result<(), FfiError> {
    if args.len() != 1 {
        return Err(FfiError::new("usage: ycallr secret set <NAME>"));
    }
    let name = &args[0];
    let value = if std::io::stdin().is_terminal() {
        eprint!("{} (input hidden): ", name);
        let value = rpassword::read_password()
            .map_err(|e| FfiError::new(format!("failed to read secret: {}", e)))?;
        if value.is_empty() {
            return Err(FfiError::new(format!(
                "required environment variable '{}' cannot be empty",
                name
            )));
        }
        value
    } else {
        read_secret_input(name, std::io::stdin())?
    };
    store_secret(name, &value)
}

fn secret_list() -> Result<(), FfiError> {
    let names = secrets::list_secret_names().map_err(FfiError::new)?;
    if names.is_empty() {
        println!("No stored secrets.");
    } else {
        for name in names {
            println!("{}", name);
        }
    }
    Ok(())
}

fn secret_unset(args: &[String]) -> Result<(), FfiError> {
    if args.len() != 1 {
        return Err(FfiError::new("usage: ycallr secret unset <NAME>"));
    }
    secrets::unset_secret(&args[0]).map_err(FfiError::new)?;
    println!("Removed secret '{}'.", args[0]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_help_and_list() {
        assert!(run_secret_command(&[]).is_ok());
        assert!(run_secret_command(&["--help".into()]).is_ok());
        assert!(run_secret_command(&["list".into()]).is_ok());
    }

    #[test]
    fn secret_set_usage_errors() {
        assert!(run_secret_command(&["set".into()]).is_err());
        assert!(run_secret_command(&["unset".into()]).is_err());
        assert!(run_secret_command(&["nope".into()]).is_err());
    }

    #[test]
    fn secret_unset_missing_is_ok() {
        secrets::with_secrets_dir(|| {
            assert!(run_secret_command(&["unset".into(), "NOPE".into()]).is_ok());
        });
    }

    #[test]
    fn read_secret_input_parses_stdin_value() {
        use std::io::Cursor;
        let value = read_secret_input("TOKEN", Cursor::new("abc\n")).unwrap();
        assert_eq!(value, "abc");
        assert!(read_secret_input("TOKEN", Cursor::new("\n")).is_err());
    }

    #[test]
    fn store_secret_persists_value() {
        secrets::with_secrets_dir(|| {
            store_secret("YCALLR_TEST_STORED", "value").unwrap();
            assert_eq!(
                secrets::get_secret_file("YCALLR_TEST_STORED").as_deref(),
                Some("value")
            );
        });
    }
}
