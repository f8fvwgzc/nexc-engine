//! `nexc egress-proxy`: the agent runtime's only way out of its network in
//! Docker Compose. A forward proxy that connects only to the hosts and ports
//! its allow-list names: `CONNECT host:port` for TLS (the LLM APIs), and
//! absolute-form plain HTTP (`GET http://host:port/…`) for a local provider
//! such as Ollama. Anything else is answered 403 and logged.
//!
//! It is not the tools gateway: it sees host names, ports and byte counts,
//! never what travels inside TLS, and it decides nothing per agent. It keeps
//! a compromised runtime from reaching the rest of the internet (CONTRACT §8,
//! docs/AGENTSECOPS.md, the runtime's egress).

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
/// How long connecting to an allowed host may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Connections served at once; one more is answered 503.
const MAX_CONNECTIONS: usize = 512;
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
    let mut out = format!("{method} {path} {version}\r\n");
    let mut has_host = false;
    for header in lines.take_while(|l| !l.is_empty()) {
        let name = header.split(':').next().unwrap_or_default().trim();
        if HOP_HEADERS.iter().any(|h| name.eq_ignore_ascii_case(h)) {
            continue;
        }
        has_host |= name.eq_ignore_ascii_case("host");
        out.push_str(header);
        out.push_str("\r\n");
    }
    if !has_host {
        out.push_str(&format!("Host: {authority}\r\n"));
    }
    out.push_str("Connection: close\r\n\r\n");
    Ok(Request::Forward {
        host,
        port,
        head: out.into_bytes(),
    })
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
            &format!("egress to {host}:{port} is not allowed (NEXC_EGRESS_ALLOW)"),
        )
        .await;
        return;
    }
    let connected =
        tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host.as_str(), port)))
            .await
            .map_err(|_| "timed out".to_owned())
            .and_then(|r| r.map_err(|e| e.kind().to_string()));
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
    tracing::info!(%peer, host, port, tunnel = upstream_head.is_none(), "egress allowed");
    let sent = match &upstream_head {
        None => client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .is_ok(),
        Some(head) => upstream.write_all(head).await.is_ok(),
    };
    if !sent || (!rest.is_empty() && upstream.write_all(&rest).await.is_err()) {
        return;
    }
    match copy_bidirectional(&mut client, &mut upstream).await {
        Ok((up, down)) => tracing::debug!(%peer, host, port, up, down, "egress closed"),
        Err(err) => tracing::debug!(%peer, host, port, error = %err, "egress ended"),
    }
}

/// Serves `listener` until `shutdown`.
pub async fn serve(listener: TcpListener, allow: Arc<AllowList>, shutdown: CancellationToken) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (mut client, peer) = tokio::select! {
            _ = shutdown.cancelled() => return,
            accepted = listener.accept() => match accepted {
                Ok(pair) => pair,
                Err(err) => {
                    tracing::warn!(error = %err, "egress proxy: accept failed");
                    continue;
                }
            },
        };
        let Ok(slot) = slots.clone().try_acquire_owned() else {
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
            drop(slot);
        });
    }
}

/// `nexc egress-proxy`: binds `listen` and serves until SIGTERM or Ctrl-C.
pub async fn run(listen: SocketAddr, allow: AllowList) -> anyhow::Result<()> {
    let listener = TcpListener::bind(listen).await?;
    tracing::info!(addr = %listen, allow = %allow.describe(), "egress proxy listening");
    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    tokio::spawn(async move {
        crate::app::shutdown_signal().await;
        stop.cancel();
    });
    serve(listener, Arc::new(allow), shutdown).await;
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

    #[tokio::test]
    async fn it_tunnels_and_forwards_to_allowed_hosts_and_refuses_the_rest() {
        let (upstream, seen) = echo_upstream().await;
        let (addr, shutdown) = proxy(&format!("127.0.0.1:{}", upstream.port())).await;
        // A tunnel: the proxy's answer, then the upstream's bytes.
        let out = exchange(
            addr,
            format!(
                "CONNECT 127.0.0.1:{} HTTP/1.1\r\n\r\nhello",
                upstream.port()
            )
            .as_bytes(),
        )
        .await;
        assert_eq!(out, "HTTP/1.1 200 Connection Established\r\n\r\necho:hello");
        assert_eq!(seen.await.unwrap(), b"hello");
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
