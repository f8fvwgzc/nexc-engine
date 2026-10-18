//! The built-in `fetch_url` tool: an HTTP GET the backend makes for an
//! agent, under the policy's host lists, with every redirect's host checked
//! again, the response bounded in size and limited to text-like content.

use std::net::IpAddr;

use futures::StreamExt;

/// Redirects followed at most.
const HOPS_MAX: usize = 5;

/// What came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub status: u16,
    pub content_type: String,
    pub body: String,
    /// Bytes of the body on the wire.
    pub bytes: usize,
    /// The host the body came from (after redirects).
    pub host: String,
    pub final_url: String,
    pub hops: usize,
}

/// Why nothing came back.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    #[error("{0}")]
    BadUrl(String),
    /// A host the policy refused (the rule and the reason).
    #[error("{reason}")]
    Refused { rule: &'static str, reason: String },
    #[error("the request failed: {0}")]
    Transport(String),
    #[error("the response is larger than {0} bytes")]
    Oversized(usize),
    #[error(
        "the response is {0}, which the gateway does not pass on (text, JSON, XML and HTML only)"
    )]
    ContentType(String),
    #[error("more than {HOPS_MAX} redirects")]
    TooManyRedirects,
}

/// Whether a content type is passed on to the model.
pub fn is_text_like(content_type: &str) -> bool {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    mime.starts_with("text/")
        || matches!(
            mime.as_str(),
            "application/json"
                | "application/xml"
                | "application/x-ndjson"
                | "application/xhtml+xml"
                | "application/ld+json"
                | "application/javascript"
                | "application/x-www-form-urlencoded"
        )
        || mime.ends_with("+json")
        || mime.ends_with("+xml")
}

/// Whether `host` names this machine or a private network: such a host is
/// reached only when the allow-list names it exactly.
pub fn is_private_host(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if bare.eq_ignore_ascii_case("localhost") || bare.ends_with(".localhost") {
        return true;
    }
    match bare.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || (ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1]))
        }
        Ok(IpAddr::V6(ip)) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|v4| is_private_host(&v4.to_string()))
        }
        Err(_) => false,
    }
}

/// The host of an http(s) URL.
pub fn host_of(url: &str) -> Result<(reqwest::Url, String), FetchError> {
    let parsed = reqwest::Url::parse(url).map_err(|e| FetchError::BadUrl(e.to_string()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(FetchError::BadUrl(
            "only http and https URLs are fetched".into(),
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(FetchError::BadUrl(
            "credentials in a URL are not sent".into(),
        ));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| FetchError::BadUrl("the URL names no host".into()))?
        .to_ascii_lowercase();
    Ok((parsed, host))
}

/// Decides whether a host may be reached: `Ok`, or the rule and the reason.
pub type HostCheck<'a> = &'a (dyn Fn(&str) -> Result<(), (&'static str, String)> + Sync);

/// Fetches `url` with `client` (which must not follow redirects itself):
/// `allow(host)` decides every host on the way, the body is read up to
/// `max_bytes`, and only text-like content is passed on.
pub async fn fetch(
    client: &reqwest::Client,
    url: &str,
    max_bytes: usize,
    allow: HostCheck<'_>,
) -> Result<Fetched, FetchError> {
    let (mut current, mut host) = host_of(url)?;
    let mut hops = 0;
    loop {
        allow(&host).map_err(|(rule, reason)| FetchError::Refused { rule, reason })?;
        let response = client
            .get(current.clone())
            .header(
                "accept",
                "text/html, application/json, application/xml, text/*;q=0.9",
            )
            .send()
            .await
            .map_err(|e| FetchError::Transport(e.without_url().to_string()))?;
        let status = response.status();
        if status.is_redirection() {
            let Some(location) = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
            else {
                return Err(FetchError::Transport(format!(
                    "HTTP {status} without a location"
                )));
            };
            hops += 1;
            if hops > HOPS_MAX {
                return Err(FetchError::TooManyRedirects);
            }
            let next = current
                .join(location)
                .map_err(|e| FetchError::BadUrl(format!("redirect: {e}")))?;
            let (next, next_host) = host_of(next.as_str())?;
            current = next;
            host = next_host;
            continue;
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_owned();
        if !is_text_like(&content_type) {
            return Err(FetchError::ContentType(
                content_type
                    .split(';')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
            ));
        }
        if let Some(length) = response.content_length()
            && length > max_bytes as u64
        {
            return Err(FetchError::Oversized(max_bytes));
        }
        let mut stream = response.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| FetchError::Transport(e.without_url().to_string()))?;
            buf.extend_from_slice(&chunk);
            if buf.len() > max_bytes {
                return Err(FetchError::Oversized(max_bytes));
            }
        }
        let bytes = buf.len();
        return Ok(Fetched {
            status: status.as_u16(),
            content_type,
            body: String::from_utf8_lossy(&buf).into_owned(),
            bytes,
            host,
            final_url: current.to_string(),
            hops,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_and_content_types_are_classified() {
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("localhost"));
        assert!(is_private_host("10.1.2.3"));
        assert!(is_private_host("169.254.169.254"));
        assert!(is_private_host("100.64.0.1"));
        assert!(is_private_host("[::1]"));
        assert!(is_private_host("fd00::1"));
        assert!(!is_private_host("api.example.com"));
        assert!(!is_private_host("8.8.8.8"));
        assert!(is_text_like("text/html; charset=utf-8"));
        assert!(is_text_like("application/json"));
        assert!(is_text_like("application/problem+json"));
        assert!(!is_text_like("image/png"));
        assert!(!is_text_like("application/octet-stream"));
        assert!(host_of("ftp://x").is_err());
        assert!(host_of("https://user:pw@example.com/").is_err());
        assert_eq!(host_of("https://Example.com/a").unwrap().1, "example.com");
    }
}
