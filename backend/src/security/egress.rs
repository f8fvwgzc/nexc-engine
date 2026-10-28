//! Outbound connections whose target a user chooses: an agent's
//! `fetch_url`, a workspace's MCP servers, alert destinations and A2A
//! counterparts, LLM and embeddings base URLs, a transfer's Redis. The SSRF
//! guard.
//!
//! The rule runs on the resolved addresses, in the HTTP client's own DNS
//! resolver, right before connecting, so a name that changes its answer
//! after a check cannot slip through; IP literals (which the client does
//! not resolve) are checked before the request is built. Loopback, private,
//! link-local, CGNAT, multicast, unspecified, broadcast, documentation and
//! reserved ranges are refused, IPv4 and IPv6 (an IPv4-mapped,
//! IPv4-compatible, NAT64 or 6to4 address is judged by the IPv4 address
//! inside), unless the operator lists the range in
//! `NEXC_EGRESS_ALLOW_PRIVATE`. Link-local addresses (where the cloud
//! metadata services answer) and the other metadata addresses are refused
//! whatever is listed. A guarded client uses no proxy and follows no
//! redirect of its own.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// The setting that lists the private ranges the operator lets users reach.
pub const ALLOW_VAR: &str = "NEXC_EGRESS_ALLOW_PRIVATE";

/// An address range: `10.0.0.0/8`, `fd00::/8`, or one address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    net: IpAddr,
    prefix: u8,
}

impl Cidr {
    /// Whether `ip` is in the range (an IPv4-mapped IPv6 address counts as its IPv4 address).
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.net, canonical(ip)) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

impl std::str::FromStr for Cidr {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let raw = raw.trim();
        let (addr, prefix) = match raw.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (raw, None),
        };
        let net: IpAddr = addr
            .trim_matches(['[', ']'])
            .parse()
            .map_err(|_| format!("`{raw}` is not an address or a range"))?;
        let max = if net.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            None => max,
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= max)
                .ok_or_else(|| format!("`{raw}`: the prefix must be 0-{max}"))?,
        };
        Ok(Cidr { net, prefix })
    }
}

impl std::fmt::Display for Cidr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.net, self.prefix)
    }
}

/// An IPv4-mapped IPv6 address as its IPv4 address; anything else as is.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || a == 0                                   // 0.0.0.0/8 "this network"
        || (a == 100 && (64..128).contains(&b))     // 100.64.0.0/10 CGNAT
        || (a == 192 && b == 0 && c == 0)           // 192.0.0.0/24 protocol assignments
        || (a == 198 && (b == 18 || b == 19))       // 198.18.0.0/15 benchmarking
        || a >= 240) // 240.0.0.0/4 reserved
}

/// The IPv4 address an IPv6 address carries and is routed by: IPv4-mapped,
/// IPv4-compatible, NAT64 (well-known and local-use prefixes) and 6to4.
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return Some(v4);
    }
    let s = ip.segments();
    let v4 = |hi: u16, lo: u16| Ipv4Addr::from((u32::from(hi) << 16) | u32::from(lo));
    match s {
        // ::a.b.c.d, IPv4-compatible (deprecated); `::` and `::1` are judged as IPv6.
        [0, 0, 0, 0, 0, 0, hi, lo] if hi != 0 => Some(v4(hi, lo)),
        // 64:ff9b::/96 and 64:ff9b:1::/48 (NAT64)
        [0x64, 0xff9b, 0, 0, 0, 0, hi, lo] | [0x64, 0xff9b, 1, _, _, _, hi, lo] => Some(v4(hi, lo)),
        // 2002:aabb:ccdd::/48 (6to4)
        [0x2002, hi, lo, ..] => Some(v4(hi, lo)),
        _ => None,
    }
}

fn public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = embedded_v4(ip) {
        return public_v4(v4);
    }
    let s = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00                // fc00::/7 unique local
        || (s[0] & 0xffc0) == 0xfe80                // fe80::/10 link-local
        || (s[0] & 0xffc0) == 0xfec0                // fec0::/10 site-local (deprecated)
        || (s[0] == 0x2001 && s[1] == 0x0db8)       // 2001:db8::/32 documentation
        || (s[0] == 0x2001 && s[1] == 0)            // 2001::/32 Teredo (the address inside is hidden)
        || (s[0] == 0x100 && s[1..4] == [0, 0, 0])  // 100::/64 discard
        || s[..6] == [0, 0, 0, 0, 0, 0]) // the rest of ::/96 (IPv4-compatible, deprecated)
}

/// Whether the address is on the public internet.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => public_v6(v6),
    }
}

/// Addresses no listed range opens: link-local (where AWS, GCP and Azure
/// answer metadata requests), the other clouds' metadata addresses, and the
/// unspecified and broadcast addresses.
fn never(ip: IpAddr) -> bool {
    let judged = match ip {
        IpAddr::V6(v6) => embedded_v4(v6).map_or(ip, IpAddr::V4),
        v4 => v4,
    };
    match judged {
        IpAddr::V4(v4) => {
            v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4 == Ipv4Addr::new(100, 100, 100, 200) // Alibaba Cloud
        }
        IpAddr::V6(v6) => {
            (v6.segments()[0] & 0xffc0) == 0xfe80
                || v6.is_unspecified()
                || v6 == Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254) // AWS over IPv6
        }
    }
}

/// The SSRF guard: which addresses a user-chosen target may resolve to.
/// Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct EgressGuard {
    allowed: Arc<[Cidr]>,
}

impl EgressGuard {
    /// A guard that also lets users reach `allowed` (the operator's `NEXC_EGRESS_ALLOW_PRIVATE`).
    pub fn new(allowed: Vec<Cidr>) -> Self {
        EgressGuard {
            allowed: allowed.into(),
        }
    }

    /// The private ranges the operator opened.
    pub fn allowed(&self) -> &[Cidr] {
        &self.allowed
    }

    /// `Ok` when `ip` may be connected to; else why not.
    pub fn check_ip(&self, ip: IpAddr) -> Result<(), String> {
        if never(ip) {
            return Err(format!(
                "{ip} is a link-local or cloud metadata address, which is never reached"
            ));
        }
        if public_ip(ip) || self.allowed.iter().any(|range| range.contains(ip)) {
            return Ok(());
        }
        Err(format!(
            "{ip} is on this machine or a private network, which is not reached ({ALLOW_VAR} lists the ranges an operator opens)"
        ))
    }

    /// The host of a URL, checked as far as it can be without resolving it:
    /// an IP literal is judged as is, and `localhost` as the loopback
    /// address. A name is judged again, by its addresses, when connecting.
    pub fn check_host(&self, host: &str) -> Result<(), String> {
        let bare = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = bare.parse::<IpAddr>() {
            return self.check_ip(ip).map_err(|why| format!("{host}: {why}"));
        }
        let lower = bare.trim_end_matches('.').to_ascii_lowercase();
        if lower == "localhost" || lower.ends_with(".localhost") {
            return self
                .check_ip(IpAddr::V4(Ipv4Addr::LOCALHOST))
                .map_err(|why| format!("{host}: {why}"));
        }
        Ok(())
    }

    /// [`EgressGuard::check_host`] of an http(s) URL.
    pub fn check_url(&self, url: &str) -> Result<(), String> {
        let parsed = reqwest::Url::parse(url).map_err(|e| format!("{url}: {e}"))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| format!("{url} names no host"))?;
        self.check_host(host)
    }

    /// Resolves `host` (an IP literal is taken as is) and keeps the
    /// addresses only when every one of them may be reached: what a raw
    /// connection (not through a guarded client) connects to.
    pub async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
        let bare = host.trim_start_matches('[').trim_end_matches(']');
        let addrs: Vec<SocketAddr> = match bare.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, port)],
            Err(_) => tokio::net::lookup_host((bare, port))
                .await
                .map_err(|e| format!("{host} does not resolve: {e}"))?
                .collect(),
        };
        self.only_allowed(host, addrs)
    }

    fn only_allowed(&self, host: &str, addrs: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, String> {
        if addrs.is_empty() {
            return Err(format!("{host} has no addresses"));
        }
        for addr in &addrs {
            self.check_ip(addr.ip())
                .map_err(|why| format!("{host} resolves to {}: {why}", addr.ip()))?;
        }
        Ok(addrs)
    }

    /// `builder` with this guard as its DNS resolver, no proxy (which would
    /// resolve the name itself) and no redirects of its own.
    pub fn client(&self, builder: reqwest::ClientBuilder) -> reqwest::Result<reqwest::Client> {
        builder
            .dns_resolver(Resolver(self.clone()))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
    }
}

/// The DNS resolver of a guarded client: answers only when every address
/// of the name may be reached.
struct Resolver(EgressGuard);

impl Resolve for Resolver {
    fn resolve(&self, name: Name) -> Resolving {
        let guard = self.0.clone();
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let addrs = guard.only_allowed(&host, addrs).map_err(|why| {
                tracing::warn!(host = %host, reason = %why, "outbound connection refused");
                format!("refused: {why}")
            })?;
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

/// Parses `NEXC_EGRESS_ALLOW_PRIVATE`: ranges separated by commas.
pub fn parse_allowed(raw: &str) -> Result<Vec<Cidr>, String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::parse)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn only_public_addresses_are_reached() {
        let guard = EgressGuard::default();
        for bad in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "169.254.170.2",
            "100.64.0.1",
            "100.100.100.200",
            "224.0.0.1",
            "0.0.0.0",
            "0.1.2.3",
            "255.255.255.255",
            "198.18.0.1",
            "240.0.0.1",
            "::1",
            "::",
            "fd00::1",
            "fd00:ec2::254",
            "fe80::1",
            "fec0::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "::ffff:10.0.0.1",
            "::127.0.0.1",
            "::5",
            "64:ff9b::a9fe:a9fe",
            "64:ff9b:1::a00:1",
            "2002:7f00:1::1",
            "2002:a9fe:a9fe::1",
            "2001:0:4136:e378:8000:63bf:3fff:fdd2",
            "2001:db8::1",
        ] {
            assert!(guard.check_ip(ip(bad)).is_err(), "{bad}");
        }
        for good in [
            "8.8.8.8",
            "1.1.1.1",
            "100.128.0.1",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:808:808::1",
        ] {
            assert!(guard.check_ip(ip(good)).is_ok(), "{good}");
        }
    }

    #[test]
    fn the_operator_opens_ranges_but_never_the_metadata_service() {
        let guard = EgressGuard::new(
            parse_allowed("10.20.0.0/16, 127.0.0.1,::1/128,169.254.0.0/16,fd00::/8").unwrap(),
        );
        for ok in [
            "10.20.3.4",
            "127.0.0.1",
            "::ffff:127.0.0.1",
            "::1",
            "fd00::5",
        ] {
            assert!(guard.check_ip(ip(ok)).is_ok(), "{ok}");
        }
        for bad in [
            "10.21.0.1",
            "127.0.0.2",
            "169.254.169.254",
            "fe80::1",
            "fd00:ec2::254",
            "::ffff:169.254.169.254",
        ] {
            assert!(guard.check_ip(ip(bad)).is_err(), "{bad}");
        }
        assert!(parse_allowed("10.0.0.0/33").is_err());
        assert!(parse_allowed("not-a-range").is_err());
        assert_eq!(parse_allowed(" , ").unwrap(), vec![]);
        assert_eq!(
            parse_allowed("10.0.0.0/8").unwrap()[0].to_string(),
            "10.0.0.0/8"
        );
    }

    #[test]
    fn literal_hosts_are_judged_before_resolution() {
        let guard = EgressGuard::default();
        for bad in [
            "http://127.0.0.1:8080/x",
            "http://[::1]/",
            "http://169.254.169.254/latest/meta-data/",
            "http://2130706433/",
            "http://0x7f.1/",
            "http://localhost:9/",
            "http://api.localhost./",
            "http://[::ffff:7f00:1]/",
        ] {
            assert!(guard.check_url(bad).is_err(), "{bad}");
        }
        assert!(guard.check_url("https://api.example.com/v1").is_ok());
        assert!(guard.check_url("https://8.8.8.8/").is_ok());
    }

    #[tokio::test]
    async fn names_are_judged_by_what_they_resolve_to() {
        let guard = EgressGuard::default();
        let err = guard.resolve("localhost", 80).await.unwrap_err();
        assert!(err.contains("resolves to"), "{err}");
        let err = match Resolver(guard.clone())
            .resolve("localhost".parse().unwrap())
            .await
        {
            Ok(_) => panic!("localhost resolved"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("refused"), "{err}");
        assert!(guard.resolve("8.8.8.8", 53).await.is_ok());
        let open = EgressGuard::new(parse_allowed("127.0.0.0/8,::1").unwrap());
        assert!(open.resolve("127.0.0.1", 80).await.is_ok());
    }

    #[tokio::test]
    async fn a_guarded_client_does_not_connect_to_a_name_that_resolves_privately() {
        // A server on loopback that would answer, reached by a name rather than an address.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = tokio::spawn(async move { listener.accept().await.is_ok() });
        let client = EgressGuard::default()
            .client(reqwest::Client::builder())
            .unwrap();
        let err = client
            .get(format!("http://localhost:{port}/"))
            .send()
            .await
            .unwrap_err();
        assert!(err.is_connect(), "{err:?}");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!accepted.is_finished(), "nothing connected to the server");
        accepted.abort();
    }
}
