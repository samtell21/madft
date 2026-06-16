//! The layered merge: `defaults ← overrides ← Other` (spec §4).
//!
//! 1. Load both layers (each already single-placement-validated within itself).
//! 2. Resolve each type's single home into `placement`: insert `defaults` first,
//!    then let `overrides` overwrite (override SUPERSEDES default — the intended
//!    re-placement mechanism, not a conflict). All keys are alias-canonicalized
//!    via the MIME DB.
//! 3. Materialize the arena: every declared category path PLUS its derived
//!    ancestor prefixes becomes a node; `supercategory` is the prefix node.
//! 4. Sweep every still-unplaced `mimedb.all_types()` into a flat `Other` node,
//!    so the tree is TOTAL over the MIME universe (spec §2).

use std::collections::{BTreeMap, BTreeSet};

use crate::error::Result;
use crate::mimedb::MimeDb;
use crate::types::MimeType;

use super::source::Source;
use super::tree::{CategoryId, CategoryNode, CategoryTree};

/// The flat catch-all node name for unplaced types.
pub const OTHER: &str = "Other";

/// Build the merged category tree from the two layers and the MIME universe.
/// This is the `categories::tree()` entry point referenced by spec §3.
pub fn build(
    defaults: &dyn Source,
    overrides: &dyn Source,
    mimedb: &MimeDb,
) -> Result<CategoryTree> {
    let default_specs = defaults.load()?;
    let override_specs = overrides.load()?;

    // (2) Final placement per canonical type. Defaults first, overrides win.
    let mut placement: BTreeMap<MimeType, String> = BTreeMap::new();
    for spec in &default_specs {
        for t in &spec.types {
            placement.insert(mimedb.canonicalize(t), spec.path.clone());
        }
    }
    for spec in &override_specs {
        for t in &spec.types {
            placement.insert(mimedb.canonicalize(t), spec.path.clone());
        }
    }

    // (3) Every declared path plus its ancestor prefixes must be a node.
    let mut node_paths: BTreeSet<String> = BTreeSet::new();
    for spec in default_specs.iter().chain(override_specs.iter()) {
        let segments: Vec<&str> = spec.path.split('.').collect();
        for i in 1..=segments.len() {
            node_paths.insert(segments[..i].join("."));
        }
    }

    // (4) Types in the universe that nothing placed fall to `Other` (sorted,
    // deduped — BTreeSet gives both).
    let other_types: Vec<MimeType> = mimedb
        .all_types()
        .map(|t| mimedb.canonicalize(t))
        .filter(|t| !placement.contains_key(t))
        .collect::<BTreeSet<MimeType>>()
        .into_iter()
        .collect();
    if !other_types.is_empty() {
        node_paths.insert(OTHER.to_string());
    }

    // Assign a CategoryId per path. Sorted order puts a parent strictly before
    // its children (a dotted prefix sorts before its extensions).
    let ordered: Vec<String> = node_paths.into_iter().collect();
    let index: BTreeMap<String, usize> = ordered
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, p)| (p, i))
        .collect();

    let mut arena: Vec<CategoryNode> = ordered
        .iter()
        .map(|p| {
            let (name, supercategory) = match p.rfind('.') {
                Some(dot) => (
                    p[dot + 1..].to_string(),
                    Some(CategoryId(*index.get(&p[..dot]).expect("ancestor indexed"))),
                ),
                None => (p.clone(), None),
            };
            CategoryNode { name, supercategory, types: Vec::new() }
        })
        .collect();

    // Place each type onto its final node, preserving per-spec listed order.
    // A type whose final home differs from this spec's path (because an override
    // moved it) is skipped here and picked up when its winning spec is visited.
    for spec in default_specs.iter().chain(override_specs.iter()) {
        let nid = *index.get(spec.path.as_str()).expect("path indexed");
        for t in &spec.types {
            let canon = mimedb.canonicalize(t);
            if placement.get(&canon).map(String::as_str) == Some(spec.path.as_str())
                && !arena[nid].types.contains(&canon)
            {
                arena[nid].types.push(canon);
            }
        }
    }

    if !other_types.is_empty() {
        let nid = *index.get(OTHER).expect("other indexed");
        arena[nid].types = other_types;
    }

    Ok(CategoryTree::from_arena(arena))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mimedb::MimeDb;
    use std::path::PathBuf;

    use super::super::source::FileSource;
    use super::super::tree::CategoryTree;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn mimedb() -> MimeDb {
        MimeDb::load(&[fixtures().join("mime")]).expect("load mime db")
    }

    fn tree() -> CategoryTree {
        let defaults = FileSource::new(fixtures().join("categories/categories.toml"));
        let overrides = FileSource::new(fixtures().join("categories/overrides.toml"));
        build(&defaults, &overrides, &mimedb()).expect("build tree")
    }

    fn types_at(t: &CategoryTree, path: &str) -> Vec<String> {
        let id = t.node_by_path(path).unwrap_or_else(|| panic!("no node {path}"));
        t.types(id).iter().map(|m| m.to_string()).collect()
    }

    #[test]
    fn interior_node_owns_types() {
        // Media is an interior node (it has subcategories) yet owns a type.
        let t = tree();
        assert_eq!(types_at(&t, "Media"), vec!["application/ogg"]);
        assert!(!t.subcategories(t.node_by_path("Media").unwrap()).is_empty());
    }

    #[test]
    fn ancestors_are_auto_vivified() {
        // Media.Video / Media.Audio imply Media; all three exist as nodes.
        let t = tree();
        assert!(t.node_by_path("Media").is_some());
        assert!(t.node_by_path("Media.Video").is_some());
        assert!(t.node_by_path("Media.Audio").is_some());
        let subs = t.subcategories(t.node_by_path("Media").unwrap());
        let sub_paths: Vec<String> = subs.iter().map(|&id| t.path(id)).collect();
        assert_eq!(sub_paths, vec!["Media.Audio".to_string(), "Media.Video".to_string()]);
    }

    #[test]
    fn override_supersedes_default() {
        // text/html is placed under Documents in defaults, moved to Web by
        // overrides. After merge it lives ONLY under Web.
        let t = tree();
        assert_eq!(types_at(&t, "Web"), vec!["text/html"]);
        assert_eq!(types_at(&t, "Documents"), vec!["application/pdf"]);
    }

    #[test]
    fn aliases_are_canonicalized() {
        // defaults lists image/jpg (an alias); it canonicalizes to image/jpeg.
        let t = tree();
        assert_eq!(types_at(&t, "Images"), vec!["image/png", "image/jpeg"]);
    }

    #[test]
    fn unplaced_types_fall_to_other() {
        // Of the 11 fixture mime types, these are never placed by the TOML:
        // text/plain, application/xml, image/svg+xml, application/octet-stream.
        // Other is flat and sorted.
        let t = tree();
        assert_eq!(
            types_at(&t, "Other"),
            vec![
                "application/octet-stream",
                "application/xml",
                "image/svg+xml",
                "text/plain",
            ]
        );
    }

    #[test]
    fn tree_is_total_over_all_types() {
        // Every type in the MIME universe appears under exactly one root subtree.
        let db = mimedb();
        let t = tree();
        let mut covered: BTreeSet<String> = BTreeSet::new();
        for root in t.roots() {
            for m in t.types_under(root) {
                covered.insert(m.to_string());
            }
        }
        for ty in db.all_types() {
            let canon = db.canonicalize(ty).to_string();
            assert!(covered.contains(&canon), "type {canon} not covered by the tree");
        }
    }

    #[test]
    fn roots_exclude_subcategories() {
        let t = tree();
        let root_paths: BTreeSet<String> = t.roots().iter().map(|&id| t.path(id)).collect();
        let expected: BTreeSet<String> = ["Documents", "Images", "Media", "Other", "Web"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(root_paths, expected);
    }
}
