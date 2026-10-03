//! Logging / tracing setup and Prometheus metrics.
#![forbid(unsafe_code)]

pub mod metrics;

use tracing_subscriber::EnvFilter;

/// Installs the global tracing subscriber: JSON lines in production,
/// human-readable output in development. The filter comes from `NEXC_LOG`
/// (or `RUST_LOG`), defaulting to `info`.
pub fn init_tracing(json: bool) {
    let filter = std::env::var("NEXC_LOG")
        .ok()
        .and_then(|f| EnvFilter::try_new(f).ok())
        .or_else(|| EnvFilter::try_from_default_env().ok())
        .unwrap_or_else(|| EnvFilter::new("info,sqlx=warn,tower_http=info"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true);
    let result = if json {
        builder
            .json()
            .flatten_event(true)
            .with_current_span(false)
            .try_init()
    } else {
        builder.compact().try_init()
    };
    // A subscriber may already be installed (tests); that is fine.
    drop(result);
}
