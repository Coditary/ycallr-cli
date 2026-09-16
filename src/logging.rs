//! CLI log initialization (`--verbose` / `RUST_LOG`).

use std::sync::Once;

static INIT: Once = Once::new();

/// Initialize tracing from argv and environment.
///
/// * `RUST_LOG` — full control when set (standard tracing filter syntax)
/// * `--verbose` / `-v` — enables `debug` for `ycallr` and `ycallr_core` when `RUST_LOG` is unset
/// * otherwise — logging disabled
pub fn init_from_args(args: &[String]) {
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    INIT.call_once(|| {
        use tracing_subscriber::EnvFilter;

        let filter = if std::env::var_os("RUST_LOG").is_some() {
            EnvFilter::from_default_env()
        } else if verbose {
            EnvFilter::new("ycallr=debug,ycallr_core=debug")
        } else {
            EnvFilter::new("off")
        };

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_target(true)
            .without_time()
            .try_init()
            .ok();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_from_args_accepts_verbose_flag() {
        init_from_args(&["--verbose".into(), "install".into()]);
        init_from_args(&["-v".into(), "install".into()]);
    }

    #[test]
    fn init_from_args_respects_rust_log() {
        std::env::set_var("RUST_LOG", "ycallr=debug");
        init_from_args(&[]);
        std::env::remove_var("RUST_LOG");
    }
}
