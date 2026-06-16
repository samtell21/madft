//! The curated category tree: a human navigation overlay that is TOTAL over the
//! MIME universe. Built by layering `defaults` (shared) under `overrides`
//! (personal), then sweeping any still-unplaced type into a flat `Other` node.
//!
//! This is the *navigation* tree — distinct from the freedesktop subclass DAG
//! in `mimedb`. The two are never conflated (see spec §2).

mod merge;
mod source;
mod tree;
