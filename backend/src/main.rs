//! `nexc` — the nexc-engine backend binary. All logic lives in the library.
#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser;
use nexc::cli::{self, Cli};
use nexc::{config, observability};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let dotenv = config::load_dotenv();
    observability::init_tracing(cli::wants_json_logs());
    if let Some(path) = dotenv {
        tracing::debug!(path = %path.display(), "loaded .env");
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("error: cannot start the async runtime: {err}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(cli::run(cli)) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
