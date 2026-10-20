//! `nexc-verify` — checks a run's evidence bundle (`GET /runs/{id}/evidence`)
//! offline: no database, no network, only the public keys. Give it the key
//! set published at `/.well-known/jwks.json` (saved to a file), or the one
//! public key you trust. It recomputes the span chain, checks the seal's
//! signature and that it seals this chain, recomputes every audit entry's
//! hash and the links between adjacent entries, and checks the attestation
//! that ties the run, the spans, the seal, the payloads and the audit
//! entries together. Exit status 0 when everything holds, 1 when anything
//! does not, 2 when the inputs cannot be read.
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use base64::Engine as _;
use clap::Parser;
use nexc::domain::bundle::{self, EvidenceBundle};
use nexc::security::signing::{Jwk, Jwks, KeyStatus};

/// Checks a run's evidence bundle with the published public keys alone.
#[derive(Debug, Parser)]
#[command(name = "nexc-verify", version)]
struct Args {
    /// The bundle (`run-<id>.evidence.json`).
    bundle: PathBuf,
    /// The published key set: `/.well-known/jwks.json` saved to a file.
    #[arg(long, conflicts_with = "key", required_unless_present = "key")]
    keys: Option<PathBuf>,
    /// One Ed25519 public key to trust, in hex or base64 (32 bytes).
    #[arg(long)]
    key: Option<String>,
    /// Print the report as JSON.
    #[arg(long)]
    json: bool,
}

fn read_keys(args: &Args) -> Result<Jwks, String> {
    if let Some(path) = &args.keys {
        let text = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        return serde_json::from_slice(&text)
            .map_err(|e| format!("{}: not a JWK set: {e}", path.display()));
    }
    let raw = args.key.as_deref().unwrap_or_default().trim();
    let bytes = hex::decode(raw)
        .ok()
        .or_else(|| base64::engine::general_purpose::STANDARD.decode(raw).ok())
        .or_else(|| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(raw)
                .ok()
        })
        .ok_or("--key: not hex or base64")?;
    let key: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "--key: an Ed25519 public key is 32 bytes")?;
    Ok(Jwks {
        keys: vec![Jwk::of(&key, KeyStatus::Current)],
    })
}

fn main() -> ExitCode {
    let args = Args::parse();
    let keys = match read_keys(&args) {
        Ok(keys) => keys,
        Err(err) => {
            eprintln!("nexc-verify: {err}");
            return ExitCode::from(2);
        }
    };
    let bundle: EvidenceBundle = match std::fs::read(&args.bundle)
        .map_err(|e| e.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
    {
        Ok(bundle) => bundle,
        Err(err) => {
            eprintln!("nexc-verify: {}: {err}", args.bundle.display());
            return ExitCode::from(2);
        }
    };
    let report = bundle::verify(&bundle, &keys);
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_default()
        );
    } else {
        println!("run {}", report.run_id);
        for check in &report.checks {
            let mark = if check.ok { "ok  " } else { "FAIL" };
            println!("  {mark} {:<12} {}", check.name, check.detail);
        }
        println!(
            "{}",
            if report.ok {
                "the bundle verifies"
            } else {
                "the bundle does NOT verify"
            }
        );
    }
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
