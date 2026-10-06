//! Searching a workspace by a few typed characters, across the kinds of
//! things a member opens: what the command palette shows.

use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::string_enum;

/// Shortest text that is searched for; less matches too much to be useful.
pub const QUERY_MIN: usize = 2;
/// Longest text that is searched for.
pub const QUERY_MAX: usize = 100;
/// Most hits of one kind.
pub const PER_KIND_MAX: i64 = 10;

string_enum!(
    /// What a search hit is.
    SearchKind {
        Issue => "issue",
        Project => "project",
        Graph => "graph",
        Document => "document",
        Team => "team",
        Member => "member",
    }
);

/// One thing that matched a search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SearchHit {
    pub kind: SearchKind,
    pub id: Uuid,
    /// An issue's identifier and title, a project's or graph's name, a person's name.
    pub title: String,
    /// What tells it apart: an issue's state, a team's key, an e-mail.
    pub subtitle: String,
}

/// The text to search for, or `None` when there is too little of it.
pub fn query(raw: &str) -> Option<String> {
    let text: String = raw.trim().chars().take(QUERY_MAX).collect();
    (text.chars().count() >= QUERY_MIN).then_some(text)
}

/// Whether `text` contains `needle`, ignoring case. `needle` is lower case.
pub fn matches(text: &str, needle: &str) -> bool {
    text.to_lowercase().contains(needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_needs_two_characters_and_is_cut() {
        assert_eq!(query("  a "), None);
        assert_eq!(query(" ab ").as_deref(), Some("ab"));
        assert_eq!(query(&"x".repeat(300)).unwrap().chars().count(), QUERY_MAX);
        assert!(matches("Dana Developer", "dev") && !matches("Dana", "dev"));
    }
}
