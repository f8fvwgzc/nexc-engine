//! Artifacts a workspace keeps by hand: text files (code, HTML, Markdown)
//! with every version, in folders, optionally attached to an issue. Which
//! names are allowed, how a file is previewed and highlighted, and how a
//! copy is named.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::issue::{IssuePerson, IssueRef};
use super::run::{Artifact, RunStatus};
use super::string_enum;

/// Largest version of a file in bytes (1 MiB, the request body limit).
pub const FILE_MAX_BYTES: usize = 1_048_576;
/// Longest file name, in characters.
pub const FILE_NAME_MAX: usize = 200;
/// Longest folder name, in characters.
pub const FOLDER_NAME_MAX: usize = 120;
/// Most folders a workspace may have; the client builds the tree from all of them.
pub const FOLDERS_MAX: i64 = 1_000;
/// Most files one list answers with.
pub const LIST_MAX: i64 = 1_000;
/// Files the list of every file answers with when no `limit` is given.
pub const RECENT_DEFAULT: i64 = 200;
/// Why a file was refused: not a name or content that can be an artifact (415).
pub const NOT_TEXT: &str = "Only text files (code, HTML, Markdown) can be artifacts";
/// Why content was refused: it carries a credential (422).
pub const HAS_CREDENTIAL: &str = "This file contains what looks like a credential (an API key, token or private key). Remove it, then upload again.";

/// Extensions (lower case, without the dot) a file name may end with:
/// code, markup, configuration and text. `dockerfile`, `makefile`,
/// `gitignore` and `editorconfig` also name a copy of a whole name
/// ([`copy_name`]).
const EXTENSIONS: &[&str] = &[
    "md",
    "markdown",
    "mdx",
    "txt",
    "html",
    "htm",
    "css",
    "scss",
    "sass",
    "less",
    "js",
    "jsx",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "mts",
    "cts",
    "json",
    "jsonc",
    "json5",
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "env.example",
    "xml",
    "svg",
    "csv",
    "tsv",
    "sql",
    "graphql",
    "gql",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "bat",
    "py",
    "rb",
    "go",
    "rs",
    "java",
    "kt",
    "kts",
    "scala",
    "swift",
    "c",
    "h",
    "cc",
    "cpp",
    "cxx",
    "hpp",
    "hh",
    "cs",
    "fs",
    "php",
    "pl",
    "lua",
    "r",
    "dart",
    "ex",
    "exs",
    "erl",
    "hs",
    "clj",
    "elm",
    "vue",
    "svelte",
    "astro",
    "proto",
    "tf",
    "hcl",
    "dockerfile",
    "makefile",
    "gitignore",
    "editorconfig",
    "lock",
    "log",
    "diff",
    "patch",
    "rst",
    "adoc",
    "tex",
];

/// Whole names that are allowed without an extension of their own.
const BASENAMES: [&str; 5] = [
    "dockerfile",
    "makefile",
    ".gitignore",
    ".editorconfig",
    ".env.example",
];

string_enum!(
    /// How a file is previewed.
    ArtifactKind {
        /// Rendered as Markdown.
        Markdown => "markdown",
        /// Rendered in a sandbox that runs no script.
        Html => "html",
        /// Highlighted source.
        Code => "code",
        /// Plain text, data or prose markup, highlighted where a language fits.
        Text => "text",
    }
);

/// A folder of a workspace's artifacts.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ArtifactFolder {
    pub id: Uuid,
    pub workspace_id: Uuid,
    /// The folder it is in; `null` at the top level.
    #[schema(required = true)]
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A text file of a workspace, as of its current version.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ArtifactFile {
    pub id: Uuid,
    pub workspace_id: Uuid,
    /// The folder it is in; `null` at the top level.
    #[schema(required = true)]
    pub folder_id: Option<Uuid>,
    pub name: String,
    /// The extension that makes it a text file (`md`, `ts`), lower case;
    /// empty for a name allowed as a whole (`Dockerfile`).
    pub extension: String,
    pub kind: ArtifactKind,
    /// The highlighter's language id (`typescript`, `rust`, `plaintext`).
    pub language: String,
    /// The issue it is attached to; who can see that issue can see the file.
    #[schema(required = true)]
    pub issue: Option<IssueRef>,
    pub current_version: i32,
    /// Bytes of the current version.
    pub size: i64,
    /// Hex SHA-256 of the current version.
    pub sha256: String,
    /// The file this one was copied from, while it exists.
    #[schema(required = true)]
    pub duplicated_from: Option<Uuid>,
    #[schema(required = true)]
    pub created_by: Option<IssuePerson>,
    /// Who saved the current version.
    #[schema(required = true)]
    pub updated_by: Option<IssuePerson>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One version of a file.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ArtifactVersion {
    pub version: i32,
    pub size: i64,
    pub sha256: String,
    #[schema(required = true)]
    pub created_by: Option<IssuePerson>,
    pub created_at: DateTime<Utc>,
}

/// The text of one version of a file.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ArtifactContent {
    pub file_id: Uuid,
    pub version: i32,
    pub content: String,
}

/// A file a run of an issue's graph produced, with the run it came from.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RunArtifact {
    #[serde(flatten)]
    pub artifact: Artifact,
    pub run_status: RunStatus,
    pub run_created_at: DateTime<Utc>,
}

/// What makes a name a text file's: its extension, lower case (empty for a
/// whole name such as `Dockerfile`), or `None` when it is not allowed.
pub fn extension_of(name: &str) -> Option<String> {
    // ASCII only, so the extension's length in bytes is the name's own.
    let lower = name.to_ascii_lowercase();
    if BASENAMES.contains(&lower.as_str()) {
        return Some(String::new());
    }
    // The one extension with a dot of its own.
    if lower.len() > ".env.example".len() && lower.ends_with(".env.example") {
        return Some("env.example".to_owned());
    }
    let (stem, extension) = lower.rsplit_once('.')?;
    (!stem.is_empty() && EXTENSIONS.contains(&extension)).then(|| extension.to_owned())
}

/// How a file of this extension (from [`extension_of`]) is previewed, and the
/// highlighter's language for it. `name` decides for whole names.
pub fn kind_and_language(name: &str, extension: &str) -> (ArtifactKind, &'static str) {
    use ArtifactKind::{Code, Html, Markdown, Text};
    let key = if extension.is_empty() {
        name.trim_start_matches('.').to_ascii_lowercase()
    } else {
        extension.to_owned()
    };
    match key.as_str() {
        "md" | "markdown" => (Markdown, "markdown"),
        "mdx" => (Markdown, "mdx"),
        "html" | "htm" => (Html, "html"),
        "txt" | "lock" | "gitignore" => (Text, "plaintext"),
        "log" => (Text, "log"),
        "csv" => (Text, "csv"),
        "tsv" => (Text, "tsv"),
        "rst" => (Text, "rst"),
        "adoc" => (Text, "asciidoc"),
        "tex" => (Text, "latex"),
        "css" => (Code, "css"),
        "scss" => (Code, "scss"),
        "sass" => (Code, "sass"),
        "less" => (Code, "less"),
        "js" | "mjs" | "cjs" => (Code, "javascript"),
        "jsx" => (Code, "jsx"),
        "ts" | "mts" | "cts" => (Code, "typescript"),
        "tsx" => (Code, "tsx"),
        "json" => (Code, "json"),
        "jsonc" => (Code, "jsonc"),
        "json5" => (Code, "json5"),
        "yaml" | "yml" => (Code, "yaml"),
        "toml" => (Code, "toml"),
        "ini" | "cfg" | "conf" | "editorconfig" => (Code, "ini"),
        "env.example" => (Code, "dotenv"),
        "xml" | "svg" => (Code, "xml"),
        "sql" => (Code, "sql"),
        "graphql" | "gql" => (Code, "graphql"),
        "sh" | "bash" => (Code, "bash"),
        "zsh" => (Code, "zsh"),
        "fish" => (Code, "fish"),
        "ps1" => (Code, "powershell"),
        "bat" => (Code, "bat"),
        "py" => (Code, "python"),
        "rb" => (Code, "ruby"),
        "go" => (Code, "go"),
        "rs" => (Code, "rust"),
        "java" => (Code, "java"),
        "kt" | "kts" => (Code, "kotlin"),
        "scala" => (Code, "scala"),
        "swift" => (Code, "swift"),
        "c" | "h" => (Code, "c"),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" => (Code, "cpp"),
        "cs" => (Code, "csharp"),
        "fs" => (Code, "fsharp"),
        "php" => (Code, "php"),
        "pl" => (Code, "perl"),
        "lua" => (Code, "lua"),
        "r" => (Code, "r"),
        "dart" => (Code, "dart"),
        "ex" | "exs" => (Code, "elixir"),
        "erl" => (Code, "erlang"),
        "hs" => (Code, "haskell"),
        "clj" => (Code, "clojure"),
        "elm" => (Code, "elm"),
        "vue" => (Code, "vue"),
        "svelte" => (Code, "svelte"),
        "astro" => (Code, "astro"),
        "proto" => (Code, "proto"),
        "tf" => (Code, "terraform"),
        "hcl" => (Code, "hcl"),
        "dockerfile" => (Code, "dockerfile"),
        "makefile" => (Code, "makefile"),
        "diff" | "patch" => (Code, "diff"),
        _ => (Text, "plaintext"),
    }
}

/// Checks a file or folder name: 1 to `max` characters once trimmed, no
/// `/`, `\` or control characters, and not only dots. Returns it trimmed.
pub fn check_name(name: &str, max: usize) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > max {
        return Err(format!("must be 1 to {max} characters"));
    }
    if name
        .chars()
        .any(|c| matches!(c, '/' | '\\') || c.is_control())
    {
        return Err("must not contain /, \\ or control characters".into());
    }
    if name.chars().all(|c| c == '.') {
        return Err("must not be only dots".into());
    }
    Ok(name.to_owned())
}

/// The text of an upload, if it can be an artifact: valid UTF-8 without a
/// NUL byte (which marks a binary file).
pub fn text_of(bytes: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(bytes).ok()?;
    (!text.contains('\0')).then_some(text)
}

/// The name of the `n`th copy of a file (` copy`, ` copy 2`, …), inserted
/// before the extension so the copy stays the same kind of file. A whole
/// name (`Dockerfile`) keeps itself as the extension
/// (`Dockerfile copy.dockerfile`). The stem is shortened when the name
/// would grow past [`FILE_NAME_MAX`].
pub fn copy_name(name: &str, n: u32) -> String {
    let suffix = if n <= 1 {
        " copy".to_owned()
    } else {
        format!(" copy {n}")
    };
    let (stem, extension) = match extension_of(name).as_deref() {
        Some("") => (
            name.to_owned(),
            format!(".{}", name.trim_start_matches('.').to_ascii_lowercase()),
        ),
        Some(extension) => {
            let cut = name.len() - extension.len() - 1;
            (name[..cut].to_owned(), name[cut..].to_owned())
        }
        None => (name.to_owned(), String::new()),
    };
    let room = FILE_NAME_MAX.saturating_sub(suffix.chars().count() + extension.chars().count());
    let stem: String = stem.chars().take(room).collect();
    format!("{}{suffix}{extension}", stem.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_markup_config_and_text_are_allowed_and_nothing_else() {
        for (name, extension) in [
            ("notes.md", "md"),
            ("README.MD", "md"),
            ("index.html", "html"),
            ("app.test.tsx", "tsx"),
            ("main.rs", "rs"),
            ("prod.env.example", "env.example"),
            ("deploy.tf", "tf"),
            ("changes.patch", "patch"),
            ("Dockerfile", ""),
            ("makefile", ""),
            (".gitignore", ""),
            (".editorconfig", ""),
            (".env.example", ""),
            ("api.dockerfile", "dockerfile"),
        ] {
            assert_eq!(extension_of(name).as_deref(), Some(extension), "{name}");
        }
        for name in [
            "report.pdf",
            "slides.docx",
            "photo.png",
            "archive.zip",
            "binary",
            "README",
            ".md",
            ".env",
            "notes.md.exe",
            "env.example",
            "Dockerfile.prod",
            // A sign that lower-cases to ASCII is not the ASCII letter.
            "a.\u{212A}t",
        ] {
            assert_eq!(extension_of(name), None, "{name}");
        }
    }

    #[test]
    fn a_file_is_previewed_and_highlighted_by_its_kind() {
        let classify = |name: &str| kind_and_language(name, &extension_of(name).unwrap());
        assert_eq!(classify("notes.md"), (ArtifactKind::Markdown, "markdown"));
        assert_eq!(classify("page.HTM"), (ArtifactKind::Html, "html"));
        assert_eq!(classify("main.ts"), (ArtifactKind::Code, "typescript"));
        assert_eq!(classify("lib.rs"), (ArtifactKind::Code, "rust"));
        assert_eq!(classify("run.sh"), (ArtifactKind::Code, "bash"));
        assert_eq!(classify("data.csv"), (ArtifactKind::Text, "csv"));
        assert_eq!(classify("todo.txt"), (ArtifactKind::Text, "plaintext"));
        assert_eq!(classify("Dockerfile"), (ArtifactKind::Code, "dockerfile"));
        assert_eq!(classify("Makefile"), (ArtifactKind::Code, "makefile"));
        assert_eq!(classify(".gitignore"), (ArtifactKind::Text, "plaintext"));
        assert_eq!(classify(".env.example"), (ArtifactKind::Code, "dotenv"));
        assert_eq!(classify("a.dockerfile"), classify("Dockerfile"));
        // Every allowed extension has a kind and a language.
        for extension in EXTENSIONS {
            let (_, language) = kind_and_language("x", extension);
            assert!(!language.is_empty(), "{extension}");
        }
    }

    #[test]
    fn names_are_one_segment_of_printable_text() {
        assert_eq!(
            check_name("  notes.md ", FILE_NAME_MAX).as_deref(),
            Ok("notes.md")
        );
        assert!(check_name("", FILE_NAME_MAX).is_err());
        assert!(check_name("   ", FILE_NAME_MAX).is_err());
        assert!(check_name("a/b.md", FILE_NAME_MAX).is_err());
        assert!(check_name("a\\b.md", FILE_NAME_MAX).is_err());
        assert!(check_name("a\u{0}.md", FILE_NAME_MAX).is_err());
        assert!(check_name("a\nb.md", FILE_NAME_MAX).is_err());
        assert!(check_name("..", FILE_NAME_MAX).is_err());
        assert!(check_name(&"x".repeat(121), FOLDER_NAME_MAX).is_err());
        assert!(check_name(&"x".repeat(120), FOLDER_NAME_MAX).is_ok());
        assert!(check_name("Spec ✓ 2026", FOLDER_NAME_MAX).is_ok());
    }

    #[test]
    fn only_utf8_without_nul_is_text() {
        assert_eq!(text_of(b"# Title\n"), Some("# Title\n"));
        assert_eq!(text_of(b""), Some(""));
        assert_eq!(text_of("héllo ✓".as_bytes()), Some("héllo ✓"));
        assert_eq!(text_of(b"\xff\xfe\x00"), None);
        assert_eq!(text_of(b"text\0more"), None);
    }

    #[test]
    fn a_copy_is_named_before_its_extension() {
        assert_eq!(copy_name("notes.md", 1), "notes copy.md");
        assert_eq!(copy_name("notes.md", 2), "notes copy 2.md");
        assert_eq!(copy_name("app.test.tsx", 1), "app.test copy.tsx");
        assert_eq!(copy_name("prod.env.example", 3), "prod copy 3.env.example");
        assert_eq!(copy_name("Dockerfile", 1), "Dockerfile copy.dockerfile");
        assert_eq!(copy_name(".gitignore", 2), ".gitignore copy 2.gitignore");
        for name in ["notes.md", "Dockerfile", ".env.example", ".gitignore"] {
            let copy = copy_name(name, 7);
            assert_eq!(
                extension_of(&copy).map(|e| kind_and_language(&copy, &e)),
                extension_of(name).map(|e| kind_and_language(name, &e)),
                "{copy} stays the kind of file {name} is"
            );
        }
        let long = format!("{}.md", "a".repeat(FILE_NAME_MAX - 3));
        let copy = copy_name(&long, 12);
        assert_eq!(copy.chars().count(), FILE_NAME_MAX);
        assert!(copy.ends_with(" copy 12.md"));
    }
}
