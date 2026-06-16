//! The minimal arena model for the curated category tree. The tree's shape
//! lives in exactly ONE place — each node's `supercategory`. Everything else
//! (dotted path, child lists, roots) is DERIVED, so nothing can drift out of
//! agreement (spec §3).
//!
//! Naming is node-domain super/sub (`supercategory` / `subcategories`),
//! deliberately NOT the mimetype-domain super/ancestor terms used by `mimedb`,
//! and NOT family terms like parent/children (spec §2).

use crate::types::MimeType;

/// Index into the arena. This *is* the node id (defined once, never duplicated
/// inside the node).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct CategoryId(pub usize);

/// `types` lives on EVERY node — interior nodes own types too, not just leaves.
/// Top-level nodes have `supercategory == None`.
#[derive(Clone, Debug)]
pub struct CategoryNode {
    pub name: String,
    pub supercategory: Option<CategoryId>,
    pub types: Vec<MimeType>,
}

#[derive(Clone, Debug, Default)]
pub struct CategoryTree {
    arena: Vec<CategoryNode>,
}

impl CategoryTree {
    /// Build from a prepared arena. Callers outside the module construct trees
    /// via `merge::build`; this is the single internal constructor.
    pub(crate) fn from_arena(arena: Vec<CategoryNode>) -> Self {
        CategoryTree { arena }
    }

    pub fn node(&self, id: CategoryId) -> &CategoryNode {
        &self.arena[id.0]
    }

    pub fn name(&self, id: CategoryId) -> &str {
        &self.arena[id.0].name
    }

    /// Directly-placed types at this node (NOT recursive).
    pub fn types(&self, id: CategoryId) -> &[MimeType] {
        &self.arena[id.0].types
    }

    /// Dotted path: walk `supercategory` to the root, join names with '.'.
    /// Always terminates — the arena is acyclic by construction (supercategory
    /// is a strict dotted-prefix).
    pub fn path(&self, id: CategoryId) -> String {
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            let node = &self.arena[c.0];
            parts.push(node.name.as_str());
            cur = node.supercategory;
        }
        parts.reverse();
        parts.join(".")
    }

    /// All nodes whose `supercategory == Some(id)`, in arena order.
    pub fn subcategories(&self, id: CategoryId) -> Vec<CategoryId> {
        self.arena
            .iter()
            .enumerate()
            .filter(|(_, n)| n.supercategory == Some(id))
            .map(|(i, _)| CategoryId(i))
            .collect()
    }

    /// Top-level nodes (`supercategory == None`), in arena order.
    pub fn roots(&self) -> Vec<CategoryId> {
        self.arena
            .iter()
            .enumerate()
            .filter(|(_, n)| n.supercategory.is_none())
            .map(|(i, _)| CategoryId(i))
            .collect()
    }

    /// Resolve a dotted path to its node id, if present.
    pub fn node_by_path(&self, path: &str) -> Option<CategoryId> {
        (0..self.arena.len())
            .map(CategoryId)
            .find(|&id| self.path(id) == path)
    }

    /// Recursive union of types under a node and all its subcategories. A type
    /// has exactly one placement, so no cross-node de-duplication is needed.
    /// Order is DFS: a node's own types first, then each subcategory's subtree.
    pub fn types_under(&self, id: CategoryId) -> Vec<MimeType> {
        let mut out = Vec::new();
        self.collect_types_under(id, &mut out);
        out
    }

    fn collect_types_under(&self, id: CategoryId, out: &mut Vec<MimeType>) {
        out.extend(self.arena[id.0].types.iter().cloned());
        for child in self.subcategories(id) {
            self.collect_types_under(child, out);
        }
    }

    /// The id of the node that DIRECTLY places `t` (its single home), if any.
    /// Single-placement (spec §4) guarantees at most one match. `t` should be
    /// alias-canonicalized by the caller.
    pub fn category_of(&self, t: &MimeType) -> Option<CategoryId> {
        self.arena
            .iter()
            .position(|n| n.types.contains(t))
            .map(CategoryId)
    }

    pub fn len(&self) -> usize {
        self.arena.len()
    }

    pub fn is_empty(&self) -> bool {
        self.arena.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Hand-built arena (no TOML / MimeDb needed) to test the derived accessors
    // in isolation:
    //   Media (root)            types: [application/ogg]
    //     Media.Video           types: [video/mp4]
    //   Other (root)            types: [text/plain]
    fn sample() -> CategoryTree {
        let arena = vec![
            CategoryNode {
                name: "Media".into(),
                supercategory: None,
                types: vec![MimeType::new("application/ogg")],
            },
            CategoryNode {
                name: "Video".into(),
                supercategory: Some(CategoryId(0)),
                types: vec![MimeType::new("video/mp4")],
            },
            CategoryNode {
                name: "Other".into(),
                supercategory: None,
                types: vec![MimeType::new("text/plain")],
            },
        ];
        CategoryTree::from_arena(arena)
    }

    #[test]
    fn path_joins_supercategory_chain() {
        let t = sample();
        assert_eq!(t.path(CategoryId(0)), "Media");
        assert_eq!(t.path(CategoryId(1)), "Media.Video");
        assert_eq!(t.path(CategoryId(2)), "Other");
    }

    #[test]
    fn roots_and_subcategories_are_derived() {
        let t = sample();
        assert_eq!(t.roots(), vec![CategoryId(0), CategoryId(2)]);
        assert_eq!(t.subcategories(CategoryId(0)), vec![CategoryId(1)]);
        assert_eq!(t.subcategories(CategoryId(1)), vec![]);
    }

    #[test]
    fn node_by_path_round_trips() {
        let t = sample();
        assert_eq!(t.node_by_path("Media.Video"), Some(CategoryId(1)));
        assert_eq!(t.node_by_path("Media"), Some(CategoryId(0)));
        assert_eq!(t.node_by_path("Nope"), None);
    }

    #[test]
    fn types_under_is_recursive_union() {
        let t = sample();
        // Media owns application/ogg, and inherits video/mp4 from Media.Video.
        assert_eq!(
            t.types_under(CategoryId(0)),
            vec![MimeType::new("application/ogg"), MimeType::new("video/mp4")]
        );
        // a leaf returns only its own types
        assert_eq!(t.types_under(CategoryId(1)), vec![MimeType::new("video/mp4")]);
    }

    #[test]
    fn category_of_finds_the_placing_node() {
        let t = sample();
        assert_eq!(t.category_of(&MimeType::new("video/mp4")), Some(CategoryId(1)));
        assert_eq!(t.category_of(&MimeType::new("application/ogg")), Some(CategoryId(0)));
        assert_eq!(t.category_of(&MimeType::new("text/plain")), Some(CategoryId(2)));
        assert_eq!(t.category_of(&MimeType::new("nope/none")), None);
    }

    #[test]
    fn direct_types_are_not_recursive() {
        let t = sample();
        assert_eq!(t.types(CategoryId(0)), &[MimeType::new("application/ogg")]);
        assert_eq!(t.len(), 3);
        assert!(!t.is_empty());
    }
}
