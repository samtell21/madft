# madft Plan 1 — Facts Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the read-only "facts" library that reads the system's freedesktop data — the MIME subclass DAG, the installed-app declarations, and the current defaults — with zero reliance on the host system in tests.

**Architecture:** A Rust library crate (`madft`) plus a stub binary. Approach A from the spec: parse the freedesktop files directly (no `gio`/`xdg-mime` subprocess, no GLib). Every reader takes an injectable `Roots` (the XDG dir set) so tests point at committed fixture trees. This plan delivers `types`, `error`, `paths`, `mimedb`, `appindex`, `defaults`. Categories/engine/writer/CLI are Plans 2–3.

**Tech Stack:** Rust (edition 2024), `thiserror`. No other runtime deps in this plan. Tests use committed fixtures under `tests/fixtures/` located via `env!("CARGO_MANIFEST_DIR")`.

**Plan series:** Plan 1 (facts, this doc) → Plan 2 (categories: tree + TOML source + merge) → Plan 3 (engine + writer + CLI + golden integration).

**Spec:** `docs/superpowers/specs/2026-06-15-madft-design.md`. This plan implements the `mimedb`, `appindex`, `defaults` module rows of §3, the core types of §3, and the exact-declaration / correct-XDG-precedence invariants of §2.

---

## File structure (this plan)

- `Cargo.toml` — crate manifest, `thiserror` dep.
- `src/lib.rs` — module declarations; the library root.
- `src/main.rs` — stub binary (replaced in Plan 3).
- `src/types.rs` — `MimeType`, `DesktopId` newtypes.
- `src/error.rs` — `Error` enum (`thiserror`) + `Result` alias. Defines all variants used across the whole project, even ones not triggered until later plans.
- `src/paths.rs` — `Roots` (XDG dir set) + derived dir/file lists.
- `src/mimedb.rs` — `MimeDb`: universe, alias canonicalization, subclass DAG.
- `src/appindex.rs` — `AppIndex` + `App`: exact-declaration index over desktop files.
- `src/defaults.rs` — `Defaults`: current default per type from the `mimeapps.list` chain.
- `tests/fixtures/mime/{types,subclasses,aliases}` — fixture MIME DB.
- `tests/fixtures/applications/*.desktop`, `tests/fixtures/local/applications/*.desktop` — fixture app dirs (system + home, for precedence).
- `tests/fixtures/config/mimeapps.list`, `tests/fixtures/config-high/mimeapps.list` — fixture defaults (for precedence).

---

### Task 1: Scaffold the crate

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/main.rs`

- [ ] **Step 1: Write `Cargo.toml`**

```toml
[package]
name = "madft"
version = "0.1.0"
edition = "2024"
description = "Inspect and set XDG default applications via a curated category tree"

[dependencies]
thiserror = "2"
```

- [ ] **Step 2: Write `src/lib.rs`** (modules added as later tasks land; start with what compiles)

```rust
//! madft — inspect and set XDG default applications.
//! Plan 1 delivers the read-only facts layer.

pub mod types;
pub mod error;
pub mod paths;
pub mod mimedb;
pub mod appindex;
pub mod defaults;
```

- [ ] **Step 3: Write `src/main.rs`** (real stub; Plan 3 replaces it with the CLI)

```rust
fn main() {
    eprintln!("madft: CLI not yet implemented (facts layer only)");
    std::process::exit(2);
}
```

- [ ] **Step 4: Create empty module files so `lib.rs` compiles**

Create each of these with a single line `// implemented in a later task` so the crate builds:
`src/types.rs`, `src/error.rs`, `src/paths.rs`, `src/mimedb.rs`, `src/appindex.rs`, `src/defaults.rs`.

- [ ] **Step 5: Verify it builds**

Run: `cargo build`
Expected: compiles (warnings about empty modules are fine).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/
git commit -m "scaffold: madft crate skeleton (lib + stub bin)"
```

---

### Task 2: Core newtypes (`MimeType`, `DesktopId`)

**Files:**
- Modify: `src/types.rs`

- [ ] **Step 1: Write the failing test** (replace the file contents)

```rust
use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct MimeType(pub String);

impl MimeType {
    pub fn new(s: impl Into<String>) -> Self {
        MimeType(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MimeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct DesktopId(pub String);

impl DesktopId {
    /// Accepts "mpv" or "mpv.desktop"; always stores the `.desktop` form.
    pub fn new(s: impl Into<String>) -> Self {
        let s = s.into();
        if s.ends_with(".desktop") {
            DesktopId(s)
        } else {
            DesktopId(format!("{s}.desktop"))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DesktopId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_id_normalizes_suffix() {
        assert_eq!(DesktopId::new("mpv"), DesktopId::new("mpv.desktop"));
        assert_eq!(DesktopId::new("mpv").as_str(), "mpv.desktop");
    }

    #[test]
    fn display_roundtrips() {
        assert_eq!(MimeType::new("video/mp4").to_string(), "video/mp4");
        assert_eq!(DesktopId::new("mpv").to_string(), "mpv.desktop");
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib types::`
Expected: PASS (the implementation is included with the tests since these are trivial value types).

- [ ] **Step 3: Commit**

```bash
git add src/types.rs
git commit -m "feat(types): MimeType and DesktopId newtypes"
```

---

### Task 3: Error type

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 1: Write the error enum + a Display test** (replace the file contents)

```rust
//! Project-wide error type. All variants are defined here, including ones not
//! triggered until later plans (categories/engine), so the type is stable.

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("unknown category path: {0}")]
    UnknownPath(String),

    #[error("unknown application: {0}")]
    UnknownApp(String),

    #[error("'{app}' declares none of the types under '{umbrella}'")]
    AppHandlesNothingUnderUmbrella { app: String, umbrella: String },

    #[error("invalid category name: {0}")]
    InvalidCategoryName(String),

    #[error("mimetype '{mime}' is placed under both '{a}' and '{b}'")]
    DuplicatePlacement { mime: String, a: String, b: String },

    #[error("MIME database not found (looked under: {0})")]
    MimeDbNotFound(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error in {path}: {msg}")]
    Parse { path: String, msg: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_human_readable() {
        let e = Error::AppHandlesNothingUnderUmbrella {
            app: "mpv.desktop".into(),
            umbrella: "Images".into(),
        };
        assert_eq!(
            e.to_string(),
            "'mpv.desktop' declares none of the types under 'Images'"
        );
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test --lib error::`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/error.rs
git commit -m "feat(error): project-wide Error enum"
```

---

### Task 4: `Roots` — injectable XDG paths

**Files:**
- Modify: `src/paths.rs`

- [ ] **Step 1: Write the implementation + tests** (replace the file contents)

```rust
//! The XDG directory set, injectable so tests never touch the real system.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Roots {
    pub data_home: PathBuf,
    pub data_dirs: Vec<PathBuf>,
    pub config_home: PathBuf,
    pub config_dirs: Vec<PathBuf>,
}

fn split_paths(var: &str, default: &str) -> Vec<PathBuf> {
    let raw = std::env::var(var).unwrap_or_default();
    let raw = if raw.is_empty() { default.to_string() } else { raw };
    raw.split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

impl Roots {
    /// Build from the live environment, applying XDG defaults.
    pub fn from_env() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let data_home = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(".local/share"));
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(".config"));
        Roots {
            data_home,
            data_dirs: split_paths("XDG_DATA_DIRS", "/usr/local/share:/usr/share"),
            config_home,
            config_dirs: split_paths("XDG_CONFIG_DIRS", "/etc/xdg"),
        }
    }

    /// `applications/` dirs, highest precedence first (data_home, then data_dirs).
    pub fn app_dirs(&self) -> Vec<PathBuf> {
        let mut v = vec![self.data_home.join("applications")];
        v.extend(self.data_dirs.iter().map(|d| d.join("applications")));
        v
    }

    /// `mime/` base dirs (shared-mime-info), user first then system. Order is not
    /// critical: the MIME DB is a union of these.
    pub fn mime_dirs(&self) -> Vec<PathBuf> {
        let mut v = vec![self.data_home.join("mime")];
        v.extend(self.data_dirs.iter().map(|d| d.join("mime")));
        v
    }

    /// `mimeapps.list` candidate files, highest precedence first.
    /// `desktops` is the lowercased $XDG_CURRENT_DESKTOP list (may be empty).
    pub fn mimeapps_files(&self, desktops: &[String]) -> Vec<PathBuf> {
        let mut v = Vec::new();
        // config_home: desktop-prefixed first, then plain
        for d in desktops {
            v.push(self.config_home.join(format!("{d}-mimeapps.list")));
        }
        v.push(self.config_home.join("mimeapps.list"));
        // config_dirs
        for dir in &self.config_dirs {
            for d in desktops {
                v.push(dir.join(format!("{d}-mimeapps.list")));
            }
            v.push(dir.join("mimeapps.list"));
        }
        // data_home/applications
        for d in desktops {
            v.push(self.data_home.join("applications").join(format!("{d}-mimeapps.list")));
        }
        v.push(self.data_home.join("applications/mimeapps.list"));
        // data_dirs/applications
        for dir in &self.data_dirs {
            let apps = dir.join("applications");
            for d in desktops {
                v.push(apps.join(format!("{d}-mimeapps.list")));
            }
            v.push(apps.join("mimeapps.list"));
        }
        v
    }

    /// Where madft WRITES user defaults.
    pub fn user_mimeapps(&self) -> PathBuf {
        self.config_home.join("mimeapps.list")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_roots() -> Roots {
        Roots {
            data_home: PathBuf::from("/home/u/.local/share"),
            data_dirs: vec![PathBuf::from("/usr/share")],
            config_home: PathBuf::from("/home/u/.config"),
            config_dirs: vec![PathBuf::from("/etc/xdg")],
        }
    }

    #[test]
    fn app_dirs_put_home_first() {
        let r = fixture_roots();
        assert_eq!(
            r.app_dirs(),
            vec![
                PathBuf::from("/home/u/.local/share/applications"),
                PathBuf::from("/usr/share/applications"),
            ]
        );
    }

    #[test]
    fn mimeapps_precedence_config_home_first() {
        let r = fixture_roots();
        let files = r.mimeapps_files(&["sway".to_string()]);
        assert_eq!(files[0], PathBuf::from("/home/u/.config/sway-mimeapps.list"));
        assert_eq!(files[1], PathBuf::from("/home/u/.config/mimeapps.list"));
        // user write target is config_home/mimeapps.list
        assert_eq!(r.user_mimeapps(), PathBuf::from("/home/u/.config/mimeapps.list"));
    }

    #[test]
    fn no_desktop_skips_prefixed_files() {
        let r = fixture_roots();
        let files = r.mimeapps_files(&[]);
        assert_eq!(files[0], PathBuf::from("/home/u/.config/mimeapps.list"));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib paths::`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/paths.rs
git commit -m "feat(paths): injectable Roots with XDG precedence"
```

---

### Task 5: `MimeDb` — universe, aliases, subclass DAG

**Files:**
- Modify: `src/mimedb.rs`
- Create: `tests/fixtures/mime/types`
- Create: `tests/fixtures/mime/subclasses`
- Create: `tests/fixtures/mime/aliases`

- [ ] **Step 1: Create the fixture MIME DB**

`tests/fixtures/mime/types`:
```
text/plain
text/html
application/xml
image/svg+xml
image/jpeg
image/png
video/mp4
video/x-matroska
audio/mpeg
application/pdf
application/octet-stream
```

`tests/fixtures/mime/subclasses`:
```
text/html text/plain
application/xml text/plain
image/svg+xml application/xml
```

`tests/fixtures/mime/aliases`:
```
image/jpg image/jpeg
```

- [ ] **Step 2: Write the failing test** (put this `#[cfg(test)]` block at the bottom of `src/mimedb.rs`; the impl above it comes in Step 3)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MimeType;
    use std::path::PathBuf;

    fn db() -> MimeDb {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mime");
        MimeDb::load(&[dir]).expect("load fixture mime db")
    }

    #[test]
    fn universe_contains_known_types() {
        let db = db();
        assert!(db.all_types().any(|t| t.as_str() == "video/mp4"));
        assert!(db.all_types().any(|t| t.as_str() == "text/html"));
    }

    #[test]
    fn canonicalize_resolves_alias() {
        let db = db();
        assert_eq!(
            db.canonicalize(&MimeType::new("image/jpg")),
            MimeType::new("image/jpeg")
        );
        // non-alias passes through
        assert_eq!(
            db.canonicalize(&MimeType::new("image/png")),
            MimeType::new("image/png")
        );
    }

    #[test]
    fn supertypes_are_direct_parents() {
        let db = db();
        assert_eq!(
            db.supertypes(&MimeType::new("text/html")),
            vec![MimeType::new("text/plain")]
        );
    }

    #[test]
    fn ancestor_types_are_transitive() {
        let db = db();
        // svg -> application/xml -> text/plain
        assert_eq!(
            db.ancestor_types(&MimeType::new("image/svg+xml")),
            vec![MimeType::new("application/xml"), MimeType::new("text/plain")]
        );
    }

    #[test]
    fn missing_db_is_error() {
        let bad = PathBuf::from("/nonexistent/mime/dir");
        assert!(MimeDb::load(&[bad]).is_err());
    }
}
```

- [ ] **Step 3: Write the implementation** (put this ABOVE the test module in `src/mimedb.rs`)

```rust
//! Reads the freedesktop shared-mime-info data files: `types`, `subclasses`,
//! `aliases`. Provides the type universe, alias canonicalization, and the
//! subclass DAG (`supertypes` direct, `ancestor_types` transitive).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::types::MimeType;

#[derive(Debug, Default)]
pub struct MimeDb {
    types: HashSet<MimeType>,
    /// child -> its direct supertypes (parents in the subclass DAG)
    supertypes: HashMap<MimeType, Vec<MimeType>>,
    /// alias -> canonical
    aliases: HashMap<MimeType, MimeType>,
}

fn read_lines(path: &PathBuf) -> Result<Vec<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.to_string())
            .collect()),
        // a missing optional file is not an error; return nothing
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(Error::Io(e)),
    }
}

impl MimeDb {
    pub fn load(mime_dirs: &[PathBuf]) -> Result<Self> {
        let mut db = MimeDb::default();
        let mut found_any_types = false;

        for dir in mime_dirs {
            let types_file = dir.join("types");
            let lines = read_lines(&types_file)?;
            if !lines.is_empty() {
                found_any_types = true;
            }
            for t in lines {
                db.types.insert(MimeType::new(t));
            }

            for line in read_lines(&dir.join("subclasses"))? {
                if let Some((child, parent)) = line.split_once(char::is_whitespace) {
                    db.supertypes
                        .entry(MimeType::new(child.trim()))
                        .or_default()
                        .push(MimeType::new(parent.trim()));
                }
            }

            for line in read_lines(&dir.join("aliases"))? {
                if let Some((alias, canonical)) = line.split_once(char::is_whitespace) {
                    db.aliases
                        .insert(MimeType::new(alias.trim()), MimeType::new(canonical.trim()));
                }
            }
        }

        if !found_any_types {
            let looked: Vec<String> =
                mime_dirs.iter().map(|d| d.display().to_string()).collect();
            return Err(Error::MimeDbNotFound(looked.join(", ")));
        }
        Ok(db)
    }

    pub fn all_types(&self) -> impl Iterator<Item = &MimeType> {
        self.types.iter()
    }

    pub fn canonicalize(&self, t: &MimeType) -> MimeType {
        self.aliases.get(t).cloned().unwrap_or_else(|| t.clone())
    }

    /// Direct supertypes (one level up the DAG), alias-canonicalized.
    pub fn supertypes(&self, t: &MimeType) -> Vec<MimeType> {
        let t = self.canonicalize(t);
        self.supertypes
            .get(&t)
            .map(|v| v.iter().map(|p| self.canonicalize(p)).collect())
            .unwrap_or_default()
    }

    /// Transitive supertypes (breadth-first, deduped, excluding `t` itself).
    /// This is the "what you'd inherit if unset" chain.
    pub fn ancestor_types(&self, t: &MimeType) -> Vec<MimeType> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut queue: VecDeque<MimeType> = self.supertypes(t).into_iter().collect();
        while let Some(cur) = queue.pop_front() {
            if !seen.insert(cur.clone()) {
                continue;
            }
            out.push(cur.clone());
            for p in self.supertypes(&cur) {
                queue.push_back(p);
            }
        }
        out
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib mimedb::`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/mimedb.rs tests/fixtures/mime/
git commit -m "feat(mimedb): MIME universe, aliases, subclass DAG"
```

---

### Task 6: `AppIndex` — exact-declaration index

**Files:**
- Modify: `src/appindex.rs`
- Create: `tests/fixtures/applications/mpv.desktop`
- Create: `tests/fixtures/applications/eog.desktop`
- Create: `tests/fixtures/applications/nvim.desktop`
- Create: `tests/fixtures/applications/webcam.desktop`
- Create: `tests/fixtures/local/applications/webcam.desktop`

- [ ] **Step 1: Create fixture desktop files**

`tests/fixtures/applications/mpv.desktop`:
```
[Desktop Entry]
Name=mpv Media Player
Type=Application
Exec=mpv %U
MimeType=video/mp4;video/x-matroska;audio/mpeg;
```

`tests/fixtures/applications/eog.desktop`:
```
[Desktop Entry]
Name=Image Viewer
Type=Application
Exec=eog %U
MimeType=image/png;image/jpeg;image/svg+xml;
```

`tests/fixtures/applications/nvim.desktop`:
```
[Desktop Entry]
Name=Neovim
Type=Application
Exec=nvim %F
MimeType=text/plain;text/html;
```

`tests/fixtures/applications/webcam.desktop` (system copy — lower precedence):
```
[Desktop Entry]
Name=Webcam SYSTEM
Type=Application
Exec=cam
MimeType=video/mp4;
```

`tests/fixtures/local/applications/webcam.desktop` (home copy — higher precedence, shadows system):
```
[Desktop Entry]
Name=Webcam HOME
Type=Application
Exec=/home/u/.local/bin/cam
MimeType=video/mp4;
```

- [ ] **Step 2: Write the failing test** (bottom of `src/appindex.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Roots;
    use crate::types::{DesktopId, MimeType};
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn index_single_dir() -> AppIndex {
        let roots = Roots {
            data_home: fixtures(),
            data_dirs: vec![],
            config_home: PathBuf::from("/unused"),
            config_dirs: vec![],
        };
        AppIndex::load(&roots).unwrap()
    }

    #[test]
    fn apps_for_type_uses_exact_declaration() {
        let idx = index_single_dir();
        let apps = idx.apps_for_type(&MimeType::new("video/mp4"));
        assert!(apps.iter().any(|a| a.id == DesktopId::new("mpv")));
        // eog does NOT declare video/mp4
        assert!(!apps.iter().any(|a| a.id == DesktopId::new("eog")));
    }

    #[test]
    fn declares_is_exact() {
        let idx = index_single_dir();
        assert!(idx.declares(&DesktopId::new("mpv"), &MimeType::new("audio/mpeg")));
        assert!(!idx.declares(&DesktopId::new("mpv"), &MimeType::new("image/png")));
    }

    #[test]
    fn home_dir_shadows_system_for_same_id() {
        // data_home = fixtures/local, data_dirs = [fixtures]; both have webcam.desktop
        let roots = Roots {
            data_home: fixtures().join("local"),
            data_dirs: vec![fixtures()],
            config_home: PathBuf::from("/unused"),
            config_dirs: vec![],
        };
        let idx = AppIndex::load(&roots).unwrap();
        let app = idx.app(&DesktopId::new("webcam")).unwrap();
        assert_eq!(app.name, "Webcam HOME");
    }
}
```

- [ ] **Step 3: Write the implementation** (above the test module)

```rust
//! Scans `applications/*.desktop` across the XDG path and indexes each app's
//! EXACTLY declared `MimeType=` set. This is the sole authority for
//! "app X handles type T" — never subclass-aware.

use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::paths::Roots;
use crate::types::{DesktopId, MimeType};

#[derive(Debug, Clone)]
pub struct App {
    pub id: DesktopId,
    pub name: String,
    pub nodisplay: bool,
    pub mimetypes: HashSet<MimeType>,
}

#[derive(Debug, Default)]
pub struct AppIndex {
    apps: HashMap<DesktopId, App>,
    by_type: HashMap<MimeType, Vec<DesktopId>>,
}

/// Parse one desktop file's [Desktop Entry] keys we care about.
/// Returns None if there is no [Desktop Entry] group.
fn parse_desktop(content: &str) -> Option<(String, bool, HashSet<MimeType>)> {
    let mut in_entry = false;
    let mut name = String::new();
    let mut nodisplay = false;
    let mut mimetypes = HashSet::new();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Name" if name.is_empty() => name = value.trim().to_string(),
            "NoDisplay" => nodisplay = value.trim().eq_ignore_ascii_case("true"),
            "MimeType" => {
                for t in value.split(';') {
                    let t = t.trim();
                    if !t.is_empty() {
                        mimetypes.insert(MimeType::new(t));
                    }
                }
            }
            _ => {}
        }
    }
    Some((name, nodisplay, mimetypes))
}

impl AppIndex {
    pub fn load(roots: &Roots) -> Result<Self> {
        let mut idx = AppIndex::default();

        // Highest precedence first: first-seen desktop-id wins (correct XDG;
        // NOT wofi's inverted behavior).
        for dir in roots.app_dirs() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue, // missing dir is fine
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                let Some(stem) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let id = DesktopId::new(stem.to_string());
                if idx.apps.contains_key(&id) {
                    continue; // already seen at higher precedence
                }
                let content = std::fs::read_to_string(&path)?;
                if let Some((name, nodisplay, mimetypes)) = parse_desktop(&content) {
                    for t in &mimetypes {
                        idx.by_type.entry(t.clone()).or_default().push(id.clone());
                    }
                    idx.apps.insert(
                        id.clone(),
                        App { id, name, nodisplay, mimetypes },
                    );
                }
            }
        }
        Ok(idx)
    }

    pub fn app(&self, id: &DesktopId) -> Option<&App> {
        self.apps.get(id)
    }

    pub fn apps_for_type(&self, t: &MimeType) -> Vec<&App> {
        self.by_type
            .get(t)
            .map(|ids| ids.iter().filter_map(|id| self.apps.get(id)).collect())
            .unwrap_or_default()
    }

    pub fn declares(&self, id: &DesktopId, t: &MimeType) -> bool {
        self.apps
            .get(id)
            .map(|a| a.mimetypes.contains(t))
            .unwrap_or(false)
    }
}
```

> Note: declared types are stored as-written. Alias-canonicalization of the
> query happens in the engine (Plan 3), which canonicalizes the lookup type via
> `MimeDb` before calling `apps_for_type`. Fixtures use canonical types.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib appindex::`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/appindex.rs tests/fixtures/applications/ tests/fixtures/local/
git commit -m "feat(appindex): exact-declaration app index with XDG precedence"
```

---

### Task 7: `Defaults` — current default from the mimeapps.list chain

**Files:**
- Modify: `src/defaults.rs`
- Create: `tests/fixtures/config/mimeapps.list`
- Create: `tests/fixtures/config-high/mimeapps.list`

- [ ] **Step 1: Create fixture mimeapps.list files**

`tests/fixtures/config/mimeapps.list`:
```
[Default Applications]
text/html=org.qutebrowser.qutebrowser.desktop
video/mp4=mpv.desktop
text/plain=nvim.desktop
```

`tests/fixtures/config-high/mimeapps.list` (a higher-precedence file overriding text/html):
```
[Default Applications]
text/html=org.mozilla.firefox.desktop
```

- [ ] **Step 2: Write the failing test** (bottom of `src/defaults.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DesktopId, MimeType};
    use std::path::PathBuf;

    fn file(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    #[test]
    fn reads_default_from_single_file() {
        let d = Defaults::load(&[file("tests/fixtures/config/mimeapps.list")]).unwrap();
        assert_eq!(
            d.current_default(&MimeType::new("video/mp4")),
            Some(DesktopId::new("mpv"))
        );
        assert_eq!(d.current_default(&MimeType::new("image/png")), None);
    }

    #[test]
    fn higher_precedence_file_wins() {
        // config-high listed first => higher precedence
        let d = Defaults::load(&[
            file("tests/fixtures/config-high/mimeapps.list"),
            file("tests/fixtures/config/mimeapps.list"),
        ])
        .unwrap();
        assert_eq!(
            d.current_default(&MimeType::new("text/html")),
            Some(DesktopId::new("org.mozilla.firefox"))
        );
        // video/mp4 only exists in the lower file, still found
        assert_eq!(
            d.current_default(&MimeType::new("video/mp4")),
            Some(DesktopId::new("mpv"))
        );
    }

    #[test]
    fn missing_files_are_skipped() {
        let d = Defaults::load(&[file("tests/fixtures/does-not-exist.list")]).unwrap();
        assert_eq!(d.current_default(&MimeType::new("video/mp4")), None);
    }
}
```

- [ ] **Step 3: Write the implementation** (above the test module)

```rust
//! Reads the effective current default per type from the `mimeapps.list`
//! precedence chain. `files` are highest-precedence first.
//!
//! Plan-1 scope: resolve `[Default Applications]` only — highest file that
//! lists a type wins (its first listed desktop-id). The "must be installed"
//! cross-check and `[Removed Associations]` handling are layered in by the
//! engine (Plan 3); for current-default DISPLAY this matches the dominant case.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::types::{DesktopId, MimeType};

#[derive(Debug, Default)]
pub struct Defaults {
    /// In precedence order (highest first): each file's [Default Applications].
    files: Vec<HashMap<MimeType, DesktopId>>,
}

/// Parse a single mimeapps.list into the [Default Applications] map
/// (type -> first listed desktop-id).
fn parse_default_apps(content: &str) -> HashMap<MimeType, DesktopId> {
    let mut map = HashMap::new();
    let mut in_section = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line == "[Default Applications]";
            continue;
        }
        if !in_section || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((mime, ids)) = line.split_once('=') {
            let first = ids.split(';').map(|s| s.trim()).find(|s| !s.is_empty());
            if let Some(id) = first {
                map.entry(MimeType::new(mime.trim()))
                    .or_insert_with(|| DesktopId::new(id.to_string()));
            }
        }
    }
    map
}

impl Defaults {
    pub fn load(files: &[PathBuf]) -> Result<Self> {
        let mut out = Defaults::default();
        for path in files {
            match std::fs::read_to_string(path) {
                Ok(content) => out.files.push(parse_default_apps(&content)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::Io(e)),
            }
        }
        Ok(out)
    }

    pub fn current_default(&self, t: &MimeType) -> Option<DesktopId> {
        self.files.iter().find_map(|m| m.get(t).cloned())
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib defaults::`
Expected: PASS (3 tests).

- [ ] **Step 5: Run the full suite + clippy**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all tests PASS; no clippy errors. (Fix any clippy lints inline.)

- [ ] **Step 6: Commit**

```bash
git add src/defaults.rs tests/fixtures/config/ tests/fixtures/config-high/
git commit -m "feat(defaults): current default from mimeapps.list chain"
```

---

## Plan 1 Self-Review (completed during authoring)

- **Spec coverage:** `mimedb` (§3 row + §2 subclass-DAG invariant) → Task 5. `appindex` (§3 row + §2 exact-declaration + correct-XDG-precedence) → Task 6. `defaults` (§3 row) → Task 7. Core types `MimeType`/`DesktopId` (§3) → Task 2. `Error` incl. all variants (§7) → Task 3. Injectable roots for fixture-based testing (§8) → Task 4. Categories/engine/writer/CLI are explicitly Plans 2–3.
- **Placeholder scan:** none — every step ships complete code and exact commands.
- **Type consistency:** `MimeType::new`/`as_str`, `DesktopId::new` (suffix-normalizing), `Roots` fields, `MimeDb::{load,all_types,canonicalize,supertypes,ancestor_types}`, `AppIndex::{load,app,apps_for_type,declares}`, `App{id,name,nodisplay,mimetypes}`, `Defaults::{load,current_default}` are used identically across tasks and match the spec's §3 API names.

## Done criteria for Plan 1

`cargo test` green, `cargo clippy -D warnings` clean, and the facts library can: enumerate the type universe, canonicalize aliases, walk the subclass DAG, answer exact-declaration app queries with correct XDG precedence, and report the current default per type — all against injectable fixture roots. Plan 2 builds the category tree (arena model + TOML `Source` + layered merge) on top of `MimeDb`.
