//! The application error type. The HTTP layer renders it as RFC 7807
//! `application/problem+json` (see `http::problem`).

use std::error::Error;

use super::guardrails::redact_secrets;
use super::validation::FieldErrors;

/// All a client reads of an [`AppError::Internal`], wherever it is answered.
pub const INTERNAL_DETAIL: &str = "An unexpected error occurred.";

/// Every failure a request can end with.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// 400 – malformed input that is not a field validation problem.
    #[error("{0}")]
    BadRequest(String),
    /// 401 – missing or invalid credentials. The message is always generic.
    #[error("{0}")]
    Unauthorized(&'static str),
    /// 403 – authenticated but not allowed.
    #[error("{0}")]
    Forbidden(String),
    /// 404 – unknown resource (also used for resources owned by someone else).
    #[error("{0} not found")]
    NotFound(&'static str),
    /// 409 – the request conflicts with current state (cycles, duplicates).
    #[error("{0}")]
    Conflict(String),
    /// 413 – request body too large.
    #[error("request body too large")]
    PayloadTooLarge,
    /// 415 – content of a kind this resource does not take (a binary file where text goes).
    #[error("{0}")]
    UnsupportedMediaType(String),
    /// 422 – field validation failed.
    #[error("validation failed")]
    Validation(FieldErrors),
    /// 422 – semantically invalid request (e.g. no LLM key configured).
    #[error("{0}")]
    Unprocessable(String),
    /// 429 – rate limited; retry after the given number of seconds.
    #[error("too many requests")]
    RateLimited { retry_after_secs: u64 },
    /// 503 – a service this request needs is off or not reachable (the audit's onto-intelli).
    #[error("{0}")]
    Unavailable(String),
    /// 500 – unexpected failure; details are logged, never returned.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    /// Shorthand for a single-field validation error.
    pub fn field(field: &str, message: impl Into<String>) -> Self {
        let mut errors = FieldErrors::default();
        errors.add(field, message);
        AppError::Validation(errors)
    }
}

/// An error and the causes under it on one line, for a log, a stored failure
/// or an answer that shows why something failed. `{e}` prints the outermost
/// message alone, and `{e:#}` walks the chain of an `anyhow::Error` only.
///
/// Pass an `anyhow::Error` as `err.as_ref()` and anything else as `&err`.
///
/// A cause the text already quotes is not repeated (sqlx's and tonic's
/// messages quote theirs). What the walk reaches may carry data, so it is
/// cut on the way: a serde_json error says what was wrong and where, not the
/// value it quotes; a URL keeps its host and path, not its credentials,
/// query or fragment; tokens of a well-known shape are redacted. A database
/// error's `DETAIL`, which quotes row values, is only in its `Debug`: never
/// log a driver's error with `{e:?}`.
pub fn error_chain(err: &(dyn Error + 'static)) -> String {
    let mut text = message(err);
    let mut cause = err.source();
    while let Some(inner) = cause {
        let more = message(inner);
        if !text.contains(&more) {
            text.push_str(": ");
            text.push_str(&more);
        }
        cause = inner.source();
    }
    redact_secrets(&without_url_secrets(&text)).0
}

/// One layer of the chain. serde_json quotes the value it refused (a
/// document's text, a token), so its words stop at the first quote.
fn message(err: &(dyn Error + 'static)) -> String {
    let Some(json) = err.downcast_ref::<serde_json::Error>() else {
        return err.to_string();
    };
    let text = json.to_string();
    let kind = text.split(['"', '`']).next().unwrap_or_default();
    let kind = kind.split(" at line ").next().unwrap_or_default().trim();
    match json.line() {
        0 => kind.to_owned(),
        line => format!("{kind} at line {line} column {}", json.column()),
    }
}

/// `text` with every URL in it cut down to its host and path. The last `@`
/// counts, as a password may hold one.
fn without_url_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (before, after) = rest.split_at(at + "://".len());
        out.push_str(before);
        let end = after
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>'))
            .unwrap_or(after.len());
        let (url, tail) = after.split_at(end);
        // The sentence's own punctuation, as in `for url (http://host/):`.
        let bare = url.trim_end_matches([')', ']', ',', ';', ':', '.']);
        let place = bare.rsplit_once('@').map_or(bare, |(_, place)| place);
        out.push_str(place.split(['?', '#']).next().unwrap_or(place));
        out.push_str(&url[bare.len()..]);
        rest = tail;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use anyhow::Context as _;

    use super::*;

    /// Quotes its source, as sqlx's errors do.
    #[derive(Debug, thiserror::Error)]
    #[error("driver failed: {0}")]
    struct Quoting(#[source] std::io::Error);

    #[derive(Debug, thiserror::Error)]
    #[error("the job failed")]
    struct Plain(#[source] std::io::Error);

    #[test]
    fn walks_a_typed_error_and_names_each_cause_once() {
        let refused = || std::io::Error::other("connection refused");
        assert_eq!(
            error_chain(&Plain(refused())),
            "the job failed: connection refused"
        );
        assert_eq!(
            error_chain(&Quoting(refused())),
            "driver failed: connection refused"
        );
    }

    #[test]
    fn a_context_reads_as_the_message_it_replaced() {
        let failed = || {
            Err::<(), _>(std::io::Error::other("disk full"))
                .context("cannot store the upload")
                .unwrap_err()
        };
        assert_eq!(failed().to_string(), "cannot store the upload");
        assert_eq!(
            error_chain(failed().as_ref()),
            "cannot store the upload: disk full"
        );
        // Through `AppError`, the way a handler's failure reaches its reporter.
        assert_eq!(
            error_chain(&AppError::from(failed())),
            "cannot store the upload: disk full"
        );
    }

    #[test]
    fn a_value_serde_json_quotes_stays_out() {
        let refused = serde_json::from_str::<u32>(r#""the text of a customer's contract""#)
            .context("the parsed document does not fit")
            .unwrap_err();
        assert_eq!(
            error_chain(refused.as_ref()),
            "the parsed document does not fit: invalid type: string at line 1 column 35"
        );

        // The other way serde quotes: between backticks.
        let negative = serde_json::from_str::<u32>("-1234").unwrap_err();
        assert_eq!(
            error_chain(&negative),
            "invalid value: integer at line 1 column 5"
        );

        // Without a position (a value, not a text).
        let value = serde_json::from_value::<u32>(serde_json::json!("customer-value"));
        assert_eq!(error_chain(&value.unwrap_err()), "invalid type: string");
    }

    #[test]
    fn a_url_keeps_its_host_and_path_only() {
        for (text, expected) in [
            (
                "error sending request for url (https://u:s3cret@api.example.com/v1?key=abc#f): refused",
                "error sending request for url (https://api.example.com/v1): refused",
            ),
            (
                "postgres://u:p@ss@db.example.com:5432/acme?sslmode=require is down",
                "postgres://db.example.com:5432/acme is down",
            ),
            (
                "from \"redis://:s3cret@cache:6379/0\", then http://plain.example.com/a.",
                "from \"redis://cache:6379/0\", then http://plain.example.com/a.",
            ),
            ("no address at all", "no address at all"),
        ] {
            assert_eq!(without_url_secrets(text), expected);
        }
    }

    #[test]
    fn a_token_in_a_message_is_redacted() {
        let key = "sk-ant-api03-0123456789abcdefghijklmnop";
        let failed = anyhow::anyhow!("the provider refused {key}");
        assert!(!error_chain(failed.as_ref()).contains(key));
    }
}
