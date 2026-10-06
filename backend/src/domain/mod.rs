//! Pure domain types and business rules. Nothing in here performs IO.
//!
//! The JSON shapes of these types are the public contract (`docs/CONTRACT.md`
//! §4); field names and enum spellings must not change without updating it.
#![forbid(unsafe_code)]

pub mod agent;
pub mod assistant;
pub mod audit;
pub mod context;
pub mod cycle;
pub mod error;
pub mod graph;
pub mod guardrails;
pub mod issue;
pub mod memory;
pub mod ontology;
pub mod plan;
pub mod prompt;
pub mod run;
pub mod settings;
pub mod status;
pub mod template;
pub mod usage;
pub mod user;
pub mod validation;
pub mod workspace;

pub use error::AppError;

/// Error returned when parsing an unknown enum spelling.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown value `{0}`")]
pub struct UnknownVariant(pub String);

/// Declares a string-backed enum with serde, OpenAPI, `as_str` and `FromStr`
/// support. The listed spelling is used on the wire and in the database.
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash,
            serde::Serialize, serde::Deserialize, utoipa::ToSchema,
        )]
        // utoipa cannot see the per-variant `rename = $s` through the macro, so the
        // container-level rule documents the same spelling (every `$s` is the
        // snake_case form of its variant; `http::tests` checks the spec against serde).
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $s)] $variant),+
        }

        impl $name {
            /// The wire / database spelling of this value.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $s),+
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::domain::UnknownVariant;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($s => Ok($name::$variant),)+
                    other => Err($crate::domain::UnknownVariant(other.to_owned())),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}
pub(crate) use string_enum;
