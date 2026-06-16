//! The curated category tree: a human navigation overlay that is TOTAL over the
//! MIME universe. Built by layering `defaults` (shared) under `overrides`
//! (personal), then sweeping any still-unplaced type into a flat `Other` node.
//!
//! This is the *navigation* tree — distinct from the freedesktop subclass DAG
//! in `mimedb`. The two are never conflated (see spec §2).

mod merge;
mod source;
mod tree;

pub use tree::{CategoryId, CategoryNode, CategoryTree};
pub use source::{
    parse_categories, write_default_categories, CategorySpec, FileSource, Source, StaticSource,
    DEFAULT_CATEGORIES,
};
pub use merge::{build, OTHER};
