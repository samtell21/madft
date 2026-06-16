# madft Plan 3 — Engine + Writer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the orchestration `engine` (the 7 operations `ls`/`types`/`info`/`apps`/`set`/`unset`/`get` over the facts + category tree, returning structured results) and the `writer` (a pure `apply` transform plus an atomic, backed-up I/O wrapper over `~/.config/mimeapps.list`).

**Architecture:** Two new flat modules (matching Plan 1's per-file layout): `src/writer.rs` and `src/engine.rs`. The writer's `apply(content, edits)` is a pure string→string transform that edits ONLY `[Default Applications]` and round-trips everything else verbatim; `write_user_defaults` wraps it with read→apply→(backup `.bak`)→atomic temp+rename, and is idempotent (no write if content is unchanged). The engine holds the Plan 1 facts (`MimeDb`, `AppIndex`, `Defaults`) plus the Plan 2 `CategoryTree`, all derived from an injectable `Roots`; each operation returns a `#[derive(Serialize)]` result struct so Plan 4's CLI can render human text or the stable `--json` schema. No CLI in this plan.

**Tech Stack:** Rust (edition 2024), `thiserror` + `toml` (existing), new dep `serde = { version = "1", features = ["derive"] }` (resolves to 1.0.228) for the result-struct derives. `serde_json` and `clap` are NOT added here — they belong to Plan 4 (the CLI). Tests use committed fixtures under `tests/fixtures/`; write-path tests use disposable directories under `std::env::temp_dir()`.

**Design decisions locked with the author (2026-06-15):**
1. **Split:** this plan is `engine` + `writer` only (library); `cli` + golden integration is Plan 4.
2. **`comment(t)` deferred:** `TypeInfo.comment` is always `None` for now (spec §9 best-effort/lazy). The field stays in the schema so a later reader is non-breaking.
3. **serde derive structs:** engine result types derive `Serialize`; actual JSON serialization happens in Plan 4.

**Plan series:** Plan 1 (facts) ✅ → Plan 2 (categories) ✅ → Plan 3 (engine + writer, this doc) → Plan 4 (cli + golden integration).

**Spec:** `docs/superpowers/specs/2026-06-15-madft-design.md`. Implements the `writer` and `engine` module rows of §3, `TypeInfo` (§3, minus deferred `comment`), the operation semantics of §5 (the data, not the rendering), the write-safety rules of §6, and the error semantics of §7 (`UnknownPath`, `UnknownApp`, `AppHandlesNothingUnderUmbrella`; partial coverage is success).

---

## File structure (this plan)

- `Cargo.toml` — add `serde = { version = "1", features = ["derive"] }`.
- `src/lib.rs` — add `pub mod writer;` and `pub mod engine;`.
- `src/writer.rs` — `Edit` enum, pure `apply`, atomic+backup `write_user_defaults`.
- `src/engine.rs` — `Engine` (facts + tree bundle), the 7 operations, and their `Serialize` result structs.
- `tests/fixtures/engine/madft/categories.toml` — engine-layer category defaults (places video/audio/images all UNDER `Media`, so the mpv-in-Media skip scenario exists).
- `tests/fixtures/engine/config/madft/overrides.toml` — engine-layer overrides (moves `text/html` to `Web`).
- `tests/fixtures/engine/config/mimeapps.list` — engine-layer current defaults.
- Reused via `data_dirs = [tests/fixtures]`: Plan 1's `tests/fixtures/mime/` and `tests/fixtures/applications/`.

**Engine fixture root wiring (used by every engine test):**
```rust
Roots {
    data_home: fixtures().join("engine"),   // categories.toml at engine/madft/, apps dir absent (falls through)
    data_dirs: vec![fixtures()],            // mime/ + applications/ from the Plan 1 fixtures
    config_home: fixtures().join("engine/config"), // mimeapps.list + madft/overrides.toml
    config_dirs: vec![],
}
```
With this, `roots.mime_dirs()` finds `tests/fixtures/mime`, `roots.app_dirs()` finds `tests/fixtures/applications`, `roots.mimeapps_files(&[])` finds `engine/config/mimeapps.list`, and the engine derives the category files as `data_home/madft/categories.toml` and `config_home/madft/overrides.toml`.

**Key types & signatures (defined once, used consistently):**
- `writer::Edit` — `Set(MimeType, DesktopId)` | `Unset(MimeType)`.
- `writer::apply(content: &str, edits: &[Edit]) -> String` (pure).
- `writer::write_user_defaults(path: &Path, edits: &[Edit]) -> Result<bool>` (returns `true` if it wrote, `false` if idempotent no-op).
- `engine::Engine` with `load(roots: &Roots, desktops: &[String]) -> Result<Self>`.
- Operations: `ls(Option<&str>) -> Result<LsResult>`, `types(&str) -> Result<Vec<String>>`, `info(&str) -> Result<TypeInfo>`, `apps(&str) -> Result<AppsResult>`, `set(&str, &str, Option<&[String]>, bool) -> Result<SetPlan>`, `unset(&str) -> Result<bool>`, `get(&str) -> Option<String>`.
- Result structs (all `#[derive(Serialize, Debug)]`): `LeafType`, `LsResult`, `AppRef`, `TypeInfo`, `AppCoverage`, `AppsResult`, `SetPlan`.

---

### Task 1: Scaffold engine + writer modules

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/writer.rs`
- Create: `src/engine.rs`

- [ ] **Step 1: Add the serde dependency**

Run: `cargo add serde --features derive`
Expected: `Cargo.toml` gains `serde = { version = "1", features = ["derive"] }`; `Cargo.lock` updates (serde 1.0.228). If `cargo add` is unavailable, add manually under `[dependencies]`:
```toml
serde = { version = "1", features = ["derive"] }
```

- [ ] **Step 2: Declare the modules in `src/lib.rs`**

Add `pub mod writer;` and `pub mod engine;` as the last two lines. After the edit `src/lib.rs` reads:
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
pub mod writer;
pub mod engine;
```

- [ ] **Step 3: Create the two stub files**

Create each with the single line `// implemented in a later task`:
`src/writer.rs`, `src/engine.rs`.

- [ ] **Step 4: Verify it builds**

Run: `cargo build`
Expected: compiles (clippy not gated here).

- [ ] **Step 5: Commit**
```bash
git add Cargo.toml Cargo.lock src/lib.rs src/writer.rs src/engine.rs
git commit -m "scaffold: engine + writer modules + serde dep"
```

---

### Task 2: `writer.rs` — pure `apply` + atomic backed-up I/O

**Files:**
- Modify: `src/writer.rs`

- [ ] **Step 1: Write the implementation + tests** (replace the entire contents of `src/writer.rs`)

```rust
//! Mutates the user's `mimeapps.list`. `apply` is a PURE transform over file
//! content; `write_user_defaults` wraps it with a `.bak` copy and an atomic
//! temp+rename. Edits touch ONLY the `[Default Applications]` section —
//! everything else (other sections, keys, ordering, comments) round-trips
//! verbatim (spec §6). Never creates `[Added]`/`[Removed]` sections.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::types::{DesktopId, MimeType};

const DEFAULT_APPS: &str = "[Default Applications]";

/// One change to the `[Default Applications]` section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Upsert `mime=app` (replace in place if present, else insert).
    Set(MimeType, DesktopId),
    /// Remove the `mime` key if present.
    Unset(MimeType),
}

fn is_section_header(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('[') && t.ends_with(']')
}

/// The key of a `key=value` line (trimmed), or `None` for comments, blanks, and
/// section headers.
fn line_key(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') || is_section_header(t) {
        return None;
    }
    t.split_once('=').map(|(k, _)| k.trim())
}

/// Apply `edits` to `mimeapps.list` `content`, returning the new content. Pure:
/// no I/O. Preserves all sections/keys/order/comments; edits only
/// `[Default Applications]`. Creates that section (or a minimal file) if absent.
pub fn apply(content: &str, edits: &[Edit]) -> String {
    let lines: Vec<&str> = content.lines().collect();

    let header_idx = lines.iter().position(|l| l.trim() == DEFAULT_APPS);

    let mut out: Vec<String> = Vec::new();
    match header_idx {
        Some(h) => {
            for line in &lines[..=h] {
                out.push((*line).to_string());
            }
            // Section body runs to the next section header (or EOF).
            let end = lines
                .iter()
                .enumerate()
                .skip(h + 1)
                .find(|(_, l)| is_section_header(l))
                .map(|(j, _)| j)
                .unwrap_or(lines.len());
            let mut section: Vec<String> =
                lines[h + 1..end].iter().map(|s| (*s).to_string()).collect();
            apply_to_section(&mut section, edits);
            out.extend(section);
            for line in &lines[end..] {
                out.push((*line).to_string());
            }
        }
        None => {
            for line in &lines {
                out.push((*line).to_string());
            }
            // Separate any prior content from the new section with one blank line.
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(DEFAULT_APPS.to_string());
            for edit in edits {
                if let Edit::Set(m, d) = edit {
                    out.push(format!("{}={}", m.as_str(), d.as_str()));
                }
            }
        }
    }

    let mut result = out.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }
    result
}

/// Apply edits within a single section body (lines between the header and the
/// next header/EOF). `Set` replaces in place or inserts; `Unset` deletes.
fn apply_to_section(section: &mut Vec<String>, edits: &[Edit]) {
    for edit in edits {
        match edit {
            Edit::Set(mime, app) => {
                let key = mime.as_str();
                let new_line = format!("{}={}", key, app.as_str());
                if let Some(pos) = section.iter().position(|l| line_key(l) == Some(key)) {
                    section[pos] = new_line;
                } else {
                    // Insert after the last non-blank line (before trailing blanks).
                    let at = section
                        .iter()
                        .rposition(|l| !l.trim().is_empty())
                        .map(|p| p + 1)
                        .unwrap_or(section.len());
                    section.insert(at, new_line);
                }
            }
            Edit::Unset(mime) => {
                let key = mime.as_str();
                section.retain(|l| line_key(l) != Some(key));
            }
        }
    }
}

fn bak_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(".bak");
    PathBuf::from(p)
}

/// Write `content` to `path` atomically: temp file in the same directory →
/// fsync → rename over the target (spec §6).
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let fname = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mimeapps.list");
    let tmp = dir.join(format!(".{fname}.madft.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read → apply → write the user defaults file. Idempotent: if applying the
/// edits does not change the content, nothing is written (returns `Ok(false)`).
/// Backs the file up to `<path>.bak` before writing. Creates the file (minimal)
/// if it does not exist. Returns `Ok(true)` if a write occurred.
pub fn write_user_defaults(path: &Path, edits: &[Edit]) -> Result<bool> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Error::Io(e)),
    };
    let updated = apply(&existing, edits);
    if updated == existing {
        return Ok(false);
    }
    if !existing.is_empty() {
        std::fs::copy(path, bak_path(path))?;
    }
    atomic_write(path, &updated)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(m: &str, a: &str) -> Edit {
        Edit::Set(MimeType::new(m), DesktopId::new(a))
    }

    #[test]
    fn upsert_replaces_in_place() {
        let before = "[Default Applications]\nvideo/mp4=old.desktop\n";
        let after = apply(before, &[set("video/mp4", "mpv")]);
        assert_eq!(after, "[Default Applications]\nvideo/mp4=mpv.desktop\n");
    }

    #[test]
    fn upsert_appends_new_key() {
        let before = "[Default Applications]\nvideo/mp4=mpv.desktop\n";
        let after = apply(before, &[set("audio/mpeg", "mpv")]);
        assert_eq!(
            after,
            "[Default Applications]\nvideo/mp4=mpv.desktop\naudio/mpeg=mpv.desktop\n"
        );
    }

    #[test]
    fn unset_removes_key() {
        let before = "[Default Applications]\nvideo/mp4=mpv.desktop\ntext/html=ff.desktop\n";
        let after = apply(before, &[Edit::Unset(MimeType::new("video/mp4"))]);
        assert_eq!(after, "[Default Applications]\ntext/html=ff.desktop\n");
    }

    #[test]
    fn preserves_other_sections_and_comments() {
        let before = "# my file\n[Default Applications]\nvideo/mp4=mpv.desktop\n\n\
                      [Added Associations]\nvideo/mp4=mpv.desktop;vlc.desktop\n";
        let after = apply(before, &[set("text/html", "ff")]);
        // The comment, the blank line, and the [Added Associations] section all survive.
        assert!(after.contains("# my file\n"));
        assert!(after.contains("[Added Associations]\nvideo/mp4=mpv.desktop;vlc.desktop\n"));
        // The new key landed inside [Default Applications], before the blank line.
        assert!(after.contains("video/mp4=mpv.desktop\ntext/html=ff.desktop\n\n[Added Associations]"));
    }

    #[test]
    fn idempotent_when_value_unchanged() {
        let before = "[Default Applications]\nvideo/mp4=mpv.desktop\n";
        let after = apply(before, &[set("video/mp4", "mpv")]);
        assert_eq!(after, before);
    }

    #[test]
    fn creates_section_when_absent() {
        let after = apply("", &[set("video/mp4", "mpv")]);
        assert_eq!(after, "[Default Applications]\nvideo/mp4=mpv.desktop\n");
    }

    #[test]
    fn creates_section_after_existing_unrelated_content() {
        let before = "[Added Associations]\nvideo/mp4=vlc.desktop\n";
        let after = apply(before, &[set("video/mp4", "mpv")]);
        assert_eq!(
            after,
            "[Added Associations]\nvideo/mp4=vlc.desktop\n\n[Default Applications]\nvideo/mp4=mpv.desktop\n"
        );
    }

    // --- I/O wrapper (uses a disposable temp dir) ---

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("madft-writer-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_user_defaults_backs_up_and_is_idempotent() {
        let dir = temp_dir("backup");
        let path = dir.join("mimeapps.list");
        std::fs::write(&path, "[Default Applications]\nvideo/mp4=old.desktop\n").unwrap();

        // First write changes the value -> writes + backs up.
        let wrote = write_user_defaults(&path, &[set("video/mp4", "mpv")]).unwrap();
        assert!(wrote);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[Default Applications]\nvideo/mp4=mpv.desktop\n"
        );
        // The .bak holds the pre-write content.
        let bak = path.with_file_name("mimeapps.list.bak");
        assert_eq!(
            std::fs::read_to_string(&bak).unwrap(),
            "[Default Applications]\nvideo/mp4=old.desktop\n"
        );

        // Second identical write is a no-op (idempotent).
        let wrote_again = write_user_defaults(&path, &[set("video/mp4", "mpv")]).unwrap();
        assert!(!wrote_again);
    }

    #[test]
    fn write_user_defaults_creates_missing_file() {
        let dir = temp_dir("create");
        let path = dir.join("mimeapps.list");
        assert!(!path.exists());
        let wrote = write_user_defaults(&path, &[set("video/mp4", "mpv")]).unwrap();
        assert!(wrote);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[Default Applications]\nvideo/mp4=mpv.desktop\n"
        );
        // No backup for a file that didn't exist.
        assert!(!path.with_file_name("mimeapps.list.bak").exists());
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib writer::`
Expected: PASS (9 tests).

- [ ] **Step 3: Commit**
```bash
git add src/writer.rs
git commit -m "feat(writer): pure apply + atomic backed-up mimeapps.list writes"
```

---

### Task 3: `engine.rs` — bundle + read operations (`ls`, `types`, `info`, `apps`, `get`)

**Files:**
- Modify: `src/engine.rs`
- Create: `tests/fixtures/engine/madft/categories.toml`
- Create: `tests/fixtures/engine/config/madft/overrides.toml`
- Create: `tests/fixtures/engine/config/mimeapps.list`

- [ ] **Step 1: Create the engine fixture files**

`tests/fixtures/engine/madft/categories.toml` (places video/audio/images all UNDER `Media`, so a later `set Media mpv` has images to skip):
```toml
["Media"]
types = ["application/ogg"]

["Media.Video"]
types = ["video/mp4", "video/x-matroska"]

["Media.Audio"]
types = ["audio/mpeg"]

["Media.Images"]
types = ["image/png", "image/jpeg"]
```

`tests/fixtures/engine/config/madft/overrides.toml`:
```toml
["Web"]
types = ["text/html"]
```

`tests/fixtures/engine/config/mimeapps.list`:
```
[Default Applications]
video/mp4=mpv.desktop
text/html=org.qutebrowser.qutebrowser.desktop
```

- [ ] **Step 2: Write the engine bundle + read operations** (replace the entire contents of `src/engine.rs`)

```rust
//! Orchestrates the facts (`MimeDb`, `AppIndex`, `Defaults`) and the category
//! `CategoryTree` into the user-facing operations (spec §5). Every operation
//! returns a `Serialize` result struct; rendering (human vs `--json`) is Plan
//! 4's CLI. All inputs derive from an injectable `Roots`, so tests run against
//! fixture trees with zero host reliance.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::appindex::AppIndex;
use crate::categories::{self, CategoryTree, FileSource};
use crate::defaults::Defaults;
use crate::error::{Error, Result};
use crate::mimedb::MimeDb;
use crate::paths::Roots;
use crate::types::{DesktopId, MimeType};

/// A leaf type as shown by `ls`: the type, its current default, and how many
/// apps declare it.
#[derive(Serialize, Debug)]
pub struct LeafType {
    pub mime: String,
    pub current_default: Option<String>,
    pub applicable_count: usize,
}

/// Result of `ls`: a node's child categories (dotted paths) and direct leaf types.
#[derive(Serialize, Debug)]
pub struct LsResult {
    pub path: String,
    pub subcategories: Vec<String>,
    pub types: Vec<LeafType>,
}

/// A bare app reference (id + display name).
#[derive(Serialize, Debug)]
pub struct AppRef {
    pub id: String,
    pub name: String,
}

/// Result of `info` for one mimetype. `comment` is deferred (always `None` in
/// the MVP — spec §9). The mimetype is alias-canonicalized.
#[derive(Serialize, Debug)]
pub struct TypeInfo {
    pub mime: String,
    pub comment: Option<String>,
    pub current_default: Option<String>,
    pub applicable_count: usize,
    pub ancestor_types: Vec<String>,
    pub applicable_apps: Vec<AppRef>,
}

/// One app's coverage of an umbrella: which of the umbrella's types it declares.
#[derive(Serialize, Debug)]
pub struct AppCoverage {
    pub id: String,
    pub name: String,
    pub coverage: usize,
    pub declared_types: Vec<String>,
}

/// Result of `apps`: the umbrella's types and the apps that declare any of them,
/// sorted by coverage (descending), then id.
#[derive(Serialize, Debug)]
pub struct AppsResult {
    pub target: String,
    pub types: Vec<String>,
    pub apps: Vec<AppCoverage>,
}

/// Plan produced by `set`: which umbrella types will be / were set, and which
/// were skipped because the app does not declare them (informational, NOT an
/// error — partial coverage is success, spec §7).
#[derive(Serialize, Debug)]
pub struct SetPlan {
    pub app: String,
    pub target: String,
    pub set_types: Vec<String>,
    pub skipped_types: Vec<String>,
    pub dry_run: bool,
    pub written: bool,
}

pub struct Engine {
    roots: Roots,
    mimedb: MimeDb,
    appindex: AppIndex,
    defaults: Defaults,
    tree: CategoryTree,
}

impl Engine {
    /// Build the engine from an injectable `Roots`. `desktops` is the lowercased
    /// `$XDG_CURRENT_DESKTOP` list (may be empty) used for `mimeapps.list`
    /// precedence. Category files are derived per spec §4:
    /// `data_home/madft/categories.toml` and `config_home/madft/overrides.toml`.
    pub fn load(roots: &Roots, desktops: &[String]) -> Result<Self> {
        let mimedb = MimeDb::load(&roots.mime_dirs())?;
        let appindex = AppIndex::load(roots)?;
        let defaults = Defaults::load(&roots.mimeapps_files(desktops))?;
        let cat_defaults = FileSource::new(roots.data_home.join("madft/categories.toml"));
        let cat_overrides = FileSource::new(roots.config_home.join("madft/overrides.toml"));
        let tree = categories::build(&cat_defaults, &cat_overrides, &mimedb)?;
        Ok(Engine {
            roots: roots.clone(),
            mimedb,
            appindex,
            defaults,
            tree,
        })
    }

    fn leaf_type(&self, t: &MimeType) -> LeafType {
        LeafType {
            mime: t.to_string(),
            current_default: self.defaults.current_default(t).map(|d| d.to_string()),
            applicable_count: self.appindex.apps_for_type(t).len(),
        }
    }

    /// Resolve a `<PATH|mimetype>` target. A target containing '/' is a mimetype
    /// (umbrella = just that canonical type); otherwise it is a category path
    /// (umbrella = its recursive types). `'/'` is the mimetype's own delimiter
    /// and never appears in a category name (spec §2).
    fn resolve_umbrella(&self, target: &str) -> Result<(String, Vec<MimeType>)> {
        if target.contains('/') {
            let canon = self.mimedb.canonicalize(&MimeType::new(target));
            Ok((canon.to_string(), vec![canon]))
        } else {
            let id = self
                .tree
                .node_by_path(target)
                .ok_or_else(|| Error::UnknownPath(target.to_string()))?;
            Ok((self.tree.path(id), self.tree.types_under(id)))
        }
    }

    /// `ls [PATH]`: child categories + direct leaf types at a node. With no
    /// PATH, lists the tree's roots (no direct types at the virtual root).
    pub fn ls(&self, path: Option<&str>) -> Result<LsResult> {
        match path {
            None => Ok(LsResult {
                path: String::new(),
                subcategories: self.tree.roots().iter().map(|&id| self.tree.path(id)).collect(),
                types: Vec::new(),
            }),
            Some(p) => {
                let id = self
                    .tree
                    .node_by_path(p)
                    .ok_or_else(|| Error::UnknownPath(p.to_string()))?;
                Ok(LsResult {
                    path: self.tree.path(id),
                    subcategories: self
                        .tree
                        .subcategories(id)
                        .iter()
                        .map(|&c| self.tree.path(c))
                        .collect(),
                    types: self.tree.types(id).iter().map(|t| self.leaf_type(t)).collect(),
                })
            }
        }
    }

    /// `types <PATH>`: all mimetypes under the umbrella (recursive, canonicalized).
    pub fn types(&self, path: &str) -> Result<Vec<String>> {
        let id = self
            .tree
            .node_by_path(path)
            .ok_or_else(|| Error::UnknownPath(path.to_string()))?;
        Ok(self.tree.types_under(id).iter().map(|t| t.to_string()).collect())
    }

    /// `info <mimetype>`: canonical name, current default, applicable apps, and
    /// the inherit-if-unset `ancestor_types` chain. `comment` is deferred.
    pub fn info(&self, mime: &str) -> Result<TypeInfo> {
        let canon = self.mimedb.canonicalize(&MimeType::new(mime));
        let applicable_apps: Vec<AppRef> = self
            .appindex
            .apps_for_type(&canon)
            .iter()
            .map(|a| AppRef { id: a.id.to_string(), name: a.name.clone() })
            .collect();
        Ok(TypeInfo {
            mime: canon.to_string(),
            comment: None,
            current_default: self.defaults.current_default(&canon).map(|d| d.to_string()),
            applicable_count: applicable_apps.len(),
            ancestor_types: self
                .mimedb
                .ancestor_types(&canon)
                .iter()
                .map(|t| t.to_string())
                .collect(),
            applicable_apps,
        })
    }

    /// `apps <PATH|mimetype>`: apps declaring any of the umbrella's types, with
    /// their coverage, sorted by coverage (desc) then id (asc).
    pub fn apps(&self, target: &str) -> Result<AppsResult> {
        let (label, umbrella) = self.resolve_umbrella(target)?;
        // app id -> the umbrella types it declares (in umbrella order).
        let mut by_app: BTreeMap<DesktopId, Vec<MimeType>> = BTreeMap::new();
        for t in &umbrella {
            for app in self.appindex.apps_for_type(t) {
                by_app.entry(app.id.clone()).or_default().push(t.clone());
            }
        }
        let mut apps: Vec<AppCoverage> = by_app
            .into_iter()
            .map(|(id, declared)| {
                let name = self.appindex.app(&id).map(|a| a.name.clone()).unwrap_or_default();
                AppCoverage {
                    id: id.to_string(),
                    name,
                    coverage: declared.len(),
                    declared_types: declared.iter().map(|t| t.to_string()).collect(),
                }
            })
            .collect();
        apps.sort_by(|a, b| b.coverage.cmp(&a.coverage).then_with(|| a.id.cmp(&b.id)));
        Ok(AppsResult {
            target: label,
            types: umbrella.iter().map(|t| t.to_string()).collect(),
            apps,
        })
    }

    /// `get <mimetype>`: the bare current default (scriptable), canonicalized.
    pub fn get(&self, mime: &str) -> Option<String> {
        let canon = self.mimedb.canonicalize(&MimeType::new(mime));
        self.defaults.current_default(&canon).map(|d| d.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn engine() -> Engine {
        let roots = Roots {
            data_home: fixtures().join("engine"),
            data_dirs: vec![fixtures()],
            config_home: fixtures().join("engine/config"),
            config_dirs: vec![],
        };
        Engine::load(&roots, &[]).expect("load engine")
    }

    #[test]
    fn ls_root_lists_category_roots() {
        let e = engine();
        let r = e.ls(None).unwrap();
        assert_eq!(r.subcategories, vec!["Media", "Other", "Web"]);
        assert!(r.types.is_empty());
    }

    #[test]
    fn ls_node_lists_subcategories_and_direct_types() {
        let e = engine();
        let r = e.ls(Some("Media")).unwrap();
        assert_eq!(r.path, "Media");
        assert_eq!(r.subcategories, vec!["Media.Audio", "Media.Images", "Media.Video"]);
        // Media owns application/ogg directly; no app declares it, no default set.
        assert_eq!(r.types.len(), 1);
        assert_eq!(r.types[0].mime, "application/ogg");
        assert_eq!(r.types[0].current_default, None);
        assert_eq!(r.types[0].applicable_count, 0);
    }

    #[test]
    fn ls_unknown_path_errors() {
        let e = engine();
        assert!(matches!(e.ls(Some("Nope")), Err(Error::UnknownPath(_))));
    }

    #[test]
    fn types_under_umbrella_is_recursive() {
        let e = engine();
        let t = e.types("Media").unwrap();
        assert_eq!(
            t,
            vec![
                "application/ogg",
                "audio/mpeg",
                "image/png",
                "image/jpeg",
                "video/mp4",
                "video/x-matroska",
            ]
        );
    }

    #[test]
    fn info_canonicalizes_alias() {
        let e = engine();
        let info = e.info("image/jpg").unwrap();
        assert_eq!(info.mime, "image/jpeg");
        assert_eq!(info.comment, None);
        assert_eq!(info.applicable_count, 1); // eog declares image/jpeg
        assert_eq!(info.applicable_apps[0].id, "eog.desktop");
    }

    #[test]
    fn info_reports_transitive_ancestors() {
        let e = engine();
        let info = e.info("image/svg+xml").unwrap();
        // svg -> application/xml -> text/plain
        assert_eq!(info.ancestor_types, vec!["application/xml", "text/plain"]);
    }

    #[test]
    fn apps_sorted_by_coverage() {
        let e = engine();
        let r = e.apps("Media").unwrap();
        let ids: Vec<&str> = r.apps.iter().map(|a| a.id.as_str()).collect();
        // mpv declares 3 (audio/mpeg, video/mp4, video/x-matroska),
        // eog declares 2 (image/png, image/jpeg), webcam declares 1 (video/mp4).
        assert_eq!(ids, vec!["mpv.desktop", "eog.desktop", "webcam.desktop"]);
        assert_eq!(r.apps[0].coverage, 3);
        assert_eq!(r.apps[1].coverage, 2);
        assert_eq!(r.apps[2].coverage, 1);
    }

    #[test]
    fn apps_for_a_mimetype_target() {
        let e = engine();
        let r = e.apps("video/mp4").unwrap();
        assert_eq!(r.target, "video/mp4");
        assert_eq!(r.types, vec!["video/mp4"]);
        // mpv and webcam both declare video/mp4 (coverage 1 each); sorted by id.
        let ids: Vec<&str> = r.apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["mpv.desktop", "webcam.desktop"]);
    }

    #[test]
    fn get_returns_current_default() {
        let e = engine();
        assert_eq!(e.get("video/mp4"), Some("mpv.desktop".to_string()));
        assert_eq!(e.get("image/png"), None);
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --lib engine::tests::`
Expected: PASS (9 tests).

- [ ] **Step 4: Commit**
```bash
git add src/engine.rs tests/fixtures/engine/
git commit -m "feat(engine): facts+tree bundle and read operations"
```

---

### Task 4: `engine.rs` — write operations (`set`, `unset`) + final gate

**Files:**
- Modify: `src/engine.rs` (APPEND only — a new `impl Engine` block and a new test module at the end of the file; do not edit existing items)

- [ ] **Step 1: Append the write-operations impl block and tests** to the END of `src/engine.rs` (after the existing `#[cfg(test)] mod tests { ... }`)

```rust
impl Engine {
    /// `set <PATH|mimetype> <app> [--types …] [--dry-run]`: set `app` as the
    /// default for exactly the umbrella types it declares. Types the app does
    /// NOT declare are reported as `skipped_types` (informational, not an error).
    /// `types_filter`, when given, restricts the action to that subset of the
    /// umbrella (each entry alias-canonicalized). Guards with
    /// `AppHandlesNothingUnderUmbrella` if the app declares none of them.
    pub fn set(
        &self,
        target: &str,
        app: &str,
        types_filter: Option<&[String]>,
        dry_run: bool,
    ) -> Result<SetPlan> {
        let (label, umbrella) = self.resolve_umbrella(target)?;
        let app_id = DesktopId::new(app);
        if self.appindex.app(&app_id).is_none() {
            return Err(Error::UnknownApp(app_id.to_string()));
        }
        let filter: Option<Vec<MimeType>> = types_filter.map(|fs| {
            fs.iter()
                .map(|s| self.mimedb.canonicalize(&MimeType::new(s.as_str())))
                .collect()
        });

        let mut set_types: Vec<MimeType> = Vec::new();
        let mut skipped: Vec<MimeType> = Vec::new();
        for t in &umbrella {
            if let Some(ref f) = filter {
                if !f.contains(t) {
                    continue; // outside the --types restriction: ignore entirely
                }
            }
            if self.appindex.declares(&app_id, t) {
                set_types.push(t.clone());
            } else {
                skipped.push(t.clone());
            }
        }

        if set_types.is_empty() {
            return Err(Error::AppHandlesNothingUnderUmbrella {
                app: app_id.to_string(),
                umbrella: label,
            });
        }

        let edits: Vec<crate::writer::Edit> = set_types
            .iter()
            .map(|t| crate::writer::Edit::Set(t.clone(), app_id.clone()))
            .collect();
        let written = if dry_run {
            false
        } else {
            crate::writer::write_user_defaults(&self.roots.user_mimeapps(), &edits)?
        };

        Ok(SetPlan {
            app: app_id.to_string(),
            target: label,
            set_types: set_types.iter().map(|t| t.to_string()).collect(),
            skipped_types: skipped.iter().map(|t| t.to_string()).collect(),
            dry_run,
            written,
        })
    }

    /// `unset <mimetype>`: remove the user default for the (canonicalized) type.
    /// Returns whether a write occurred (false if there was nothing to remove).
    pub fn unset(&self, mime: &str) -> Result<bool> {
        let canon = self.mimedb.canonicalize(&MimeType::new(mime));
        crate::writer::write_user_defaults(
            &self.roots.user_mimeapps(),
            &[crate::writer::Edit::Unset(canon)],
        )
    }
}

#[cfg(test)]
mod write_tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    /// An engine whose READS come from the committed fixtures but whose WRITE
    /// target (`config_home`) is a fresh disposable temp dir seeded with a copy
    /// of the fixture mimeapps.list.
    fn engine_with_temp_config(tag: &str) -> (Engine, PathBuf) {
        let cfg = std::env::temp_dir().join(format!("madft-engine-{tag}"));
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::copy(
            fixtures().join("engine/config/mimeapps.list"),
            cfg.join("mimeapps.list"),
        )
        .unwrap();
        let roots = Roots {
            data_home: fixtures().join("engine"),
            data_dirs: vec![fixtures()],
            config_home: cfg.clone(),
            config_dirs: vec![],
        };
        (Engine::load(&roots, &[]).unwrap(), cfg.join("mimeapps.list"))
    }

    #[test]
    fn set_dry_run_partitions_without_writing() {
        // Read-only engine is fine for a dry run (no write target touched).
        let roots = Roots {
            data_home: fixtures().join("engine"),
            data_dirs: vec![fixtures()],
            config_home: fixtures().join("engine/config"),
            config_dirs: vec![],
        };
        let e = Engine::load(&roots, &[]).unwrap();
        let plan = e.set("Media", "mpv", None, true).unwrap();
        // mpv declares video/mp4, video/x-matroska, audio/mpeg.
        assert_eq!(plan.set_types, vec!["audio/mpeg", "video/mp4", "video/x-matroska"]);
        // application/ogg + the two images are under Media but mpv doesn't declare them.
        assert_eq!(plan.skipped_types, vec!["application/ogg", "image/png", "image/jpeg"]);
        assert!(!plan.written);
        assert!(plan.dry_run);
    }

    #[test]
    fn set_guards_when_app_handles_nothing() {
        let roots = Roots {
            data_home: fixtures().join("engine"),
            data_dirs: vec![fixtures()],
            config_home: fixtures().join("engine/config"),
            config_dirs: vec![],
        };
        let e = Engine::load(&roots, &[]).unwrap();
        // nvim declares text/plain + text/html — neither is under Media.
        let err = e.set("Media", "nvim", None, false).unwrap_err();
        assert!(matches!(err, Error::AppHandlesNothingUnderUmbrella { .. }));
    }

    #[test]
    fn set_unknown_app_errors() {
        let roots = Roots {
            data_home: fixtures().join("engine"),
            data_dirs: vec![fixtures()],
            config_home: fixtures().join("engine/config"),
            config_dirs: vec![],
        };
        let e = Engine::load(&roots, &[]).unwrap();
        assert!(matches!(e.set("Media", "nope", None, false), Err(Error::UnknownApp(_))));
    }

    #[test]
    fn set_writes_backs_up_and_is_idempotent() {
        let (e, path) = engine_with_temp_config("set");
        let plan = e.set("Media", "mpv", None, false).unwrap();
        assert!(plan.written);

        let content = std::fs::read_to_string(&path).unwrap();
        // The three declared types are now defaulted to mpv...
        assert!(content.contains("audio/mpeg=mpv.desktop"));
        assert!(content.contains("video/x-matroska=mpv.desktop"));
        assert!(content.contains("video/mp4=mpv.desktop"));
        // ...and the unrelated existing default is preserved.
        assert!(content.contains("text/html=org.qutebrowser.qutebrowser.desktop"));
        // Backup of the pre-write file exists.
        assert!(path.with_file_name("mimeapps.list.bak").exists());

        // Re-running the same set writes nothing (idempotent).
        let again = e.set("Media", "mpv", None, false).unwrap();
        assert!(!again.written);
    }

    #[test]
    fn set_with_types_filter_restricts() {
        let (e, _path) = engine_with_temp_config("filter");
        let only = ["video/mp4".to_string()];
        let plan = e.set("Media", "mpv", Some(&only), true).unwrap();
        assert_eq!(plan.set_types, vec!["video/mp4"]);
        // Restriction excludes everything else, so nothing is reported skipped.
        assert!(plan.skipped_types.is_empty());
    }

    #[test]
    fn unset_removes_existing_default() {
        let (e, path) = engine_with_temp_config("unset");
        let wrote = e.unset("video/mp4").unwrap();
        assert!(wrote);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("video/mp4="));
        // Idempotent: removing again is a no-op.
        assert!(!e.unset("video/mp4").unwrap());
    }
}
```

- [ ] **Step 2: Run the engine tests**

Run: `cargo test --lib engine::`
Expected: PASS (9 read tests + 6 write tests = 15 in the engine module).

- [ ] **Step 3: Run the FULL suite + clippy (required gate)**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all tests PASS (Plan 1's 17 + Plan 2's 19 + this plan's 24 = 60 lib tests); no clippy warnings. If clippy flags anything, fix it minimally inline and re-run until clean. (Edition 2024 is valid on this toolchain — do not change it.)

- [ ] **Step 4: Commit**
```bash
git add src/engine.rs
git commit -m "feat(engine): set/unset write operations with guard + idempotency"
```

---

## Plan 3 Self-Review (completed during authoring)

- **Spec coverage:**
  - §3 `writer` row (pure `apply`; IO wrapper = atomic + `.bak`) → Task 2.
  - §3 `engine` row (`ls`/`types`/`info`/`apps`/`set`/`unset`/`get`) → Tasks 3 & 4.
  - §3 `TypeInfo` (mime, comment, current_default, applicable_count, ancestor_types) → Task 3 (`comment` deferred to `None` per agreed decision; `applicable_apps` added because §5 `info` surfaces the apps).
  - §5 operation semantics (data only; rendering is Plan 4): `ls` children+leaf annotations, `types` recursive+canonical, `info` canonical+ancestors+apps, `apps` coverage-sorted, `set` umbrella-declared-only with skipped reporting + `--types` + `--dry-run`, `unset`, `get` bare → Tasks 3 & 4.
  - §6 write safety (preserve sections/keys/order/comments, edit only `[Default Applications]`, atomic temp+fsync+rename, `.bak`, idempotent, create minimal file, never `[Added]`/`[Removed]`) → Task 2.
  - §7 errors (`UnknownPath`, `UnknownApp`, `AppHandlesNothingUnderUmbrella`; partial coverage = success) → Tasks 3 & 4.
  - §8 testing (injectable roots; writer round-trip/upsert/unset/idempotency/atomic; the mpv-in-Media logic) → Tasks 2 & 4. The full golden `--json` mpv-in-Media integration test is Plan 4.
- **Out of scope (Plan 4):** clap subcommands, human vs `--json` rendering, `serde_json`, the `{"error":{kind,message}}` envelope, exit codes, and the golden integration test. `comment(t)` and reverse alias listing remain deferred (schema-stable: `comment` is an `Option`, no alias field added).
- **Placeholder scan:** none — every step ships complete code, fixtures, and exact commands.
- **Type consistency:** `Edit::{Set,Unset}`, `apply`, `write_user_defaults` (Task 2) are used by `engine::{set,unset}` (Task 4) with matching signatures. `Engine::load`, `resolve_umbrella`, and the result structs (`LsResult`/`LeafType`/`TypeInfo`/`AppRef`/`AppsResult`/`AppCoverage`/`SetPlan`) are defined in Task 3 and extended (append-only) in Task 4. All build on Plan 1 (`MimeDb::{load,canonicalize,ancestor_types,all_types}`, `AppIndex::{load,app,apps_for_type,declares}`, `Defaults::{load,current_default}`, `Roots` fields + `mime_dirs`/`app_dirs`/`mimeapps_files`/`user_mimeapps`) and Plan 2 (`categories::build`, `CategoryTree::{node_by_path,path,roots,subcategories,types,types_under}`, `FileSource::new`).
- **Determinism:** `apps` collects into a `BTreeMap<DesktopId,_>` and sorts by (coverage desc, id asc); `declared_types` follows umbrella DFS order; `set` partitions in umbrella order. `apps_for_type`'s internal (read-dir-dependent) order is never relied upon. Write tests use uniquely-named temp dirs (parallel-safe) and never mutate committed fixtures.
- **Exact-declaration assumption (from Plan 1):** apps are assumed to declare canonical mimetypes (the fixtures do); `set`/`apps` query with the canonical umbrella type and use `AppIndex::declares` (exact). A reverse alias index on the app side is not needed for the MVP fixtures.

## Done criteria for Plan 3

`cargo test` green (60 lib tests), `cargo clippy --all-targets -- -D warnings` clean, and the library can: transform a `mimeapps.list` in memory (upsert/unset within `[Default Applications]` only, preserving everything else), write it atomically with a `.bak` and idempotent no-op skipping, and run all 7 operations against an injectable fixture root — including the mpv-in-Media `set` logic (sets the declared video/audio types, reports the images skipped, guards when an app declares nothing) — with structured `Serialize` results ready for Plan 4's CLI to render as human text or the stable `--json` schema.
