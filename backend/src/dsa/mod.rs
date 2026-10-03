//! Data structures and algorithms used by the engine, each self-contained and
//! unit tested. See the README for the complexity table.
#![forbid(unsafe_code)]

pub mod bm25;
pub mod graph;
pub mod lru;
pub mod priority;
pub mod token_bucket;
pub mod union_find;
