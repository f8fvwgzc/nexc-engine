//! The `nexc` command line.
#![forbid(unsafe_code)]

mod doctor;
mod init;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::app::{self, AppState};
use crate::config::{Settings, vars::ENV_VARS};
use crate::domain::user::{self, Role};
use crate::domain::validation::FieldErrors;
use crate::{http, repo, security};

/// nexc-engine backend.
#[derive(Debug, Parser)]
#[command(
    name = "nexc",
    version,
    about = "nexc-engine backend: API server, scheduler and tooling"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the HTTP API, scheduler and background workers.
    Serve,
    /// Apply database migrations and exit.
    Migrate,
    /// Write a `.env` with freshly generated secrets.
    Init {
        /// Where to write the file.
        #[arg(long, default_value = "../.env")]
        path: PathBuf,
        /// Overwrite an existing file.
        #[arg(long)]
        force: bool,
    },
    /// Manage users.
    User {
        #[command(subcommand)]
        command: UserCommand,
    },
    /// Check configuration, database and backends; non-zero exit on failure.
    Doctor,
    /// Inspect configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Print the OpenAPI document (JSON) to stdout.
    Openapi,
}

/// `nexc user …`
#[derive(Debug, Subcommand)]
pub enum UserCommand {
    /// Create a user. The password is read from `NEXC_PASSWORD` or prompted for.
    Create {
        #[arg(long)]
        email: String,
        #[arg(long)]
        name: String,
        /// Grant the admin role.
        #[arg(long)]
        admin: bool,
    },
}

/// `nexc config …`
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the effective configuration with secrets redacted.
    Show,
}

/// Whether a command needs the tracing subscriber in JSON mode.
pub fn wants_json_logs() -> bool {
    std::env::var("NEXC_ENV").is_ok_and(|e| e == "production" || e == "prod")
}

/// Executes a parsed command.
pub async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Serve => app::serve(Settings::from_env()?)
            .await
            .map(|()| ExitCode::SUCCESS),
        Command::Migrate => {
            app::connect_db(&Settings::from_env()?).await?;
            println!("migrations applied");
            Ok(ExitCode::SUCCESS)
        }
        Command::Init { path, force } => init::run(&path, force).map(|()| ExitCode::SUCCESS),
        Command::User {
            command: UserCommand::Create { email, name, admin },
        } => create_user(&email, &name, admin)
            .await
            .map(|()| ExitCode::SUCCESS),
        Command::Doctor => Ok(doctor::run().await),
        Command::Config {
            command: ConfigCommand::Show,
        } => Ok(show_config()),
        Command::Openapi => {
            println!("{}", serde_json::to_string_pretty(&http::openapi())?);
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn read_password() -> anyhow::Result<String> {
    if let Ok(pw) = std::env::var("NEXC_PASSWORD") {
        return Ok(pw);
    }
    let first = rpassword::prompt_password("Password: ")?;
    let second = rpassword::prompt_password("Repeat password: ")?;
    anyhow::ensure!(first == second, "passwords do not match");
    Ok(first)
}

async fn create_user(email: &str, name: &str, admin: bool) -> anyhow::Result<()> {
    let settings = Settings::from_env()?;
    let email = user::normalize_email(email);
    let password = read_password()?;
    let mut errors = FieldErrors::default();
    user::check_email(&mut errors, &email);
    user::check_name(&mut errors, name);
    user::check_password(&mut errors, &password);
    if !errors.is_empty() {
        anyhow::bail!("invalid input: {:?}", errors.as_map());
    }
    let db = app::connect_db(&settings).await?;
    anyhow::ensure!(
        !repo::users::email_exists(&db, &email).await?,
        "a user with this e-mail already exists"
    );
    let state = AppState::new(settings, db)?;
    let hash = security::password::hash_password(&password)?;
    let role = if admin { Role::Admin } else { Role::User };
    let created = http::handlers::auth::create_user(&state, &email, name.trim(), role, &hash)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!(
        "created {} user {} ({})",
        created.role, created.email, created.id
    );
    Ok(())
}

fn show_config() -> ExitCode {
    for var in ENV_VARS {
        let shown = match std::env::var(var.name).ok().filter(|v| !v.is_empty()) {
            Some(_) if var.secret => "<set, redacted>".to_owned(),
            Some(v) => v,
            None if var.default.is_empty() => "<unset>".to_owned(),
            None => format!("{} (default)", var.default),
        };
        println!("{:<26} {shown}", var.name);
    }
    match Settings::from_env() {
        Ok(_) => {
            println!("\nconfiguration is valid");
            ExitCode::SUCCESS
        }
        Err(err) => {
            println!("\n{err}");
            ExitCode::FAILURE
        }
    }
}
