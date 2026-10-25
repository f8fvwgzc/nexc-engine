//! Data loss prevention on the way out: a classifier for what an agent is
//! about to deliver (an artifact) or send (an external call), and the rule
//! that decides what happens to it. Pure functions, no IO: credentials (the
//! guardrails' scrubber), shapes of personal data (e-mail, phone, card and
//! national-id numbers, by hand, no regex), the workspace's own keywords, and
//! the clearance of the data the content was derived from. Numbers that
//! carry a check digit are only taken when it is right: cards (Luhn), Dutch
//! BSNs (the 11-test), Japanese My Numbers and corporate numbers (their
//! official check digits), so about one random number in ten of the right
//! length is taken for one, not every one.

use serde::Serialize;
use utoipa::ToSchema;

use super::capabilities::DataLabel;
use super::guardrails::redact_secrets;
use super::policy::{Decision, DlpRules, host_matches};
use super::policy_document::{PiiKind, RuleAction, Severity};

/// Most text the classifier reads from one artifact (the rest is not looked at).
pub const SCAN_MAX_BYTES: usize = 4 * 1024 * 1024;

/// Personal data of one kind, counted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PiiFinding {
    pub kind: PiiKind,
    pub count: usize,
}

/// What the classifier found in a piece of content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, ToSchema)]
pub struct Findings {
    /// Credentials: API keys, tokens, private keys.
    pub secrets: usize,
    pub pii: Vec<PiiFinding>,
    /// The workspace's keywords that occur.
    pub keywords: Vec<String>,
}

impl Findings {
    /// Whether nothing was found.
    pub fn is_clean(&self) -> bool {
        self.secrets == 0 && self.pii.is_empty() && self.keywords.is_empty()
    }

    /// Personal data found, of any kind.
    pub fn pii_total(&self) -> usize {
        self.pii.iter().map(|f| f.count).sum()
    }

    /// The findings in words: `1 credential, 2 e-mail addresses, keyword "Project Zeus"`.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.secrets > 0 {
            parts.push(format!(
                "{} credential{}",
                self.secrets,
                if self.secrets == 1 { "" } else { "s" }
            ));
        }
        for finding in &self.pii {
            let what = match (finding.kind, finding.count) {
                (PiiKind::Email, 1) => "e-mail address".to_owned(),
                (PiiKind::Email, _) => "e-mail addresses".to_owned(),
                (PiiKind::Phone, 1) => "phone number".to_owned(),
                (PiiKind::Phone, _) => "phone numbers".to_owned(),
                (PiiKind::Card, 1) => "card number".to_owned(),
                (PiiKind::Card, _) => "card numbers".to_owned(),
                (PiiKind::NationalId, 1) => "national id".to_owned(),
                (PiiKind::NationalId, _) => "national ids".to_owned(),
                (PiiKind::CorporateNumber, 1) => "corporate number".to_owned(),
                (PiiKind::CorporateNumber, _) => "corporate numbers".to_owned(),
            };
            parts.push(format!("{} {what}", finding.count));
        }
        for keyword in &self.keywords {
            parts.push(format!("keyword \"{keyword}\""));
        }
        if parts.is_empty() {
            "nothing".to_owned()
        } else {
            parts.join(", ")
        }
    }
}

fn is_email_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

/// Counts e-mail addresses: `local@domain.tld` with a letter-only top level.
fn count_emails(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut count = 0;
    for (at, _) in text.match_indices('@') {
        let local = text[..at]
            .chars()
            .rev()
            .take_while(|c| is_email_char(*c))
            .count();
        if local == 0 || (at > 0 && bytes[at - 1] == b'.') {
            continue;
        }
        let rest = &text[at + 1..];
        let domain: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
            .collect();
        let domain = domain.trim_end_matches('.');
        let Some((_, tld)) = domain.rsplit_once('.') else {
            continue;
        };
        if tld.len() >= 2
            && tld.chars().all(|c| c.is_ascii_alphabetic())
            && domain.len() > tld.len() + 1
        {
            count += 1;
        }
    }
    count
}

/// Luhn check of a run of digits.
fn luhn(digits: &[u8]) -> bool {
    let mut sum = 0;
    for (i, d) in digits.iter().rev().enumerate() {
        let mut d = u32::from(*d);
        if i % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    sum % 10 == 0
}

/// The check digit of a Japanese My Number (individual number, 12 digits)
/// over its first 11: with `P(n)` the n-th digit counted from the right of
/// those 11 and `Q(n)` = n + 1 for n ≤ 6, n − 5 above, the remainder `r` of
/// Σ P(n)·Q(n) by 11 gives 0 when r ≤ 1, else 11 − r (Cabinet Order of the
/// My Number Act, art. 8).
fn my_number_check(first: &[u8]) -> u8 {
    let sum: u32 = first
        .iter()
        .rev()
        .enumerate()
        .map(|(i, p)| {
            let n = i as u32 + 1;
            let q = if n <= 6 { n + 1 } else { n - 5 };
            u32::from(*p) * q
        })
        .sum();
    match sum % 11 {
        0 | 1 => 0,
        r => (11 - r) as u8,
    }
}

/// Whether 12 digits are a My Number: the last is the check digit of the
/// first 11.
fn is_my_number(d: &[u8]) -> bool {
    d.len() == 12 && my_number_check(&d[..11]) == d[11]
}

/// Whether 13 digits are a Japanese corporate number (houjin bangou): the
/// first is the check digit of the 12-digit base, 9 − (Σ P(n)·Q(n) mod 9)
/// with `P(n)` the n-th base digit from the right and `Q(n)` 1 for odd n, 2
/// for even (Ministerial Order on corporate numbers, art. 2). It is never 0.
fn is_corporate_number(d: &[u8]) -> bool {
    if d.len() != 13 {
        return false;
    }
    let sum: u32 = d[1..]
        .iter()
        .rev()
        .enumerate()
        .map(|(i, p)| u32::from(*p) * if i % 2 == 0 { 1 } else { 2 })
        .sum();
    u32::from(d[0]) == 9 - sum % 9
}

/// Runs of digits with the separators numbers are written with, as `(digits,
/// had a leading plus, separators seen, the raw text)`.
struct Number<'a> {
    digits: Vec<u8>,
    plus: bool,
    separated: bool,
    raw: &'a str,
}

fn numbers(text: &str) -> Vec<Number<'_>> {
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if !(c.is_ascii_digit() || c == '+') {
            continue;
        }
        // Not in the middle of a word or a longer token (`v2`, `a1b2`, `sk-...`).
        if start > 0
            && text[..start]
                .chars()
                .next_back()
                .is_some_and(|p| p.is_ascii_alphanumeric() || p == '_' || p == '/')
        {
            continue;
        }
        let plus = c == '+';
        let mut digits: Vec<u8> = Vec::new();
        if !plus {
            digits.push(c as u8 - b'0');
        }
        let mut separated = false;
        let mut end = start + c.len_utf8();
        let mut pending_sep = false;
        while let Some(&(i, n)) = chars.peek() {
            if n.is_ascii_digit() {
                digits.push(n as u8 - b'0');
                end = i + 1;
                // A separator counts once digits follow it; a trailing one is punctuation.
                separated |= pending_sep;
                pending_sep = false;
                chars.next();
            } else if matches!(n, ' ' | '-' | '.' | '(' | ')') && !pending_sep && !digits.is_empty()
            {
                pending_sep = true;
                chars.next();
            } else {
                break;
            }
        }
        // A number must not run into letters (`1234abc`).
        if text[end..]
            .chars()
            .next()
            .is_some_and(|n| n.is_ascii_alphanumeric())
        {
            continue;
        }
        if digits.len() >= 7 {
            out.push(Number {
                digits,
                plus,
                separated,
                raw: &text[start..end],
            });
        }
    }
    out
}

/// Whether a run of digits is shaped like a national id: a US SSN
/// (`ddd-dd-dddd`), a Dutch BSN (9 digits passing the 11-test), a Japanese
/// My Number (12 digits with its check digit, run together or as on the
/// card, `dddd dddd dddd`), a Swedish personnummer (`yymmdd-nnnn`, or
/// `yyyymmdd-nnnn` and `yyyymmddnnnn` with its Luhn check digit).
fn is_national_id(number: &Number<'_>) -> bool {
    let d = &number.digits;
    match d.len() {
        9 => {
            // SSN with its dashes, or a BSN (11-test).
            let ssn = number.raw.len() == 11
                && number.raw.as_bytes()[3] == b'-'
                && number.raw.as_bytes()[6] == b'-'
                && d[0..3] != [0, 0, 0]
                && d[0..3] != [6, 6, 6]
                && d[0] != 9
                && d[3..5] != [0, 0]
                && d[5..9] != [0, 0, 0, 0];
            if ssn {
                return true;
            }
            let eleven: i32 = d
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let weight = if i == 8 { -1 } else { 9 - i as i32 };
                    i32::from(*v) * weight
                })
                .sum();
            !number.separated && eleven % 11 == 0 && d[0] != 0
        }
        10 => {
            // yymmdd-nnnn
            number.raw.len() == 11
                && number.raw.as_bytes()[6] == b'-'
                && (1..=12).contains(&(d[2] * 10 + d[3]))
                && (1..=31).contains(&(d[4] * 10 + d[5]))
        }
        12 => {
            let raw = number.raw.as_bytes();
            // As the card prints it: four, four, four, one separator throughout.
            let grouped = raw.len() == 14 && matches!(raw[4], b' ' | b'-') && raw[9] == raw[4];
            // A Swedish personnummer with the century: yyyymmdd-nnnn or run together.
            let century = (!number.separated || (raw.len() == 13 && raw[8] == b'-'))
                && matches!(d[0] * 10 + d[1], 19 | 20)
                && (1..=12).contains(&(d[4] * 10 + d[5]))
                && (1..=31).contains(&(d[6] * 10 + d[7]))
                && luhn(&d[2..]);
            !number.plus && (((!number.separated || grouped) && is_my_number(d)) || century)
        }
        _ => false,
    }
}

/// Whether a run of digits is a Japanese corporate number: 13 digits run
/// together, the first the check digit of the rest.
fn is_corporate(number: &Number<'_>) -> bool {
    !number.separated && !number.plus && is_corporate_number(&number.digits)
}

/// Whether a run of digits is shaped like a payment card: 13 to 19 digits
/// passing Luhn, starting the way cards do.
fn is_card(number: &Number<'_>) -> bool {
    let d = &number.digits;
    (13..=19).contains(&d.len()) && !number.plus && matches!(d[0], 3..=6) && luhn(d)
}

/// Whether a run of digits is shaped like a phone number: 9 to 15 digits
/// with a leading plus or the separators numbers are dialled with.
fn is_phone(number: &Number<'_>) -> bool {
    let n = number.digits.len();
    (9..=15).contains(&n) && (number.plus || number.separated)
}

/// Looks for personal data of `kinds` and the `keywords` in `text`.
pub fn classify(text: &str, kinds: &[PiiKind], keywords: &[String]) -> Findings {
    let text = if text.len() > SCAN_MAX_BYTES {
        let mut end = SCAN_MAX_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    } else {
        text
    };
    let mut findings = Findings {
        secrets: redact_secrets(text).1,
        ..Findings::default()
    };
    let mut count = |kind: PiiKind, n: usize| {
        if n > 0 && kinds.contains(&kind) {
            findings.pii.push(PiiFinding { kind, count: n });
        }
    };
    if kinds.contains(&PiiKind::Email) {
        count(PiiKind::Email, count_emails(text));
    }
    if kinds.iter().any(|k| {
        matches!(
            k,
            PiiKind::Phone | PiiKind::Card | PiiKind::NationalId | PiiKind::CorporateNumber
        )
    }) {
        let (mut phones, mut cards, mut ids, mut corporate) = (0, 0, 0, 0);
        // Asked for, a corporate number is one before it is a 13-digit card (both
        // checks pass for about one valid corporate number in twenty-five).
        let corporate_first = kinds.contains(&PiiKind::CorporateNumber);
        for number in numbers(text) {
            if corporate_first && is_corporate(&number) {
                corporate += 1;
            } else if is_card(&number) {
                cards += 1;
            } else if is_national_id(&number) {
                ids += 1;
            } else if is_corporate(&number) {
                corporate += 1;
            } else if is_phone(&number) {
                phones += 1;
            }
        }
        count(PiiKind::Card, cards);
        count(PiiKind::NationalId, ids);
        count(PiiKind::CorporateNumber, corporate);
        count(PiiKind::Phone, phones);
    }
    let lower = text.to_lowercase();
    findings.keywords = keywords
        .iter()
        .filter(|k| !k.trim().is_empty() && lower.contains(&k.trim().to_lowercase()))
        .cloned()
        .collect();
    findings
}

/// Where content is about to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination<'a> {
    /// Delivered as a file of the run.
    Artifact,
    /// Sent to the host.
    Egress(&'a str),
}

/// Decides what happens to content derived from data of `label` with
/// `findings`, headed for `destination`, under the DLP rules.
pub fn decide(
    rules: &DlpRules,
    label: DataLabel,
    findings: &Findings,
    destination: Destination<'_>,
) -> Decision {
    let outbound = rules.outbound(label);
    match destination {
        Destination::Artifact if !outbound.artifacts => {
            return Decision::deny(
                "dlp.labels",
                format!("content derived from {label} data may not leave as an artifact"),
            );
        }
        Destination::Egress(host) => {
            if let Some(hosts) = &outbound.egress_hosts
                && !host_matches(hosts, host)
            {
                return Decision::deny(
                    "dlp.labels",
                    format!("content derived from {label} data may not be sent to {host}"),
                );
            }
        }
        Destination::Artifact => {}
    }
    if findings.secrets > 0 {
        let what = format!(
            "{} credential{} in the content",
            findings.secrets,
            if findings.secrets == 1 { "" } else { "s" }
        );
        return match rules.secrets {
            RuleAction::Deny => Decision::deny("dlp.secrets", what),
            RuleAction::Alert => Decision::alert("dlp.secrets", Severity::High, what),
        };
    }
    if rules.pii.enabled && (!findings.pii.is_empty() || !findings.keywords.is_empty()) {
        let what = format!("personal data in the content: {}", findings.describe());
        return match rules.pii.action {
            RuleAction::Deny => Decision::deny("dlp.pii", what),
            RuleAction::Alert => Decision::alert("dlp.pii", Severity::Medium, what),
        };
    }
    Decision::allow("dlp", format!("{label} content, nothing found"))
}

/// Text to classify out of an artifact's bytes: plain text as it is; the
/// words of an Office document (`.docx`, `.xlsx`, `.pptx`) out of its XML
/// parts; nothing for other binaries (the label check still applies).
pub fn extract_text(bytes: &[u8], mime: &str, path: &str) -> Option<String> {
    let lower = path.to_ascii_lowercase();
    let office = lower.ends_with(".docx")
        || lower.ends_with(".xlsx")
        || lower.ends_with(".pptx")
        || mime.contains("officedocument");
    if office || (bytes.starts_with(b"PK\x03\x04") && mime.contains("zip")) {
        return office_text(bytes);
    }
    let textual = mime.starts_with("text/")
        || matches!(
            mime,
            "application/json"
                | "application/xml"
                | "application/yaml"
                | "application/toml"
                | "application/javascript"
                | "application/x-sh"
        )
        || lower.ends_with(".md")
        || lower.ends_with(".csv")
        || lower.ends_with(".txt")
        || lower.ends_with(".py")
        || lower.ends_with(".rs")
        || lower.ends_with(".ts");
    if !textual {
        return None;
    }
    let slice = &bytes[..bytes.len().min(SCAN_MAX_BYTES)];
    Some(String::from_utf8_lossy(slice).into_owned())
}

/// The text of an Office Open XML document: every XML part's text between tags.
fn office_text(bytes: &[u8]) -> Option<String> {
    use std::io::Read as _;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut out = String::new();
    for i in 0..archive.len() {
        let Ok(file) = archive.by_index(i) else {
            continue;
        };
        let Ok(name) = file.name().map(|name| name.into_owned()) else {
            continue;
        };
        let wanted = name.starts_with("word/")
            || name.starts_with("xl/sharedStrings")
            || name.starts_with("xl/worksheets/")
            || name.starts_with("ppt/slides/");
        if !wanted || !name.ends_with(".xml") || out.len() > SCAN_MAX_BYTES {
            continue;
        }
        let mut xml = String::new();
        if file
            .take(SCAN_MAX_BYTES as u64)
            .read_to_string(&mut xml)
            .is_err()
        {
            continue;
        }
        strip_tags(&xml, &mut out);
        out.push('\n');
    }
    Some(out)
}

/// Appends the text between the tags of `xml` to `out`, a space between runs.
fn strip_tags(xml: &str, out: &mut String) {
    let mut in_tag = false;
    let mut last_space = true;
    for c in xml.chars() {
        match c {
            '<' => {
                in_tag = true;
                if !last_space {
                    out.push(' ');
                    last_space = true;
                }
            }
            '>' => in_tag = false,
            _ if in_tag => {}
            c => {
                out.push(c);
                last_space = c.is_whitespace();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_emails_phones_cards_and_ids_but_not_ordinary_numbers() {
        let text = "Reach Ada at ada.lovelace@example.org or +44 20 7946 0958. \
            Card 4111 1111 1111 1111 expires 12/27. SSN 123-45-6789. BSN 111222333. \
            Order 1234567890123 shipped; version 2.0.1; port 8080; year 2026; invoice #44.";
        let f = classify(text, &PiiKind::ALL, &[]);
        let count = |kind| f.pii.iter().find(|p| p.kind == kind).map_or(0, |p| p.count);
        assert_eq!(count(PiiKind::Email), 1, "{f:?}");
        assert_eq!(count(PiiKind::Phone), 1, "{f:?}");
        assert_eq!(count(PiiKind::Card), 1, "{f:?}");
        assert_eq!(count(PiiKind::NationalId), 2, "{f:?}");
        assert_eq!(f.secrets, 0);
        assert!(
            f.describe().contains("1 e-mail address"),
            "{}",
            f.describe()
        );
        assert!(f.describe().contains("2 national ids"));
        // Only the kinds asked for are counted.
        let emails_only = classify(text, &[PiiKind::Email], &[]);
        assert_eq!(emails_only.pii.len(), 1);
        assert_eq!(emails_only.pii[0].kind, PiiKind::Email);
        assert!(classify("nothing here, 42 times", &PiiKind::ALL, &[]).is_clean());
        assert_eq!(count_emails("not@an-email, a@b.c, x@y.org and @handle"), 1);
        assert_eq!(count_emails("first.last+tag@sub.example.co.uk"), 1);
    }

    #[test]
    fn my_numbers_and_corporate_numbers_are_taken_only_with_their_check_digit() {
        let ids = |text: &str, kind: PiiKind| {
            classify(text, &[kind], &[])
                .pii
                .first()
                .map_or(0, |f| f.count)
        };
        // My Number: 12345678901 has check digit 8.
        assert_eq!(my_number_check(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1]), 8);
        assert_eq!(
            ids("individual number 123456789018", PiiKind::NationalId),
            1
        );
        assert_eq!(ids("as printed: 1234 5678 9018", PiiKind::NationalId), 1);
        assert_eq!(ids("as printed: 1234-5678-9018", PiiKind::NationalId), 1);
        assert_eq!(
            ids("wrong check digit 123456789012", PiiKind::NationalId),
            0
        );
        assert_eq!(
            ids("other grouping 123 456 789 018", PiiKind::NationalId),
            0
        );
        assert_eq!(
            ids("mixed separators 1234 5678-9018", PiiKind::NationalId),
            0
        );
        // A remainder of 0 or 1 gives check digit 0.
        let zero = (10_000_000_000..100_000_000_000u64)
            .step_by(7_919)
            .map(|n| {
                format!("{n:011}")
                    .bytes()
                    .map(|b| b - b'0')
                    .collect::<Vec<u8>>()
            })
            .find(|d| my_number_check(d) == 0)
            .expect("some base has check digit 0");
        let text: String = zero.iter().map(|d| char::from(b'0' + d)).collect();
        assert_eq!(ids(&format!("number {text}0"), PiiKind::NationalId), 1);
        // Corporate numbers: 7000012050002 and 4000012090001 are valid.
        assert!(is_corporate_number(&[
            7, 0, 0, 0, 0, 1, 2, 0, 5, 0, 0, 0, 2
        ]));
        assert_eq!(
            ids("corporate number 7000012050002", PiiKind::CorporateNumber),
            1
        );
        assert_eq!(
            ids("corporate number 4000012090001", PiiKind::CorporateNumber),
            1
        );
        assert_eq!(ids("off by one 7000012050003", PiiKind::CorporateNumber), 0);
        assert_eq!(
            ids(
                "check digit 0 never is 0000012050002",
                PiiKind::CorporateNumber
            ),
            0
        );
        // A corporate number that also passes Luhn is still a corporate number when asked.
        let both = (1_000_000_000_000..10_000_000_000_000u64)
            .step_by(7_919_113)
            .map(|n| {
                format!("{n:013}")
                    .bytes()
                    .map(|b| b - b'0')
                    .collect::<Vec<u8>>()
            })
            .find(|d| is_corporate_number(d) && luhn(d) && matches!(d[0], 3..=6))
            .expect("a corporate number that passes Luhn");
        let text: String = both.iter().map(|d| char::from(b'0' + d)).collect();
        assert_eq!(ids(&text, PiiKind::CorporateNumber), 1, "{text}");
        assert_eq!(ids(&text, PiiKind::Card), 1, "{text}");
        // The Swedish personnummer with its century, run together or with its dash.
        assert_eq!(ids("pnr 199001011239", PiiKind::NationalId), 1);
        assert_eq!(ids("pnr 19900101-1239", PiiKind::NationalId), 1);
        assert_eq!(ids("pnr 199001011238", PiiKind::NationalId), 0);
        // Not a personal identifier, and not looked for unless asked.
        assert_eq!(
            ids("corporate number 7000012050002", PiiKind::NationalId),
            0
        );
        assert!(!PiiKind::DEFAULT.contains(&PiiKind::CorporateNumber));
        let f = classify("7000012050002 and 4000012090001", &PiiKind::ALL, &[]);
        assert!(
            f.describe().contains("2 corporate numbers"),
            "{}",
            f.describe()
        );
    }

    /// A deterministic stream of digits (xorshift64*), for the corpus.
    struct Digits(u64);

    impl Digits {
        fn next(&mut self) -> u8 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as u8 % 10
        }

        fn number(&mut self, len: usize) -> String {
            (0..len).map(|_| char::from(b'0' + self.next())).collect()
        }
    }

    /// How often random numbers of `len` digits, written alone in a
    /// sentence, are taken for `kind`.
    fn false_positive_rate(len: usize, kind: PiiKind, samples: usize) -> f64 {
        let mut digits = Digits(0x9E37_79B9_7F4A_7C15 ^ len as u64);
        let hits = (0..samples)
            .filter(|_| {
                let text = format!("Reference {} on file.", digits.number(len));
                classify(&text, &[kind], &[]).pii_total() > 0
            })
            .count();
        hits as f64 / samples as f64
    }

    #[test]
    fn random_numbers_are_taken_for_identifiers_about_one_time_in_ten() {
        // Before the check digits every 12-digit number was a My Number (100 %).
        let my_number = false_positive_rate(12, PiiKind::NationalId, 50_000);
        assert!((0.09..0.11).contains(&my_number), "{my_number}");
        let corporate = false_positive_rate(13, PiiKind::CorporateNumber, 50_000);
        assert!((0.09..0.11).contains(&corporate), "{corporate}");
        assert_eq!(false_positive_rate(13, PiiKind::NationalId, 10_000), 0.0);
        assert_eq!(
            false_positive_rate(12, PiiKind::CorporateNumber, 10_000),
            0.0
        );
    }

    #[test]
    fn finds_credentials_and_keywords() {
        let text = "export ANTHROPIC_API_KEY=sk-ant-api03-AbCdEf0123456789xyz_-Q\nProject Zeus launches in May.";
        let f = classify(
            text,
            &PiiKind::ALL,
            &["project zeus".into(), "Hermes".into()],
        );
        assert_eq!(f.secrets, 1);
        assert_eq!(f.keywords, vec!["project zeus"]);
        assert!(!f.is_clean());
        assert!(f.describe().starts_with("1 credential"));
    }

    #[test]
    fn the_rules_decide_by_label_then_secrets_then_pii() {
        let rules = DlpRules::default();
        let clean = Findings::default();
        let restricted = decide(&rules, DataLabel::Restricted, &clean, Destination::Artifact);
        assert!(!restricted.allowed && restricted.rule == "dlp.labels");
        assert!(
            decide(
                &rules,
                DataLabel::Confidential,
                &clean,
                Destination::Artifact
            )
            .allowed
        );
        let sent = decide(
            &rules,
            DataLabel::Confidential,
            &clean,
            Destination::Egress("api.example.com"),
        );
        assert!(!sent.allowed && sent.rule == "dlp.labels");
        assert!(
            decide(
                &rules,
                DataLabel::Internal,
                &clean,
                Destination::Egress("api.example.com")
            )
            .allowed
        );
        let leaked = Findings {
            secrets: 1,
            ..Findings::default()
        };
        let denied = decide(&rules, DataLabel::Internal, &leaked, Destination::Artifact);
        assert_eq!((denied.allowed, denied.rule), (false, "dlp.secrets"));
        let personal = Findings {
            pii: vec![PiiFinding {
                kind: PiiKind::Email,
                count: 2,
            }],
            ..Findings::default()
        };
        let flagged = decide(
            &rules,
            DataLabel::Internal,
            &personal,
            Destination::Artifact,
        );
        assert!(flagged.allowed);
        assert_eq!((flagged.verdict(), flagged.rule), ("alert", "dlp.pii"));
        assert!(flagged.reason.contains("2 e-mail addresses"));
        let strict = DlpRules {
            pii: crate::domain::policy::PiiRules {
                action: RuleAction::Deny,
                ..Default::default()
            },
            ..DlpRules::default()
        };
        assert!(
            !decide(
                &strict,
                DataLabel::Internal,
                &personal,
                Destination::Artifact
            )
            .allowed
        );
        let off = DlpRules {
            pii: crate::domain::policy::PiiRules {
                enabled: false,
                ..Default::default()
            },
            ..DlpRules::default()
        };
        assert!(decide(&off, DataLabel::Internal, &personal, Destination::Artifact).allowed);
    }

    #[test]
    fn reads_text_out_of_plain_files_and_office_documents() {
        assert_eq!(
            extract_text(b"hello", "text/markdown", "a.md").as_deref(),
            Some("hello")
        );
        assert_eq!(extract_text(b"\x89PNG", "image/png", "a.png"), None);
        // A minimal .docx: one XML part with a paragraph.
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("word/document.xml", options).unwrap();
        std::io::Write::write_all(
            &mut writer,
            br#"<w:document><w:body><w:p><w:r><w:t>Call ada@example.org</w:t></w:r><w:t>today</w:t></w:p></w:body></w:document>"#,
        )
        .unwrap();
        writer.start_file("word/media/x.bin", options).unwrap();
        std::io::Write::write_all(&mut writer, b"\x00\x01").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let text = extract_text(
            &bytes,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "report.docx",
        )
        .unwrap();
        assert!(text.contains("Call ada@example.org"), "{text}");
        assert!(text.contains("today"));
        assert_eq!(classify(&text, &PiiKind::ALL, &[]).pii_total(), 1);
        assert_eq!(extract_text(b"not a zip", "", "x.docx"), None);
    }
}
