# madft Plan 2 — Categories Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the curated **category tree** — a minimal arena model, a layered `Source` loader for the TOML category files, and the `defaults ← overrides ← Other` merge that is total over `mimedb.all_types()`.

**Architecture:** A new `categories` module, laid out as a submodule directory (`src/categories/{mod,tree,source,merge}.rs`) so its three jobs stay focused: `tree` is the arena data structure (id, name, supercategory, types) with everything else *derived*; `source` is the `Source` trait + `FileSource` TOML loader (generic `toml::Table` walk for precise load errors); `merge` layers the two sources and sweeps unplaced types into a flat `Other` node. Builds on Plan 1's `MimeType`, `Error`, and `MimeDb` (for alias canonicalization + the type universe).

**Tech Stack:** Rust (edition 2024), `thiserror` (from Plan 1), new dep `toml = "0.8"` (resolves to 0.8.23; `serde` pulled transitively, but the loader uses the generic `toml::Table` API — no `#[derive(Deserialize)]` structs). Tests use committed fixtures under `tests/fixtures/`, located via `env!("CARGO_MANIFEST_DIR")`, reusing Plan 1's `tests/fixtures/mime/`.

**Plan series:** Plan 1 (facts) ✅ → Plan 2 (categories: tree + TOML source + merge, this doc) → Plan 3 (engine + writer + CLI + golden integration).

**Spec:** `docs/superpowers/specs/2026-06-15-madft-design.md`. This plan implements the `categories` module row of §3, the category arena core types of §3, the entire §4 layered-merge model (single-placement, override-supersede, totality, auto-vivified ancestors), and the category-name charset / `DuplicatePlacement` / `Parse` invariants of §2 and §7.

**Design decisions locked with the author (2026-06-15):**
1. **Submodule directory** layout (not a single `categories.rs`).
2. **Generic `toml::Table` walk** (not serde-derive structs) — full control over `DuplicatePlacement` / `InvalidCategoryName` / `Parse` messages.
3. **Auto-vivify ancestors** — a file listing `["Media.Video"]` without `["Media"]` auto-creates the `Media` node (empty `types`) from the dotted-path prefix.

---

## File structure (this plan)

- `Cargo.toml` — add `toml = "0.8"`.
- `src/lib.rs` — add `pub mod categories;`.
- `src/categories/mod.rs` — module facade: declares the three submodules, re-exports the public surface.
- `src/categories/tree.rs` — `CategoryId`, `CategoryNode`, `CategoryTree` (arena + derived accessors: `path`, `subcategories`, `roots`, `node_by_path`, `types`, `types_under`).
- `src/categories/source.rs` — `Source` trait, `CategorySpec`, `FileSource`, `parse_categories` (TOML walk + charset + single-placement validation).
- `src/categories/merge.rs` — `build(defaults, overrides, mimedb)`: the layered merge + `Other` sweep.
- `tests/fixtures/categories/categories.toml` — fixture defaults layer.
- `tests/fixtures/categories/overrides.toml` — fixture overrides layer (re-places a type to prove supersede).
- Reused: `tests/fixtures/mime/{types,subclasses,aliases}` (from Plan 1) for `MimeDb` canonicalization + the `Other` sweep universe.

**Key types & signatures (defined once, used consistently across tasks):**

- `CategoryId(pub usize)` — derives `Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug`.
- `CategoryNode { name: String, supercategory: Option<CategoryId>, types: Vec<MimeType> }` — derives `Clone, Debug`.
- `CategoryTree { arena: Vec<CategoryNode> }` — derives `Clone, Debug, Default`; built via `pub(crate) from_arena(Vec<CategoryNode>)`.
  - Accessors: `node(id) -> &CategoryNode`, `name(id) -> &str`, `types(id) -> &[MimeType]`, `path(id) -> String`, `subcategories(id) -> Vec<CategoryId>`, `roots() -> Vec<CategoryId>`, `node_by_path(&str) -> Option<CategoryId>`, `types_under(id) -> Vec<MimeType>`, `len() -> usize`, `is_empty() -> bool`.
- `CategorySpec { path: String, types: Vec<MimeType> }` — derives `Clone, Debug, PartialEq, Eq`.
- `trait Source { fn load(&self) -> Result<Vec<CategorySpec>>; }`.
- `FileSource { path: PathBuf }` — `FileSource::new(PathBuf)`.
- `parse_categories(content: &str, where_: &str) -> Result<Vec<CategorySpec>>`.
- `merge::build(defaults: &dyn Source, overrides: &dyn Source, mimedb: &MimeDb) -> Result<CategoryTree>`; `pub const OTHER: &str = "Other";`.

---

### Task 1: Scaffold the `categories` module

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/categories/mod.rs`
- Create: `src/categories/tree.rs`
- Create: `src/categories/source.rs`
- Create: `src/categories/merge.rs`

- [ ] **Step 1: Add the `toml` dependency**

Run: `cargo add toml@0.8`
Expected: `Cargo.toml` gains `toml = "0.8"` under `[dependencies]`; `Cargo.lock` updates (toml 0.8.23). If `cargo add` is unavailable, manually add this line under `[dependencies]` in `Cargo.toml` (below `thiserror = "2"`):

```toml
toml = "0.8"
```

- [ ] **Step 2: Declare the module in `src/lib.rs`**

Add `pub mod categories;` to the module list. After the edit `src/lib.rs` reads:

```rust
//! madft — inspect and set XDG default applications.
//! Plan 1 delivers the read-only facts layer.

pub mod types;
pub mod error;
pub mod paths;
pub mod mimedb;
pub mod appindex;
pub mod defaults;
pub mod categories;
```

- [ ] **Step 3: Create `src/categories/mod.rs`** (module declarations only; re-exports are added by later tasks as the items come into existence, so each task ends compiling)

```rust
//! The curated category tree: a human navigation overlay that is TOTAL over the
//! MIME universe. Built by layering `defaults` (shared) under `overrides`
//! (personal), then sweeping any still-unplaced type into a flat `Other` node.
//!
//! This is the *navigation* tree — distinct from the freedesktop subclass DAG
//! in `mimedb`. The two are never conflated (see spec §2).

mod merge;
mod source;
mod tree;
```

- [ ] **Step 4: Create the three submodule stub files**

Create each with a single placeholder line `// implemented in a later task`:
`src/categories/tree.rs`, `src/categories/source.rs`, `src/categories/merge.rs`.

- [ ] **Step 5: Verify it builds**

Run: `cargo build`
Expected: compiles. (Warnings about empty modules are fine; clippy is not gated until Task 4.)

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/categories/
git commit -m "scaffold: categories module skeleton + toml dep"
```

---

### Task 2: `tree.rs` — the arena model

**Files:**
- Modify: `src/categories/tree.rs`
- Modify: `src/categories/mod.rs`

- [ ] **Step 1: Write the implementation** (replace the entire contents of `src/categories/tree.rs`)

```rust
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
    fn direct_types_are_not_recursive() {
        let t = sample();
        assert_eq!(t.types(CategoryId(0)), &[MimeType::new("application/ogg")]);
        assert_eq!(t.len(), 3);
        assert!(!t.is_empty());
    }
}
```

- [ ] **Step 2: Add the re-export to `src/categories/mod.rs`**

Append below the `mod tree;` line:

```rust
pub use tree::{CategoryId, CategoryNode, CategoryTree};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --lib categories::tree::`
Expected: PASS (5 tests).

> Note: `from_arena` will be reported as unused by `cargo build` (it is used by tests here and by `merge` in Task 4). This warning is expected and resolves once Task 4 lands; clippy is not gated until then.

- [ ] **Step 4: Commit**

```bash
git add src/categories/tree.rs src/categories/mod.rs
git commit -m "feat(categories): arena tree model with derived accessors"
```

---

### Task 3: `source.rs` — `Source` trait + `FileSource` TOML loader

**Files:**
- Modify: `src/categories/source.rs`
- Modify: `src/categories/mod.rs`
- Create: `tests/fixtures/categories/categories.toml`
- Create: `tests/fixtures/categories/overrides.toml`

- [ ] **Step 1: Create the fixture TOML files**

`tests/fixtures/categories/categories.toml` (the `defaults` layer — note `image/jpg` is an alias that must canonicalize to `image/jpeg`, and `Media` is an interior node owning its own type):
```toml
["Media"]
types = ["application/ogg"]

["Media.Video"]
types = ["video/mp4", "video/x-matroska"]

["Media.Audio"]
types = ["audio/mpeg"]

["Documents"]
types = ["application/pdf", "text/html"]

["Images"]
types = ["image/png", "image/jpg"]
```

`tests/fixtures/categories/overrides.toml` (the `overrides` layer — re-places `text/html` from `Documents` into a new `Web` node to prove supersede):
```toml
["Web"]
types = ["text/html"]
```

- [ ] **Step 2: Write the implementation** (replace the entire contents of `src/categories/source.rs`)

```rust
//! The `Source` trait abstracts "load one layer of category placements" so the
//! file backend can later be swapped for a remote, community-maintained DB
//! (spec §4). MVP ships `FileSource` (TOML) only; the trait seam is the only
//! remote-readiness required now.
//!
//! The loader walks a generic `toml::Table` rather than deriving serde structs,
//! so it can attach precise `DuplicatePlacement` / `InvalidCategoryName` /
//! `Parse` messages (spec §7). Category keys MUST be quoted dotted paths
//! (`["Media.Video"]`); an unquoted `[Media.Video]` nests in TOML and is
//! rejected with a guiding Parse error.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::types::MimeType;

/// One category's directly-placed types, as declared by a single source layer.
/// `path` is the dotted category path (e.g. "Media.Video"); ancestors are
/// derived later (in `merge`) from its prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategorySpec {
    pub path: String,
    pub types: Vec<MimeType>,
}

/// A layer of the category tree (`defaults` or `overrides`).
pub trait Source {
    /// Load this layer's placements. An ABSENT file is NOT an error — it yields
    /// an empty layer (a user with no `overrides.toml` is the common case).
    fn load(&self) -> Result<Vec<CategorySpec>>;
}

/// Reads a single TOML file in the category grammar. Used for both the
/// `defaults` (categories.toml) and `overrides` (overrides.toml) layers.
#[derive(Clone, Debug)]
pub struct FileSource {
    pub path: PathBuf,
}

impl FileSource {
    pub fn new(path: PathBuf) -> Self {
        FileSource { path }
    }
}

impl Source for FileSource {
    fn load(&self) -> Result<Vec<CategorySpec>> {
        let content = match std::fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::Io(e)),
        };
        parse_categories(&content, &self.path.display().to_string())
    }
}

/// A category-name segment may contain only `[A-Za-z0-9 _-]` (no '.', ':', '/')
/// and may not be empty (spec §2).
fn valid_category_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '_' || c == '-')
}

/// Parse the category TOML grammar into validated specs. `where_` labels error
/// messages (the file path). Validates: well-formed TOML; category-name charset
/// on every dotted segment; single-placement (a type may not appear under two
/// different category paths within THIS file). Type strings are stored
/// as-written; alias canonicalization happens later in `merge` (which has the
/// `MimeDb`).
pub fn parse_categories(content: &str, where_: &str) -> Result<Vec<CategorySpec>> {
    let table: toml::Table = content.parse().map_err(|e: toml::de::Error| Error::Parse {
        path: where_.to_string(),
        msg: e.to_string(),
    })?;

    let mut specs = Vec::new();
    // type -> the (single) path it was placed under, for the single-placement guard
    let mut placed: HashMap<MimeType, String> = HashMap::new();

    for (path, value) in &table {
        // Every dotted segment must satisfy the name charset.
        for segment in path.split('.') {
            if !valid_category_name(segment) {
                return Err(Error::InvalidCategoryName(path.clone()));
            }
        }

        // The value must be a table whose only key is an optional `types` array.
        // A nested table (what an UNQUOTED `[Media.Video]` produces) trips the
        // unexpected-key arm with a message pointing at quoted dotted keys.
        let item = value.as_table().ok_or_else(|| Error::Parse {
            path: where_.to_string(),
            msg: format!("category '{path}' must be a table with a `types` array"),
        })?;

        let mut types = Vec::new();
        for (key, val) in item {
            if key != "types" {
                return Err(Error::Parse {
                    path: where_.to_string(),
                    msg: format!(
                        "category '{path}' has unexpected key '{key}'; use quoted dotted keys \
                         like [\"Media.Video\"] and only a `types` array"
                    ),
                });
            }
            let arr = val.as_array().ok_or_else(|| Error::Parse {
                path: where_.to_string(),
                msg: format!("`types` of '{path}' must be an array of strings"),
            })?;
            for entry in arr {
                let s = entry.as_str().ok_or_else(|| Error::Parse {
                    path: where_.to_string(),
                    msg: format!("`types` of '{path}' must contain only strings"),
                })?;
                let mime = MimeType::new(s);
                match placed.get(&mime) {
                    Some(other) if other != path => {
                        return Err(Error::DuplicatePlacement {
                            mime: mime.to_string(),
                            a: other.clone(),
                            b: path.clone(),
                        });
                    }
                    Some(_) => {} // same path repeat: dedupe below
                    None => {
                        placed.insert(mime.clone(), path.clone());
                    }
                }
                if !types.contains(&mime) {
                    types.push(mime);
                }
            }
        }
        specs.push(CategorySpec { path: path.clone(), types });
    }
    Ok(specs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn file_source_loads_specs() {
        let src = FileSource::new(fixtures().join("categories/categories.toml"));
        let specs = src.load().unwrap();
        // toml::Table iterates in sorted key order, so specs are sorted by path.
        let media_video = specs.iter().find(|s| s.path == "Media.Video").unwrap();
        assert_eq!(
            media_video.types,
            vec![MimeType::new("video/mp4"), MimeType::new("video/x-matroska")]
        );
        // image/jpg is stored as-written here (canonicalization is merge's job).
        let images = specs.iter().find(|s| s.path == "Images").unwrap();
        assert_eq!(
            images.types,
            vec![MimeType::new("image/png"), MimeType::new("image/jpg")]
        );
    }

    #[test]
    fn missing_file_is_an_empty_layer() {
        let src = FileSource::new(fixtures().join("categories/does-not-exist.toml"));
        assert_eq!(src.load().unwrap(), vec![]);
    }

    #[test]
    fn duplicate_placement_within_a_file_errors() {
        let toml = r#"
["Media"]
types = ["video/mp4"]

["Films"]
types = ["video/mp4"]
"#;
        let err = parse_categories(toml, "test").unwrap_err();
        match err {
            Error::DuplicatePlacement { mime, .. } => assert_eq!(mime, "video/mp4"),
            other => panic!("expected DuplicatePlacement, got {other:?}"),
        }
    }

    #[test]
    fn same_path_repeat_is_deduped_not_an_error() {
        let toml = r#"
["Media"]
types = ["video/mp4", "video/mp4"]
"#;
        let specs = parse_categories(toml, "test").unwrap();
        assert_eq!(specs[0].types, vec![MimeType::new("video/mp4")]);
    }

    #[test]
    fn invalid_category_name_errors() {
        // ':' is forbidden in a category name.
        let toml = "[\"Bad:Name\"]\ntypes = [\"video/mp4\"]\n";
        let err = parse_categories(toml, "test").unwrap_err();
        assert!(matches!(err, Error::InvalidCategoryName(_)));
    }

    #[test]
    fn malformed_toml_is_a_parse_error() {
        let err = parse_categories("this is not = = toml [[[", "test").unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }

    #[test]
    fn unquoted_nested_key_is_rejected() {
        // Unquoted [Media.Video] nests as Media -> Video, so the Media table has
        // an unexpected key 'Video' instead of `types`.
        let toml = "[Media.Video]\ntypes = [\"video/mp4\"]\n";
        let err = parse_categories(toml, "test").unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }
}
```

- [ ] **Step 3: Add the re-export to `src/categories/mod.rs`**

Append below the `pub use tree::...` line:

```rust
pub use source::{parse_categories, CategorySpec, FileSource, Source};
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib categories::source::`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add src/categories/source.rs src/categories/mod.rs tests/fixtures/categories/
git commit -m "feat(categories): Source trait + FileSource TOML loader"
```

---

### Task 4: `merge.rs` — the layered merge + `Other` sweep

**Files:**
- Modify: `src/categories/merge.rs`
- Modify: `src/categories/mod.rs`

- [ ] **Step 1: Write the implementation** (replace the entire contents of `src/categories/merge.rs`)

```rust
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
```

- [ ] **Step 2: Add the re-export to `src/categories/mod.rs`**

Append below the `pub use source::...` line:

```rust
pub use merge::{build, OTHER};
```

- [ ] **Step 3: Run the module tests**

Run: `cargo test --lib categories::merge::`
Expected: PASS (7 tests).

- [ ] **Step 4: Run the FULL suite + clippy (required gate)**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all tests PASS (Plan 1's 17 + this plan's 19 = 36 lib tests); no clippy warnings. If clippy flags anything, fix it minimally inline and re-run until clean. (Edition 2024 is valid on this toolchain — do not change it.)

- [ ] **Step 5: Commit**

```bash
git add src/categories/merge.rs src/categories/mod.rs
git commit -m "feat(categories): layered merge with Other sweep + totality"
```

---

## Plan 2 Self-Review (completed during authoring)

- **Spec coverage:**
  - §3 `categories` module row (`tree()`, `types_under`, `subcategories`, `node_by_path`, `path`, `roots`) → Tasks 2 & 4 (`build` is `tree()`).
  - §3 arena core types (`CategoryId`, `CategoryNode`, `CategoryTree`; types on every node; shape in one place) → Task 2.
  - §4 layered merge (`defaults ← overrides ← Other`, override-supersede, totality, interior nodes own types, auto-vivified ancestors, acyclic-by-prefix) → Task 4.
  - §4 TOML grammar (table per dotted path, `types` array, same grammar for both files) → Task 3.
  - §2 / §7 invariants: category-name charset (`InvalidCategoryName`), single-placement within a file (`DuplicatePlacement`), malformed file (`Parse`) → Task 3. Alias canonicalization of placed types → Task 4. Totality over `all_types()` → Task 4.
  - `Source` trait seam for a future `RemoteSource` (trait only, file backend built) → Task 3.
- **Out of scope (Plan 3, seams noted):** `Other:<mimetype>` display rendering, `path`↔CLI notation with `:`/`/` delimiters, `comment(t)`, the `engine`/`writer`/`cli`. This plan stops at the in-memory `CategoryTree`.
- **Placeholder scan:** none — every step ships complete code, fixtures, and exact commands.
- **Type consistency:** `CategoryId`/`CategoryNode`/`CategoryTree` (+ accessors `path`/`subcategories`/`roots`/`node_by_path`/`types`/`types_under`/`name`/`node`/`len`/`is_empty`), `CategorySpec{path,types}`, `Source::load`, `FileSource::new`, `parse_categories(content, where_)`, `build(defaults, overrides, mimedb)`, `OTHER` are used identically across Tasks 2–4 and against Plan 1's `MimeType::new`/`MimeDb::{load,all_types,canonicalize}` and `Error::{InvalidCategoryName,DuplicatePlacement,Parse,Io}`.
- **Determinism note:** `toml::Table` iterates in sorted key order and arena ids are assigned over a `BTreeSet` of paths, so node ordering, `roots()`, `subcategories()`, and `Other` contents are all deterministic — every assertion above is stable.

## Done criteria for Plan 2

`cargo test` green (36 lib tests), `cargo clippy --all-targets -- -D warnings` clean, and the categories library can: load a TOML layer into validated `CategorySpec`s (rejecting bad names, duplicate placements, and malformed/nested TOML), build a `CategoryTree` that layers `overrides` over `defaults`, auto-vivifies ancestor nodes, canonicalizes aliased placements, and is total over `mimedb.all_types()` via a flat `Other` node — all against injectable fixture roots. Plan 3 builds the `engine` + `writer` + `cli` on top of this tree and Plan 1's facts.
