//! The generated gRPC services this backend speaks beside onto-intelli's:
//! the agent runtime's stream (`nexc.runtime.v1`, the backend is the client)
//! and the Agent Firewall API (`nexc.firewall.v1`, the backend is the
//! server). Both are compiled by `build.rs` from `proto/` with protox.
//!
//! Every client channel the backend opens (to the runtime, to onto-intelli,
//! to the OTLP collector) is built by [`channel`]: a small pool of HTTP/2
//! connections with keepalive pings, reused for the life of the process.
#![forbid(unsafe_code)]

use std::time::Duration;

use tonic::transport::{Channel, Endpoint};

/// `nexc.runtime.v1`: the backend ⇄ runtime stream (CONTRACT §8).
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    unreachable_pub,
    rustdoc::all
)]
pub mod runtime {
    tonic::include_proto!("nexc.runtime.v1");
}

/// `nexc.firewall.v1`: the Agent Firewall API.
#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    unreachable_pub,
    rustdoc::all
)]
pub mod firewall {
    tonic::include_proto!("nexc.firewall.v1");
}

/// The descriptor set of every vendored proto, for server reflection.
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("nexc_descriptors");

/// How a client channel is kept: HTTP/2 keepalive and the size of its pool
/// (`NEXC_GRPC_KEEPALIVE_SECS`, `NEXC_GRPC_KEEPALIVE_TIMEOUT_SECS`,
/// `NEXC_GRPC_POOL_SIZE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelSettings {
    /// Opening a TCP connection.
    pub connect_timeout: Duration,
    /// How often a PING goes out on an idle or busy connection.
    pub keepalive: Duration,
    /// How long a PING may go unanswered before the connection is dropped.
    pub keepalive_timeout: Duration,
    /// Connections per channel; requests and streams are spread over them.
    pub pool: usize,
}

impl Default for ChannelSettings {
    fn default() -> Self {
        ChannelSettings {
            connect_timeout: Duration::from_secs(10),
            keepalive: Duration::from_secs(30),
            keepalive_timeout: Duration::from_secs(10),
            pool: 2,
        }
    }
}

impl ChannelSettings {
    /// The same settings with another connect timeout.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }
}

fn endpoint(url: &str, settings: &ChannelSettings) -> Result<Endpoint, tonic::transport::Error> {
    Ok(Endpoint::from_shared(url.to_owned())?
        .connect_timeout(settings.connect_timeout)
        .tcp_nodelay(true)
        .http2_keep_alive_interval(settings.keepalive)
        .keep_alive_timeout(settings.keepalive_timeout)
        .keep_alive_while_idle(true))
}

/// A lazily connected channel to `url`: `settings.pool` connections, each
/// with HTTP/2 keepalive, requests balanced over them; a server that is down
/// is a failed request, never a failed start.
pub fn channel(url: &str, settings: &ChannelSettings) -> Result<Channel, tonic::transport::Error> {
    let pool = settings.pool.clamp(1, 64);
    if pool == 1 {
        return Ok(endpoint(url, settings)?.connect_lazy());
    }
    let endpoints: Vec<Endpoint> = (0..pool)
        .map(|_| endpoint(url, settings))
        .collect::<Result<_, _>>()?;
    Ok(Channel::balance_list(endpoints.into_iter()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn channels_are_built_lazily_in_a_pool() {
        let one = ChannelSettings {
            pool: 1,
            ..ChannelSettings::default()
        };
        assert!(channel("http://127.0.0.1:1", &one).is_ok());
        assert!(channel("http://127.0.0.1:1", &ChannelSettings::default()).is_ok());
        assert!(channel("not a url", &ChannelSettings::default()).is_err());
        assert_eq!(
            ChannelSettings::default()
                .connect_timeout(Duration::from_secs(3))
                .connect_timeout,
            Duration::from_secs(3)
        );
    }
}
