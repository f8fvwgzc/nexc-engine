//! The generated gRPC services this backend speaks beside onto-intelli's:
//! the agent runtime's stream (`nexc.runtime.v1`, the backend is the client)
//! and the Agent Firewall API (`nexc.firewall.v1`, the backend is the
//! server). Both are compiled by `build.rs` from `proto/` with protox.
//!
//! Every client channel the backend opens (to the runtime, to onto-intelli,
//! to the OTLP collector) is built by [`channel`]: a small pool of HTTP/2
//! connections with keepalive pings and adaptive flow control, opened once
//! and cloned into every client for the life of the process (a clone shares
//! the connections; no request opens one of its own). Messages travel
//! uncompressed: tonic's `gzip` feature is not enabled in this build.
//!
//! An `https://` endpoint is reached over TLS ([`ClientTls`]: the
//! operator's CA, else the system's roots, and a client certificate for a
//! server that asks for one); `http://` stays plaintext, for a hop that
//! never leaves one host or a network policy. The Agent Firewall API serves
//! TLS with [`ServerTls`].
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use tonic::transport::{
    Certificate, Channel, ClientTlsConfig, Endpoint, Identity, ServerTlsConfig,
};

/// How a channel to an `https://` endpoint is secured (`NEXC_GRPC_CA_FILE`,
/// `NEXC_GRPC_CLIENT_CERT_FILE`, `NEXC_GRPC_CLIENT_KEY_FILE`), PEM.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ClientTls {
    /// The CA bundle the server's certificate must chain to; the system's
    /// roots when unset.
    pub ca: Option<Arc<[u8]>>,
    /// A certificate and its key, for a server that asks for one (mutual TLS).
    pub identity: Option<CertKey>,
}

/// A certificate and its private key, PEM.
#[derive(Clone, PartialEq, Eq)]
pub struct CertKey {
    pub cert: Arc<[u8]>,
    pub key: Arc<[u8]>,
}

impl std::fmt::Debug for ClientTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientTls")
            .field("ca", &self.ca.is_some())
            .field("identity", &self.identity.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl ClientTls {
    fn config(&self) -> ClientTlsConfig {
        let config = match &self.ca {
            Some(ca) => ClientTlsConfig::new().ca_certificate(Certificate::from_pem(ca)),
            None => ClientTlsConfig::new().with_native_roots(),
        };
        match &self.identity {
            Some(pair) => config.identity(Identity::from_pem(&pair.cert, &pair.key)),
            None => config,
        }
    }
}

/// The Agent Firewall API's TLS (`NEXC_GRPC_TLS_CERT_FILE`,
/// `NEXC_GRPC_TLS_KEY_FILE`, and `NEXC_GRPC_TLS_CLIENT_CA_FILE` to ask every
/// caller for a certificate of that CA), PEM.
#[derive(Clone, PartialEq, Eq)]
pub struct ServerTls {
    pub cert: Arc<[u8]>,
    pub key: Arc<[u8]>,
    pub client_ca: Option<Arc<[u8]>>,
}

impl std::fmt::Debug for ServerTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerTls")
            .field("key", &"[redacted]")
            .field("client_ca", &self.client_ca.is_some())
            .finish()
    }
}

impl ServerTls {
    /// The server's TLS configuration.
    pub fn config(&self) -> ServerTlsConfig {
        let config = ServerTlsConfig::new().identity(Identity::from_pem(&self.cert, &self.key));
        match &self.client_ca {
            Some(ca) => config.client_ca_root(Certificate::from_pem(ca)),
            None => config,
        }
    }
}

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
/// `NEXC_GRPC_POOL_SIZE`), and its TLS when the endpoint is `https://`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelSettings {
    /// Opening a TCP connection.
    pub connect_timeout: Duration,
    /// How often a PING goes out on an idle or busy connection.
    pub keepalive: Duration,
    /// How long a PING may go unanswered before the connection is dropped.
    pub keepalive_timeout: Duration,
    /// Connections per channel; requests and streams are spread over them.
    pub pool: usize,
    /// How an `https://` endpoint is reached.
    pub tls: ClientTls,
}

impl Default for ChannelSettings {
    fn default() -> Self {
        ChannelSettings {
            connect_timeout: Duration::from_secs(10),
            keepalive: Duration::from_secs(30),
            keepalive_timeout: Duration::from_secs(10),
            pool: 2,
            tls: ClientTls::default(),
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

/// One connection to `url`: keepalive pings, idle or not, and adaptive flow
/// control: in place of hyper's fixed windows on a client (2 MiB per stream,
/// 5 MiB per connection), one that starts at HTTP/2's 64 KiB and grows with
/// the bandwidth-delay product hyper measures with its pings. TLS for an
/// `https://` URL.
pub fn endpoint(
    url: &str,
    settings: &ChannelSettings,
) -> Result<Endpoint, tonic::transport::Error> {
    let endpoint = Endpoint::from_shared(url.to_owned())?
        .connect_timeout(settings.connect_timeout)
        .tcp_nodelay(true)
        .http2_keep_alive_interval(settings.keepalive)
        .keep_alive_timeout(settings.keepalive_timeout)
        .keep_alive_while_idle(true)
        .http2_adaptive_window(true);
    if url.starts_with("https://") {
        endpoint.tls_config(settings.tls.config())
    } else {
        Ok(endpoint)
    }
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
