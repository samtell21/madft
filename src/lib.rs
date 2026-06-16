//! madft — inspect and set XDG default applications via a curated category tree.
//!
//! Layers: the read-only facts (`mimedb`, `appindex`, `defaults`, `paths`), the
//! curated `categories` tree, the `writer` (atomic backed-up mimeapps.list
//! edits), and the `engine` that orchestrates them into operations.

pub mod types;
pub mod error;
pub mod paths;
pub mod mimedb;
pub mod appindex;
pub mod defaults;
pub mod categories;
pub mod writer;
pub mod engine;
