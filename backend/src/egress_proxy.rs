//! `nexc egress-proxy`: the agent runtime's only way out of its network in
//! Docker Compose. A forward proxy that connects only to the hosts and ports
//! its allow-list names: `CONNECT host:port` for TLS (the LLM APIs), and
//! absolute-form plain HTTP (`GET http://host:port/…`) for a local provider
//! such as Ollama. Anything else is answered 403 and logged.
//!
//! A tunnel must carry TLS whose ClientHello names the host the tunnel was
//! opened to (SNI): a CDN that routes by server name cannot be asked for
//! another site behind an allowed address. It is not the tools gateway: it
//! sees host names, ports and byte counts, never what travels inside TLS, and
//! it decides nothing per agent. It keeps a compromised runtime from reaching
//! the rest of the internet (CONTRACT §8, docs/AGENTSECOPS.md, the runtime's
//! egress).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// The longest request head read before a connection is refused.
const MAX_HEAD: usize = 16 * 1024;
/// How long a client may take to send its request head.
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);
/// How long connecting to one address of an allowed host may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long resolving and connecting may take in all, whatever the addresses.
const CONNECT_BUDGET: Duration = Duration::from_secs(20);
/// Connections served at once; one more is answered 503.
const MAX_CONNECTIONS: usize = 512;
/// Connections one client address may hold at once, so one runtime replica
/// cannot take every slot from the others.
const MAX_PER_PEER: usize = 128;
/// The longest a connection lives: longer than any node may run (the
/// runtime caps a run at 30 minutes), short enough that nothing is held
/// forever.
const MAX_LIFETIME: Duration = Duration::from_secs(3600);
/// The port a host listed without one is allowed on.
const DEFAULT_PORT: u16 = 443;

/// One entry of the allow-list: a host (or every subdomain of one) on one port.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    /// Lower case, without a trailing dot; the suffix after `*.` for a wildcard.
    host: String,
    wildcard: bool,
    port: u16,
}

/// Where the runtime may connect: `host[:port]`, comma separated. A host
/// without a port is allowed on 443 only; `*.example.com` allows every
/// subdomain of `example.com` (not `example.com` itself); an IPv6 literal is
/// written in brackets (`[fd00::1]:11434`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllowList {
    rules: Vec<Rule>,
}

/// `host` lower case without a trailing dot or IPv6 brackets.
fn normalize_host(host: &str) -> String {
    host.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

/// Splits `host:port`, `[v6]:port` or `host` (port `default`).
fn split_authority(raw: &str, default: Option<u16>) -> Option<(String, u16)> {
    let raw = raw.trim();
    let (host, port) = if let Some(rest) = raw.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        match after.strip_prefix(':') {
            Some(port) => (host, Some(port)),
            None if after.is_empty() => (host, None),
            None => return None,
        }
    } else {
        match raw.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, Some(port)),
            Some(_) => return None,
            None => (raw, None),
        }
    };
    let port = match port {
        Some(p) => p.parse::<u16>().ok().filter(|p| *p != 0)?,
        None => default?,
    };
    let host = normalize_host(host);
    let valid = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '*'));
    valid.then_some((host, port))
}

impl AllowList {
    /// Parses `NEXC_EGRESS_ALLOW`.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut rules = Vec::new();
        for entry in raw.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (host, port) = split_authority(entry, Some(DEFAULT_PORT))
                .ok_or_else(|| format!("`{entry}` is not host[:port]"))?;
            let (host, wildcard) = match host.strip_prefix("*.") {
                Some(suffix) => (suffix.to_owned(), true),
                None => (host, false),
            };
            if host.contains('*') || host.is_empty() {
                return Err(format!(
                    "`{entry}`: a wildcard is only `*.` before a domain"
                ));
            }
            rules.push(Rule {
                host,
                wildcard,
                port,
            });
        }
        Ok(AllowList { rules })
    }

    /// Whether a connection to `host:port` may be opened.
    pub fn allows(&self, host: &str, port: u16) -> bool {
        let host = normalize_host(host);
        self.rules.iter().any(|r| {
            r.port == port
                && if r.wildcard {
                    host.len() > r.host.len() + 1
                        && host.ends_with(&r.host)
                        && host.as_bytes()[host.len() - r.host.len() - 1] == b'.'
                } else {
                    host == r.host
                }
        })
    }

    /// The entries, as they are logged at start.
    pub fn describe(&self) -> String {
        if self.rules.is_empty() {
            return "nothing".into();
        }
        self.rules
            .iter()
            .map(|r| {
                let host = if r.host.contains(':') {
                    format!("[{}]", r.host)
                } else {
                    r.host.clone()
                };
                format!("{}{host}:{}", if r.wildcard { "*." } else { "" }, r.port)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// What a client asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    /// `CONNECT host:port`: a tunnel.
    Tunnel { host: String, port: u16 },
    /// Absolute-form plain HTTP: the head to send upstream, rewritten to
    /// origin form with `Connection: close`.
    Forward {
        host: String,
        port: u16,
        head: Vec<u8>,
    },
    /// `GET /healthz`, asked of the proxy itself.
    Health,
}

/// Headers about the hop to the proxy, never passed on.
const HOP_HEADERS: [&str; 4] = [
    "proxy-connection",
    "proxy-authorization",
    "connection",
    "keep-alive",
];

/// Reads what a request head asks for.
fn parse_head(head: &[u8]) -> Result<Request, &'static str> {
    let text = std::str::from_utf8(head).map_err(|_| "the request head is not UTF-8")?;
    let mut lines = text.split("\r\n");
    let line = lines.next().unwrap_or_default();
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err("the request line is not METHOD TARGET VERSION");
    };
    if !version.starts_with("HTTP/1.") {
        return Err("only HTTP/1.x is proxied");
    }
    if method == "CONNECT" {
        let (host, port) = split_authority(target, None).ok_or("CONNECT needs host:port")?;
        return Ok(Request::Tunnel { host, port });
    }
    if target == "/healthz" && method == "GET" {
        return Ok(Request::Health);
    }
    if target.starts_with("https://") {
        return Err("an https URL goes through CONNECT");
    }
    let rest = target
        .strip_prefix("http://")
        .ok_or("only absolute http:// URLs and CONNECT are proxied")?;
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    if authority.contains('@') {
        return Err("credentials in the URL are not proxied");
    }
    let (host, port) = split_authority(authority, Some(80)).ok_or("the URL names no host")?;
    let path = match path {
        "" => "/".to_owned(),
        p if p.starts_with('?') => format!("/{p}"),
        p => p.to_owned(),
    };
    // The upstream is told the host the allow-list admitted, never the client's own `Host`:
    // a reverse proxy behind an allowed address routes by it.
    let mut out = format!("{method} {path} {version}\r\nHost: {authority}\r\n");
    for header in lines.take_while(|l| !l.is_empty()) {
        let name = header.split(':').next().unwrap_or_default().trim();
        if name.eq_ignore_ascii_case("host")
            || HOP_HEADERS.iter().any(|h| name.eq_ignore_ascii_case(h))
        {
            continue;
        }
        out.push_str(header);
        out.push_str("\r\n");
    }
    out.push_str("Connection: close\r\n\r\n");
    Ok(Request::Forward {
        host,
        port,
        head: out.into_bytes(),
    })
}

/// The largest ClientHello read before a tunnel is refused (a hello with
/// post-quantum key shares is a few KiB).
const MAX_HELLO: usize = 32 * 1024;

/// What the first TLS handshake message of a tunnel says: the server name
/// of its ClientHello (RFC 8446 §4.1.2, RFC 6066 §3), `None` when it names
/// none. `Err` when the bytes are not a ClientHello, `Ok(Err(n))` while at
/// least `n` more bytes are needed.
fn client_hello_sni(bytes: &[u8]) -> Result<Result<Option<String>, usize>, &'static str> {
    // The handshake message may span records: gather their payloads.
    let mut handshake = Vec::new();
    let mut at = 0;
    loop {
        let Some(header) = bytes.get(at..at + 5) else {
            return Ok(Err(at + 5 - bytes.len()));
        };
        if header[0] != 0x16 || header[1] != 0x03 {
            return Err("the tunnel does not start with a TLS handshake");
        }
        let len = usize::from(u16::from_be_bytes([header[3], header[4]]));
        let Some(payload) = bytes.get(at + 5..at + 5 + len) else {
            return Ok(Err(at + 5 + len - bytes.len()));
        };
        handshake.extend_from_slice(payload);
        at += 5 + len;
        if handshake.len() >= 4 {
            if handshake[0] != 0x01 {
                return Err("the first handshake message is not a ClientHello");
            }
            let need = 4
                + (usize::from(handshake[1]) << 16
                    | usize::from(handshake[2]) << 8
                    | usize::from(handshake[3]));
            if handshake.len() >= need {
                return sni_of_hello(&handshake[4..need]).map(Ok);
            }
            if need > MAX_HELLO {
                return Err("the ClientHello is too large");
            }
        }
    }
}

/// The `server_name` extension of a ClientHello body.
fn sni_of_hello(hello: &[u8]) -> Result<Option<String>, &'static str> {
    const BAD: &str = "the ClientHello is malformed";
    let mut r = Reader(hello);
    r.take(2 + 32).ok_or(BAD)?; // legacy_version, random
    let n = r.u8().ok_or(BAD)?;
    r.take(n.into()).ok_or(BAD)?; // legacy_session_id
    let n = r.u16().ok_or(BAD)?;
    r.take(n.into()).ok_or(BAD)?; // cipher_suites
    let n = r.u8().ok_or(BAD)?;
    r.take(n.into()).ok_or(BAD)?; // legacy_compression_methods
    if r.0.is_empty() {
        return Ok(None); // no extensions at all
    }
    let n = r.u16().ok_or(BAD)?;
    let mut extensions = Reader(r.take(n.into()).ok_or(BAD)?);
    while !extensions.0.is_empty() {
        let kind = extensions.u16().ok_or(BAD)?;
        let n = extensions.u16().ok_or(BAD)?;
        let data = extensions.take(n.into()).ok_or(BAD)?;
        if kind != 0 {
            continue;
        }
        let mut list = Reader(data);
        let n = list.u16().ok_or(BAD)?;
        let mut names = Reader(list.take(n.into()).ok_or(BAD)?);
        while !names.0.is_empty() {
            let name_type = names.u8().ok_or(BAD)?;
            let n = names.u16().ok_or(BAD)?;
            let name = names.take(n.into()).ok_or(BAD)?;
            if name_type == 0 {
                let name = std::str::from_utf8(name).map_err(|_| BAD)?;
                return Ok(Some(name.to_owned()));
            }
        }
        return Ok(None);
    }
    Ok(None)
}

/// A cursor over bytes that never reads past the end.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }
}

/// Reads a tunnel's ClientHello; returns the bytes read (to pass on) and
/// the server name it asks for.
async fn read_client_hello(
    client: &mut TcpStream,
    mut buf: Vec<u8>,
) -> Result<(Vec<u8>, Option<String>), &'static str> {
    loop {
        match client_hello_sni(&buf)? {
            Ok(sni) => return Ok((buf, sni)),
            Err(_) if buf.len() > MAX_HELLO => return Err("the ClientHello is too large"),
            Err(_) => {
                let mut chunk = [0u8; 4096];
                let n = client
                    .read(&mut chunk)
                    .await
                    .map_err(|_| "the client went away")?;
                if n == 0 {
                    return Err("the client went away");
                }
                buf.extend_from_slice(&chunk[..n]);
            }
        }
    }
}

/// Whether a tunnel to `host` may carry a ClientHello naming `sni`: the same
/// name, or none at all for an address written as an IP.
fn sni_matches(host: &str, sni: Option<&str>) -> bool {
    match sni {
        Some(name) => normalize_host(name) == host,
        None => host.parse::<std::net::IpAddr>().is_ok(),
    }
}

/// Reads the request head; returns it and the bytes after it.
async fn read_head(client: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>), &'static str> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    loop {
        let n = client
            .read(&mut chunk)
            .await
            .map_err(|_| "the client went away")?;
        if n == 0 {
            return Err("the client went away");
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(at + 4);
            buf.truncate(at);
            return Ok((buf, rest));
        }
        if buf.len() > MAX_HEAD {
            return Err("the request head is too large");
        }
    }
}

async fn answer(client: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = client.write_all(response.as_bytes()).await;
    let _ = client.shutdown().await;
}

/// Serves one client connection to its end.
async fn handle(mut client: TcpStream, peer: SocketAddr, allow: &AllowList) {
    let head = tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut client)).await;
    let (head, rest) = match head {
        Ok(Ok(read)) => read,
        Ok(Err(reason)) => {
            answer(&mut client, "400 Bad Request", reason).await;
            return;
        }
        Err(_) => {
            answer(
                &mut client,
                "408 Request Timeout",
                "no request head in time",
            )
            .await;
            return;
        }
    };
    let request = match parse_head(&head) {
        Ok(request) => request,
        Err(reason) => {
            tracing::warn!(%peer, reason, "egress refused: not a request this proxy serves");
            answer(&mut client, "400 Bad Request", reason).await;
            return;
        }
    };
    let (host, port, upstream_head) = match request {
        Request::Health => {
            answer(&mut client, "200 OK", "ok").await;
            return;
        }
        Request::Tunnel { host, port } => (host, port, None),
        Request::Forward { host, port, head } => (host, port, Some(head)),
    };
    if !allow.allows(&host, port) {
        tracing::warn!(%peer, host, port, "egress denied: not on the allow-list");
        answer(
            &mut client,
            "403 Forbidden",
            &format!(
                "egress to {host}:{port} is not on the runtime's allow-list \
                 (NEXC_EGRESS_ALLOW of runtime-egress: NEXC_RUNTIME_EGRESS_ALLOW in Compose and \
                 Kubernetes, runtime.egress.allow in the chart)"
            ),
        )
        .await;
        return;
    }
    let connected = tokio::time::timeout(CONNECT_BUDGET, connect(&host, port))
        .await
        .unwrap_or_else(|_| Err("timed out".to_owned()));
    let mut upstream = match connected {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!(%peer, host, port, error, "egress allowed but the host is unreachable");
            answer(
                &mut client,
                "502 Bad Gateway",
                &format!("{host}:{port} is unreachable"),
            )
            .await;
            return;
        }
    };
    let rest = match &upstream_head {
        None => {
            if client
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .is_err()
            {
                return;
            }
            // The tunnel carries TLS for the host it was opened to, nothing else.
            let hello = tokio::time::timeout(HEAD_TIMEOUT, read_client_hello(&mut client, rest));
            match hello.await {
                Ok(Ok((bytes, sni))) if sni_matches(&host, sni.as_deref()) => bytes,
                Ok(Ok((_, sni))) => {
                    tracing::warn!(%peer, host, port, sni, "egress denied: the TLS server name is not the host the tunnel was opened to");
                    return;
                }
                Ok(Err(reason)) => {
                    tracing::warn!(%peer, host, port, reason, "egress denied: not TLS to the allowed host");
                    return;
                }
                Err(_) => {
                    tracing::warn!(%peer, host, port, "egress denied: no ClientHello in time");
                    return;
                }
            }
        }
        Some(head) => {
            if upstream.write_all(head).await.is_err() {
                return;
            }
            rest
        }
    };
    if !rest.is_empty() && upstream.write_all(&rest).await.is_err() {
        return;
    }
    tracing::info!(%peer, host, port, tunnel = upstream_head.is_none(), "egress allowed");
    match tokio::time::timeout(MAX_LIFETIME, copy_bidirectional(&mut client, &mut upstream)).await {
        Ok(Ok((up, down))) => tracing::debug!(%peer, host, port, up, down, "egress closed"),
        Ok(Err(err)) => tracing::debug!(%peer, host, port, error = %err, "egress ended"),
        Err(_) => tracing::warn!(%peer, host, port, "egress cut: open longer than an hour"),
    }
}

/// The connections each client address holds; a [`PeerSlot`] gives one back.
#[derive(Default)]
struct Peers(std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, usize>>);

struct PeerSlot {
    peers: Arc<Peers>,
    ip: std::net::IpAddr,
}

impl Peers {
    fn take(self: &Arc<Self>, ip: std::net::IpAddr) -> Option<PeerSlot> {
        let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let n = held.entry(ip).or_default();
        if *n >= MAX_PER_PEER {
            return None;
        }
        *n += 1;
        Some(PeerSlot {
            peers: self.clone(),
            ip,
        })
    }
}

impl Drop for PeerSlot {
    fn drop(&mut self) {
        let mut held = self.peers.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = held.get_mut(&self.ip) {
            *n -= 1;
            if *n == 0 {
                held.remove(&self.ip);
            }
        }
    }
}

/// Connects to each address `host` resolves to in turn, each given
/// [`CONNECT_TIMEOUT`]: an address family the network drops (IPv6 under an
/// IPv4-only policy) costs one timeout, not the connection.
async fn connect(host: &str, port: u16) -> Result<TcpStream, String> {
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("cannot resolve: {}", e.kind()))?;
    let mut last = "no address".to_owned();
    for addr in addrs {
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(e)) => last = format!("{addr}: {}", e.kind()),
            Err(_) => last = format!("{addr}: timed out"),
        }
    }
    Err(last)
}

/// Serves `listener` until `shutdown`, then stops accepting and lets the
/// connections open finish for up to `drain` (a model's answer streaming
/// through is not cut by a rollout that leaves it time).
pub async fn serve(
    listener: TcpListener,
    allow: Arc<AllowList>,
    shutdown: CancellationToken,
    drain: Duration,
) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let peers = Arc::new(Peers::default());
    loop {
        let (mut client, peer) = tokio::select! {
            _ = shutdown.cancelled() => {
                drop(listener);
                let open = MAX_CONNECTIONS - slots.available_permits();
                tracing::info!(open, "egress proxy draining");
                let all = u32::try_from(MAX_CONNECTIONS).unwrap_or(u32::MAX);
                if tokio::time::timeout(drain, slots.acquire_many(all)).await.is_err() {
                    let open = MAX_CONNECTIONS - slots.available_permits();
                    tracing::warn!(open, "egress proxy: connections still open after the drain");
                }
                return;
            }
            accepted = listener.accept() => match accepted {
                Ok(pair) => pair,
                Err(err) => {
                    tracing::warn!(error = %err, "egress proxy: accept failed");
                    continue;
                }
            },
        };
        let (Ok(slot), Some(peer_slot)) =
            (slots.clone().try_acquire_owned(), peers.take(peer.ip()))
        else {
            tracing::warn!(%peer, "egress refused: too many connections");
            tokio::spawn(async move {
                answer(
                    &mut client,
                    "503 Service Unavailable",
                    "too many connections",
                )
                .await;
            });
            continue;
        };
        let allow = allow.clone();
        tokio::spawn(async move {
            handle(client, peer, &allow).await;
            drop((slot, peer_slot));
        });
    }
}

/// `nexc egress-proxy`: binds `listen` and serves until SIGTERM or Ctrl-C,
/// then drains for up to `drain`.
pub async fn run(listen: SocketAddr, allow: AllowList, drain: Duration) -> anyhow::Result<()> {
    let listener = TcpListener::bind(listen).await?;
    tracing::info!(addr = %listen, allow = %allow.describe(), "egress proxy listening");
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    tokio::spawn(async move {
        crate::app::shutdown_signal().await;
        stop.cancel();
    });
    serve(listener, Arc::new(allow), shutdown, drain).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allow_list_names_hosts_and_ports_exactly() {
        let allow = AllowList::parse(
            " api.anthropic.com, api.openai.com:443 ,*.openai.azure.com:443, \
             host.docker.internal:11434,[fd00::1]:8000, 10.0.0.7:8080",
        )
        .unwrap();
        assert!(allow.allows("api.anthropic.com", 443));
        assert!(allow.allows("API.Anthropic.com.", 443));
        assert!(
            !allow.allows("api.anthropic.com", 80),
            "443 only unless named"
        );
        assert!(!allow.allows("evil-api.anthropic.com", 443));
        assert!(!allow.allows("api.anthropic.com.evil.example", 443));
        assert!(allow.allows("eu.openai.azure.com", 443));
        assert!(
            !allow.allows("openai.azure.com", 443),
            "a wildcard is subdomains only"
        );
        assert!(!allow.allows("xopenai.azure.com", 443));
        assert!(allow.allows("host.docker.internal", 11434));
        assert!(!allow.allows("host.docker.internal", 22));
        assert!(allow.allows("fd00::1", 8000));
        assert!(allow.allows("10.0.0.7", 8080));
        assert!(!allow.allows("169.254.169.254", 80));
        assert!(AllowList::parse("").unwrap().describe() == "nothing");
        assert!(
            !AllowList::parse("")
                .unwrap()
                .allows("api.anthropic.com", 443)
        );
        for bad in ["host:0", "host:http", "a*.example.com", "*", "[::1", "x y"] {
            assert!(AllowList::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(
            allow.describe(),
            "api.anthropic.com:443, api.openai.com:443, *.openai.azure.com:443, \
             host.docker.internal:11434, [fd00::1]:8000, 10.0.0.7:8080"
        );
    }

    #[test]
    fn requests_are_read_as_tunnels_or_rewritten_for_one_upstream() {
        assert_eq!(
            parse_head(b"CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: api.anthropic.com:443"),
            Ok(Request::Tunnel {
                host: "api.anthropic.com".into(),
                port: 443
            })
        );
        assert!(parse_head(b"CONNECT api.anthropic.com HTTP/1.1").is_err());
        assert_eq!(parse_head(b"GET /healthz HTTP/1.1"), Ok(Request::Health));
        let Ok(Request::Forward { host, port, head }) = parse_head(
            b"POST http://host.docker.internal:11434/v1/chat/completions?x=1 HTTP/1.1\r\n\
              Host: host.docker.internal:11434\r\nProxy-Connection: keep-alive\r\n\
              Connection: keep-alive\r\nProxy-Authorization: Basic eA==\r\nContent-Length: 2",
        ) else {
            panic!("a forward");
        };
        assert_eq!((host.as_str(), port), ("host.docker.internal", 11434));
        assert_eq!(
            String::from_utf8(head).unwrap(),
            "POST /v1/chat/completions?x=1 HTTP/1.1\r\nHost: host.docker.internal:11434\r\n\
             Content-Length: 2\r\nConnection: close\r\n\r\n"
        );
        // The client's own Host never reaches the upstream: the admitted authority does.
        let Ok(Request::Forward { head, .. }) =
            parse_head(b"GET http://10.0.0.7:8080/ HTTP/1.1\r\nHost: admin.internal\r\nhost: x")
        else {
            panic!("a forward");
        };
        assert_eq!(
            String::from_utf8(head).unwrap(),
            "GET / HTTP/1.1\r\nHost: 10.0.0.7:8080\r\nConnection: close\r\n\r\n"
        );
        let Ok(Request::Forward { head, port, .. }) = parse_head(b"GET http://ollama HTTP/1.1")
        else {
            panic!("a forward");
        };
        assert_eq!(port, 80);
        assert_eq!(
            String::from_utf8(head).unwrap(),
            "GET / HTTP/1.1\r\nHost: ollama\r\nConnection: close\r\n\r\n"
        );
        for bad in [
            &b"GET https://api.anthropic.com/ HTTP/1.1"[..],
            b"GET /v1/models HTTP/1.1",
            b"GET http://user:pw@host/ HTTP/1.1",
            b"GET http://host/ HTTP/2",
            b"GET http://host/ HTTP/1.1 extra",
            b"\xff\xfe",
        ] {
            assert!(parse_head(bad).is_err(), "{bad:?}");
        }
    }

    /// A minimal TLS 1.3 ClientHello, with a `supported_versions` extension
    /// before the `server_name` one.
    fn client_hello(sni: Option<&str>) -> Vec<u8> {
        let mut body = vec![0x03, 0x03];
        body.extend([7u8; 32]);
        body.push(0);
        body.extend([0x00, 0x02, 0x13, 0x01, 0x01, 0x00]);
        let mut ext = vec![0x00, 0x2b, 0x00, 0x03, 0x02, 0x03, 0x04];
        if let Some(name) = sni {
            let mut entry = vec![0u8];
            entry.extend((name.len() as u16).to_be_bytes());
            entry.extend(name.as_bytes());
            let mut list = (entry.len() as u16).to_be_bytes().to_vec();
            list.extend(entry);
            ext.extend([0x00, 0x00]);
            ext.extend((list.len() as u16).to_be_bytes());
            ext.extend(list);
        }
        body.extend((ext.len() as u16).to_be_bytes());
        body.extend(ext);
        let mut handshake = vec![0x01];
        handshake.extend(&(body.len() as u32).to_be_bytes()[1..]);
        handshake.extend(body);
        let mut record = vec![0x16, 0x03, 0x01];
        record.extend((handshake.len() as u16).to_be_bytes());
        record.extend(handshake);
        record
    }

    #[test]
    fn a_tunnel_must_carry_tls_for_the_host_it_was_opened_to() {
        let hello = client_hello(Some("api.anthropic.com"));
        assert_eq!(
            client_hello_sni(&hello),
            Ok(Ok(Some("api.anthropic.com".into())))
        );
        assert_eq!(client_hello_sni(&client_hello(None)), Ok(Ok(None)));
        // Partial: more bytes are needed, never a verdict.
        for cut in [0, 3, 5, 40, hello.len() - 1] {
            assert!(
                matches!(client_hello_sni(&hello[..cut]), Ok(Err(n)) if n > 0),
                "{cut}"
            );
        }
        // The same hello split over two records.
        let handshake = &hello[5..];
        let (a, b) = handshake.split_at(20);
        let mut split = vec![0x16, 0x03, 0x01, 0x00, a.len() as u8];
        split.extend(a);
        split.extend([0x16, 0x03, 0x01]);
        split.extend((b.len() as u16).to_be_bytes());
        split.extend(b);
        assert_eq!(
            client_hello_sni(&split),
            Ok(Ok(Some("api.anthropic.com".into())))
        );
        // Not TLS, or not a ClientHello first.
        assert!(client_hello_sni(b"GET / HTTP/1.1\r\n\r\n").is_err());
        let mut server_hello = hello.clone();
        server_hello[5] = 0x02;
        assert!(client_hello_sni(&server_hello).is_err());
        // A truncated extension block is malformed, not a missing name.
        let mut broken = hello.clone();
        let len = broken.len() - 3;
        broken.truncate(len);
        broken[3..5].copy_from_slice(&((len - 5) as u16).to_be_bytes());
        broken[6..9].copy_from_slice(&((len - 9) as u32).to_be_bytes()[1..]);
        assert!(client_hello_sni(&broken).is_err());

        assert!(sni_matches("api.anthropic.com", Some("API.Anthropic.com.")));
        assert!(!sni_matches("api.anthropic.com", Some("attacker.example")));
        assert!(
            !sni_matches("api.anthropic.com", None),
            "a name needs its SNI"
        );
        assert!(
            sni_matches("10.0.0.7", None),
            "an address written as an IP needs none"
        );
        assert!(!sni_matches("10.0.0.7", Some("attacker.example")));
    }

    /// An upstream that answers every connection with what it read, once.
    async fn echo_upstream() -> (SocketAddr, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = s.read(&mut buf).await.unwrap();
            buf.truncate(n);
            s.write_all(b"echo:").await.unwrap();
            s.write_all(&buf).await.unwrap();
            s.shutdown().await.unwrap();
            buf
        });
        (addr, task)
    }

    async fn proxy(allow: &str) -> (SocketAddr, CancellationToken) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        tokio::spawn(serve(
            listener,
            Arc::new(AllowList::parse(allow).unwrap()),
            shutdown.clone(),
            Duration::from_secs(1),
        ));
        (addr, shutdown)
    }

    async fn exchange(proxy: SocketAddr, request: &[u8]) -> String {
        let mut s = TcpStream::connect(proxy).await.unwrap();
        s.write_all(request).await.unwrap();
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out)).await;
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn one_client_cannot_take_every_slot() {
        let peers = Arc::new(Peers::default());
        let (a, b): (std::net::IpAddr, std::net::IpAddr) =
            ("10.0.0.1".parse().unwrap(), "10.0.0.2".parse().unwrap());
        let held: Vec<PeerSlot> = (0..MAX_PER_PEER).map(|_| peers.take(a).unwrap()).collect();
        assert!(peers.take(a).is_none(), "the cap per client");
        assert!(peers.take(b).is_some(), "another client is served");
        drop(held);
        assert!(peers.take(a).is_some(), "slots come back");
        assert_eq!(
            peers.0.lock().unwrap().len(),
            0,
            "a client holding nothing is forgotten"
        );
    }

    #[tokio::test]
    async fn it_tunnels_and_forwards_to_allowed_hosts_and_refuses_the_rest() {
        let (upstream, seen) = echo_upstream().await;
        let (addr, shutdown) = proxy(&format!("127.0.0.1:{}", upstream.port())).await;
        // A tunnel: the proxy's answer, then the upstream's bytes. The address is an IP, so
        // the hello may name nothing.
        let hello = client_hello(None);
        let mut request =
            format!("CONNECT 127.0.0.1:{} HTTP/1.1\r\n\r\n", upstream.port()).into_bytes();
        request.extend(&hello);
        let out = exchange(addr, &request).await;
        assert!(
            out.starts_with("HTTP/1.1 200 Connection Established\r\n\r\necho:"),
            "{out}"
        );
        assert_eq!(seen.await.unwrap(), hello);
        // A hello naming another site behind the allowed address goes nowhere.
        let (upstream, seen) = echo_upstream().await;
        let (fronted, stop) = proxy(&format!("127.0.0.1:{}", upstream.port())).await;
        let mut request =
            format!("CONNECT 127.0.0.1:{} HTTP/1.1\r\n\r\n", upstream.port()).into_bytes();
        request.extend(client_hello(Some("attacker.example")));
        let out = exchange(fronted, &request).await;
        assert_eq!(out, "HTTP/1.1 200 Connection Established\r\n\r\n");
        assert!(
            seen.await.unwrap().is_empty(),
            "nothing reached the upstream"
        );
        // Nor does a tunnel that does not carry TLS.
        let (upstream, seen) = echo_upstream().await;
        let (plain, stop2) = proxy(&format!("127.0.0.1:{}", upstream.port())).await;
        let request = format!(
            "CONNECT 127.0.0.1:{} HTTP/1.1\r\n\r\nGET / HTTP/1.1\r\n\r\n",
            upstream.port()
        );
        let out = exchange(plain, request.as_bytes()).await;
        assert_eq!(out, "HTTP/1.1 200 Connection Established\r\n\r\n");
        assert!(seen.await.unwrap().is_empty());
        stop.cancel();
        stop2.cancel();
        // A plain-HTTP forward: the upstream sees origin form and `Connection: close`.
        let (upstream, seen) = echo_upstream().await;
        let (addr2, shutdown2) = proxy(&format!("127.0.0.1:{}", upstream.port())).await;
        let out = exchange(
            addr2,
            format!(
                "GET http://127.0.0.1:{}/v1/models HTTP/1.1\r\nConnection: keep-alive\r\n\r\n",
                upstream.port()
            )
            .as_bytes(),
        )
        .await;
        let received = String::from_utf8(seen.await.unwrap()).unwrap();
        assert!(
            received.starts_with("GET /v1/models HTTP/1.1\r\n"),
            "{received}"
        );
        assert!(
            received.ends_with("Connection: close\r\n\r\n"),
            "{received}"
        );
        assert!(!received.contains("keep-alive"), "{received}");
        assert!(out.starts_with("echo:GET /v1/models"), "{out}");
        // Anything else is refused before a connection is opened.
        for request in [
            "CONNECT example.com:443 HTTP/1.1\r\n\r\n".to_owned(),
            "CONNECT 127.0.0.1:1 HTTP/1.1\r\n\r\n".to_owned(),
            "GET http://169.254.169.254/latest/meta-data HTTP/1.1\r\n\r\n".to_owned(),
        ] {
            let out = exchange(addr, request.as_bytes()).await;
            assert!(
                out.starts_with("HTTP/1.1 403 Forbidden"),
                "{request}: {out}"
            );
        }
        let out = exchange(addr, b"GET /v1/models HTTP/1.1\r\n\r\n").await;
        assert!(out.starts_with("HTTP/1.1 400 Bad Request"), "{out}");
        let out = exchange(addr, b"GET /healthz HTTP/1.1\r\n\r\n").await;
        assert!(
            out.starts_with("HTTP/1.1 200 OK") && out.ends_with("ok"),
            "{out}"
        );
        shutdown.cancel();
        shutdown2.cancel();
    }
}
