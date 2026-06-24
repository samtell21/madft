# `.desktop` File Parse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `madft app <app> desktop [fields...]` — a faithful, order-preserving `.desktop` file inspector with optional field selection.

**Architecture:** A new dependency-free `src/desktop.rs` parses `.desktop` files into ordered sections of ordered key/value pairs, with a hand-written `Serialize` impl that emits order-preserving JSON objects. `appindex.rs` stores each app's path and reuses the new parser. `engine.rs` gains a `desktop(id)` method; `cli.rs` gains a nested `AppAction::Desktop` subcommand with human and JSON rendering.

**Tech Stack:** Rust 2024, clap (derive), serde + serde_json (no new dependencies).

## Global Constraints

- Edition 2024, `rust-version = "1.85"`.
- **No new dependencies** — only clap, serde, serde_json, thiserror, toml are allowed.
- JSON struct fields use snake_case verbatim (serde does NOT rename); `.desktop` keys are kept verbatim from the file.
- Errors use the existing `crate::error::Error` / `Result` (thiserror); error JSON `kind` is kebab-case.
- Tests: unit tests inline under `#[cfg(test)] mod tests`; golden integration tests in `tests/golden.rs`.
- Faithful parse: NO type coercion, NO `Exec` splitting, NO key renaming.

---

### Task 1: The `desktop` parser module

**Files:**
- Create: `src/desktop.rs`
- Modify: `src/lib.rs` (add `pub mod desktop;` — modules are declared in `lib.rs`, NOT `main.rs`)

**Interfaces:**
- Produces:
  - `pub struct DesktopFile { pub path: String, pub sections: Vec<DesktopSection> }`
  - `pub struct DesktopSection { pub name: String, pub entries: Vec<(String, String)> }`
  - `pub fn parse(content: &str) -> DesktopFile` — returns `path: String::new()` (caller sets path); preserves file order; first key wins within a section.
  - `impl DesktopSection { pub fn get(&self, key: &str) -> Option<&str> }` — exact, case-sensitive lookup.
  - `impl DesktopFile { pub fn entry_section(&self) -> Option<&DesktopSection> }` — the `"Desktop Entry"` section.

- [ ] **Step 1: Write the failing tests**

Add to `src/desktop.rs`:

```rust
//! Faithful, order-preserving parser for freedesktop `.desktop` files.
//! Values are raw strings — no type coercion, no `Exec` splitting, keys verbatim.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_and_keys_in_order() {
        let f = parse("[Desktop Entry]\nName=Neovim\nExec=nvim %F\nTerminal=true\n");
        assert_eq!(f.sections.len(), 1);
        let s = &f.sections[0];
        assert_eq!(s.name, "Desktop Entry");
        assert_eq!(
            s.entries,
            vec![
                ("Name".to_string(), "Neovim".to_string()),
                ("Exec".to_string(), "nvim %F".to_string()),
                ("Terminal".to_string(), "true".to_string()),
            ]
        );
    }

    #[test]
    fn skips_comments_blanks_and_preamble() {
        let f = parse("# a comment\npreamble=ignored\n\n[Desktop Entry]\n# inner\nName=X\n\n");
        assert_eq!(f.sections.len(), 1);
        assert_eq!(f.sections[0].entries, vec![("Name".to_string(), "X".to_string())]);
    }

    #[test]
    fn keeps_verbatim_keys_locales_and_extensions() {
        let f = parse("[Desktop Entry]\nName[de]=Editor\nX-GNOME-Autostart=true\n");
        let s = &f.sections[0];
        assert_eq!(s.get("Name[de]"), Some("Editor"));
        assert_eq!(s.get("X-GNOME-Autostart"), Some("true"));
    }

    #[test]
    fn first_key_wins_and_case_is_distinct() {
        let f = parse("[Desktop Entry]\nExec=first\nExec=second\nexec=lower\n");
        let s = &f.sections[0];
        assert_eq!(s.get("Exec"), Some("first"));
        assert_eq!(s.get("exec"), Some("lower"));
    }

    #[test]
    fn splits_value_on_first_equals_only() {
        let f = parse("[Desktop Entry]\nExec=env A=b app %U\n");
        assert_eq!(f.sections[0].get("Exec"), Some("env A=b app %U"));
    }

    #[test]
    fn captures_action_sections() {
        let f = parse("[Desktop Entry]\nName=X\n[Desktop Action new-window]\nName=New Window\nExec=app --new\n");
        assert_eq!(f.sections.len(), 2);
        assert_eq!(f.sections[1].name, "Desktop Action new-window");
        assert_eq!(f.sections[1].get("Exec"), Some("app --new"));
    }

    #[test]
    fn entry_section_finds_desktop_entry() {
        let f = parse("[Desktop Action x]\nName=A\n[Desktop Entry]\nName=B\n");
        assert_eq!(f.entry_section().unwrap().get("Name"), Some("B"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib desktop`
Expected: FAIL — `parse`, `DesktopFile`, `DesktopSection` not found.

- [ ] **Step 3: Implement the parser**

Add above the `#[cfg(test)]` block in `src/desktop.rs`:

```rust
#[derive(Debug, Clone)]
pub struct DesktopFile {
    pub path: String,
    pub sections: Vec<DesktopSection>,
}

#[derive(Debug, Clone)]
pub struct DesktopSection {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

impl DesktopSection {
    /// Exact, case-sensitive lookup of a key's value within this section.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

impl DesktopFile {
    /// The `[Desktop Entry]` section, if present.
    pub fn entry_section(&self) -> Option<&DesktopSection> {
        self.sections.iter().find(|s| s.name == "Desktop Entry")
    }
}

/// Parse `.desktop` content into ordered sections of ordered key/value pairs.
/// Faithful: keys verbatim, values raw, file order preserved. The first
/// occurrence of a key within a section wins (keeps emitted JSON objects valid).
/// `path` is left empty for the caller to populate.
pub fn parse(content: &str) -> DesktopFile {
    let mut sections: Vec<DesktopSection> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let name = line[1..line.len() - 1].to_string();
            sections.push(DesktopSection { name, entries: Vec::new() });
            continue;
        }
        let Some(section) = sections.last_mut() else {
            continue; // key/value before the first header — ignore
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim().to_string();
        if section.entries.iter().any(|(k, _)| *k == key) {
            continue; // first occurrence wins
        }
        section.entries.push((key, value));
    }

    DesktopFile { path: String::new(), sections }
}
```

- [ ] **Step 4: Register the module**

In `src/lib.rs`, add `pub mod desktop;` alongside the other `pub mod` declarations (e.g. after `pub mod appindex;`).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib desktop`
Expected: PASS (7 tests).

- [ ] **Step 6: Commit**

```bash
git add src/desktop.rs src/main.rs
git commit -m "feat(desktop): faithful order-preserving .desktop parser"
```

---

### Task 2: Order-preserving JSON serialization

**Files:**
- Modify: `src/desktop.rs`

**Interfaces:**
- Produces: `impl serde::Serialize for DesktopFile` — emits `{ "path": <string>, "sections": { <section name>: { <key>: <value>, ... }, ... } }` in file order, via `serialize_map`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/desktop.rs`:

```rust
#[test]
fn serializes_to_ordered_json_objects() {
    let mut f = parse("[Desktop Entry]\nName=Neovim\nExec=nvim %F\n[Desktop Action x]\nName=W\n");
    f.path = "/apps/nvim.desktop".to_string();
    let v: serde_json::Value = serde_json::to_value(&f).unwrap();
    assert_eq!(v["path"], "/apps/nvim.desktop");
    assert_eq!(v["sections"]["Desktop Entry"]["Exec"], "nvim %F");
    assert_eq!(v["sections"]["Desktop Action x"]["Name"], "W");

    // Order preserved in the serialized string.
    let s = serde_json::to_string(&f).unwrap();
    let name_at = s.find("\"Name\"").unwrap();
    let exec_at = s.find("\"Exec\"").unwrap();
    assert!(name_at < exec_at, "keys should serialize in file order");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib desktop::tests::serializes_to_ordered_json_objects`
Expected: FAIL — `DesktopFile` does not implement `Serialize`.

- [ ] **Step 3: Implement the Serialize impl**

Add to `src/desktop.rs` (top-level, near the structs). Note: `DesktopFile`/`DesktopSection` keep their plain derives; serialization is hand-written so ordered `Vec`s become JSON objects without `indexmap`.

```rust
use serde::ser::{Serialize, SerializeMap, Serializer};

impl Serialize for DesktopFile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("path", &self.path)?;
        map.serialize_entry("sections", &Sections(&self.sections))?;
        map.end()
    }
}

/// Serializes a slice of sections as a JSON object keyed by section name.
struct Sections<'a>(&'a [DesktopSection]);

impl Serialize for Sections<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for section in self.0 {
            map.serialize_entry(&section.name, &Entries(&section.entries))?;
        }
        map.end()
    }
}

/// Serializes a slice of key/value pairs as a JSON object in order.
struct Entries<'a>(&'a [(String, String)]);

impl Serialize for Entries<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib desktop`
Expected: PASS (8 tests).

- [ ] **Step 5: Commit**

```bash
git add src/desktop.rs
git commit -m "feat(desktop): order-preserving JSON serialization without indexmap"
```

---

### Task 3: Store the app path and reuse the parser in `appindex`

**Files:**
- Modify: `src/appindex.rs`

**Interfaces:**
- Consumes: `desktop::parse`, `DesktopFile::entry_section`, `DesktopSection::get` (Task 1).
- Produces: `App` gains `pub path: std::path::PathBuf`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/appindex.rs`:

```rust
#[test]
fn app_records_its_source_path() {
    let idx = index_single_dir();
    let app = idx.app(&DesktopId::new("mpv")).unwrap();
    assert!(app.path.ends_with("mpv.desktop"), "got {:?}", app.path);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib appindex::tests::app_records_its_source_path`
Expected: FAIL — no field `path` on `App`.

- [ ] **Step 3: Add the field and reuse the parser**

In `src/appindex.rs`:

1. Add to the imports at the top: `use std::path::PathBuf;`
2. Add the field to `App`:

```rust
#[derive(Debug, Clone)]
pub struct App {
    pub id: DesktopId,
    pub name: String,
    pub nodisplay: bool,
    pub mimetypes: HashSet<MimeType>,
    pub path: PathBuf,
}
```

3. Replace the existing `parse_desktop` function with one that delegates to the shared parser:

```rust
/// Extract the [Desktop Entry] fields the index needs, via the shared parser.
/// Returns None if there is no [Desktop Entry] group.
fn parse_desktop(content: &str) -> Option<(String, bool, HashSet<MimeType>)> {
    let file = crate::desktop::parse(content);
    let entry = file.entry_section()?;

    let name = entry.get("Name").unwrap_or("").to_string();
    let nodisplay = entry.get("NoDisplay").is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let mut mimetypes = HashSet::new();
    if let Some(list) = entry.get("MimeType") {
        for t in list.split(';') {
            let t = t.trim();
            if !t.is_empty() {
                mimetypes.insert(MimeType::new(t));
            }
        }
    }
    Some((name, nodisplay, mimetypes))
}
```

4. In `load()`, populate the new field. Change the `App { ... }` construction to include `path: path.clone()`:

```rust
                if let Some((name, nodisplay, mimetypes)) = parse_desktop(&content) {
                    for t in &mimetypes {
                        idx.by_type.entry(t.clone()).or_default().push(id.clone());
                    }
                    idx.apps.insert(
                        id.clone(),
                        App { id, name, nodisplay, mimetypes, path: path.clone() },
                    );
                }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib appindex`
Expected: PASS — the new test plus the existing `apps_for_type_uses_exact_declaration`, `declares_is_exact`, `home_dir_shadows_system_for_same_id`.

- [ ] **Step 5: Commit**

```bash
git add src/appindex.rs
git commit -m "refactor(appindex): store app path, reuse shared .desktop parser"
```

---

### Task 4: `engine.desktop(id)`

**Files:**
- Modify: `src/engine.rs`

**Interfaces:**
- Consumes: `App.path` (Task 3), `desktop::parse` / `DesktopFile` (Task 1).
- Produces: `pub fn desktop(&self, id: &str) -> Result<crate::desktop::DesktopFile>`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/engine.rs` (it already has a `read_engine()`/`engine()` helper — match whichever the surrounding tests use; the `app` tests use `engine()`):

```rust
#[test]
fn desktop_parses_named_app_with_path() {
    let f = engine().desktop("mpv").unwrap();
    assert!(f.path.ends_with("mpv.desktop"));
    assert_eq!(f.entry_section().unwrap().get("Name"), Some("mpv Media Player"));
}

#[test]
fn desktop_unknown_app_errors() {
    let err = engine().desktop("ghost").unwrap_err();
    assert!(matches!(err, crate::error::Error::UnknownApp(_)));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib engine::tests::desktop`
Expected: FAIL — no method `desktop` on `Engine`.

- [ ] **Step 3: Implement the method**

Add to the `impl Engine` block in `src/engine.rs`, near the existing `app` method:

```rust
    /// Parse the named app's `.desktop` file faithfully (raw strings, file order).
    pub fn desktop(&self, id: &str) -> Result<crate::desktop::DesktopFile> {
        let app_id = DesktopId::new(id);
        let app = self
            .appindex
            .app(&app_id)
            .ok_or_else(|| Error::UnknownApp(app_id.to_string()))?;
        let content = std::fs::read_to_string(&app.path)?;
        let mut file = crate::desktop::parse(&content);
        file.path = app.path.display().to_string();
        Ok(file)
    }
```

(If `Error` or `DesktopId` are not already in scope at the top of `engine.rs`, they are — they're used by the existing `app` method. Reuse the same imports.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib engine::tests::desktop`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/engine.rs
git commit -m "feat(engine): desktop(id) reads and parses an app's .desktop file"
```

---

### Task 5: CLI subcommand + rendering

**Files:**
- Modify: `src/cli.rs`

**Interfaces:**
- Consumes: `engine.desktop(id)` (Task 4), `DesktopFile` / `DesktopSection` (Task 1), `to_json` helper (existing in `cli.rs`).
- Produces: `AppAction` enum; `Command::App` gains `action: Option<AppAction>`.

> **Architecture note (verified against `src/cli.rs`):** the dispatcher is
> `fn run_command(engine, command, json, show_all) -> Result<String, Error>`.
> Each arm returns the rendered **stdout string** (NO trailing newline — `run()`
> adds one via `println!`), and propagates errors with `?`. `execute` wraps the
> `Result` and routes errors through the central `render_error`, which produces
> `{"error":{"kind":...}}` + code 1 in JSON mode, or `error: …` on **stderr**
> (empty stdout) + code 1 in human mode. So the desktop arm does NOT do its own
> error handling — it just `?`s `engine.desktop(id)` and returns a `String`.
> The `Outcome` struct is `{ code, stdout, stderr }`. The cli test helper is
> `engine()`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/cli.rs` (mirror the existing `app_json_reports_rows` test; helper is `engine()`):

```rust
#[test]
fn desktop_full_json_has_sections() {
    let cmd = Command::App { id: "mpv".to_string(), action: Some(AppAction::Desktop { fields: vec![] }) };
    let out = execute(&engine(), &cmd, true, false);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert!(v["path"].as_str().unwrap().ends_with("mpv.desktop"));
    assert_eq!(v["sections"]["Desktop Entry"]["Name"], "mpv Media Player");
}

#[test]
fn desktop_full_human_reproduces_ini() {
    let cmd = Command::App { id: "mpv".to_string(), action: Some(AppAction::Desktop { fields: vec![] }) };
    let out = execute(&engine(), &cmd, false, false);
    assert!(out.stdout.contains("[Desktop Entry]"));
    assert!(out.stdout.contains("Name=mpv Media Player"));
}

#[test]
fn desktop_selected_fields_human_one_per_line() {
    let cmd = Command::App {
        id: "mpv".to_string(),
        action: Some(AppAction::Desktop { fields: vec!["Name".to_string(), "Exec".to_string()] }),
    };
    let out = execute(&engine(), &cmd, false, false);
    // No trailing newline — run() adds the final one via println!.
    assert_eq!(out.stdout, "mpv Media Player\nmpv %U");
}

#[test]
fn desktop_selected_fields_json_keyed_by_field() {
    let cmd = Command::App {
        id: "mpv".to_string(),
        action: Some(AppAction::Desktop { fields: vec!["Exec".to_string(), "Nope".to_string()] }),
    };
    let out = execute(&engine(), &cmd, true, false);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["Exec"], "mpv %U");
    assert!(v["Nope"].is_null());
}

#[test]
fn desktop_unknown_app_errors_json() {
    let cmd = Command::App { id: "ghost".to_string(), action: Some(AppAction::Desktop { fields: vec![] }) };
    let out = execute(&engine(), &cmd, true, false);
    assert_eq!(out.code, 1);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "unknown-app");
}
```

> Fixture facts (confirmed in `tests/fixtures/applications/mpv.desktop`):
> `Name=mpv Media Player`, `Exec=mpv %U`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli::tests::desktop`
Expected: FAIL — `AppAction` undefined; `Command::App` has no `action` field.

- [ ] **Step 3: Add the subcommand to the `Command` enum**

In `src/cli.rs`, change the `App` variant and add the `AppAction` enum next to `Command`:

```rust
    App {
        id: String,
        #[command(subcommand)]
        action: Option<AppAction>,
    },
```

```rust
#[derive(Subcommand, Debug)]
pub enum AppAction {
    /// Show the parsed .desktop file, or select specific fields
    Desktop {
        /// Specific keys to print (case-sensitive, from [Desktop Entry])
        fields: Vec<String>,
    },
}
```

- [ ] **Step 4: Update the `App` match arm in `run_command`**

Replace the existing arm (lines 135-138):

```rust
        Command::App { id } => {
            let r = engine.app(id)?;
            if json { to_json(&r) } else { human_app(&r) }
        }
```

with:

```rust
        Command::App { id, action } => match action {
            None => {
                let r = engine.app(id)?;
                if json { to_json(&r) } else { human_app(&r) }
            }
            Some(AppAction::Desktop { fields }) => {
                let file = engine.desktop(id)?;
                render_desktop(&file, fields, json)
            }
        }
```

Errors from `engine.desktop(id)` propagate via `?` to `render_error` — no
per-arm error handling needed.

- [ ] **Step 5: Add the desktop renderer**

Add this free function near the other `human_*` renderers in `src/cli.rs`. It
returns a plain `String` (the rendered stdout, no trailing newline), matching
every other renderer:

```rust
/// Render a parsed `.desktop` file: full INI-style dump, or selected raw values.
fn render_desktop(file: &crate::desktop::DesktopFile, fields: &[String], json: bool) -> String {
    if fields.is_empty() {
        // Full dump.
        if json {
            return to_json(file);
        }
        let mut s = String::new();
        for section in &file.sections {
            s.push_str(&format!("[{}]\n", section.name));
            for (k, v) in &section.entries {
                s.push_str(&format!("{k}={v}\n"));
            }
            s.push('\n');
        }
        return s.trim_end().to_string();
    }

    // Field selection: case-sensitive, [Desktop Entry] only.
    let entry = file.entry_section();
    if json {
        let mut map = serde_json::Map::new();
        for f in fields {
            let val = entry
                .and_then(|s| s.get(f))
                .map_or(serde_json::Value::Null, |v| serde_json::Value::String(v.to_string()));
            map.insert(f.clone(), val);
        }
        return to_json(&serde_json::Value::Object(map));
    }
    fields
        .iter()
        .map(|f| entry.and_then(|s| s.get(f)).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --lib cli::tests::desktop`
Expected: PASS (5 tests).

- [ ] **Step 7: Fix other `Command::App` constructions, then run the full suite**

Two existing tests construct `Command::App { id: ... }` without the new field
(around lines 489 and 559). Add `action: None` to each:

```rust
Command::App { id: "mpv".to_string(), action: None }
```

Run: `cargo test`
Expected: PASS — all existing tests plus the new ones.

- [ ] **Step 8: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): madft app <app> desktop [fields...] with human/JSON rendering"
```

---

### Task 6: Golden integration tests + faithfulness fixture

**Files:**
- Create: `tests/fixtures/applications/actions-app.desktop`
- Modify: `tests/golden.rs`

**Interfaces:**
- Consumes: `parse`/`execute` test harness already in `tests/golden.rs`.

- [ ] **Step 1: Create the faithfulness fixture**

Create `tests/fixtures/applications/actions-app.desktop`:

```ini
[Desktop Entry]
Name=Actions App
Name[de]=Aktionen App
Exec=actions-app %U
Terminal=false
X-Custom-Flag=hello
MimeType=text/plain;

[Desktop Action new-window]
Name=New Window
Exec=actions-app --new-window
```

- [ ] **Step 2: Write the failing golden tests**

Add to `tests/golden.rs` (mirror the existing `golden_*` style — `parse(&[...])` then `execute(&read_engine(), ...)`):

```rust
#[test]
fn golden_desktop_full_json_is_faithful() {
    let cli = parse(&["madft", "app", "actions-app", "desktop", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let entry = &v["sections"]["Desktop Entry"];
    assert_eq!(entry["Name[de]"], "Aktionen App");      // locale key verbatim
    assert_eq!(entry["X-Custom-Flag"], "hello");        // X- extension kept
    assert_eq!(entry["Terminal"], "false");             // raw string, not a bool
    assert_eq!(v["sections"]["Desktop Action new-window"]["Exec"], "actions-app --new-window");
}

#[test]
fn golden_desktop_selected_field_plain() {
    let cli = parse(&["madft", "app", "actions-app", "desktop", "Exec"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 0);
    // No trailing newline — execute() captures stdout before run()'s println!.
    assert_eq!(out.stdout, "actions-app %U");
}

#[test]
fn golden_desktop_case_sensitive_miss_is_empty() {
    let cli = parse(&["madft", "app", "actions-app", "desktop", "exec"]); // wrong case
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.stdout, ""); // single missed field → empty string (println! prints one blank line)
}
```

- [ ] **Step 3: Run tests to verify they fail, then pass**

Run: `cargo test --test golden desktop`
Expected: First FAIL if fixture/route missing; after Tasks 1-5 are in, PASS.

- [ ] **Step 4: Run the full suite**

Run: `cargo test`
Expected: PASS (all).

- [ ] **Step 5: Commit**

```bash
git add tests/fixtures/applications/actions-app.desktop tests/golden.rs
git commit -m "test(desktop): golden coverage for faithful parse, fields, case-sensitivity"
```

---

### Task 7: Docs & version bump

**Files:**
- Modify: `Cargo.toml` (version), `README.md` (if it documents commands — check first)

- [ ] **Step 1: Check whether README documents the `app` command**

Run: `grep -n "madft app" README.md`
Expected: shows existing `app` usage lines to mirror, or nothing (skip README edit if absent).

- [ ] **Step 2: Document `app … desktop`**

If the README has a commands section, add an entry mirroring the existing style, e.g.:

```markdown
### `madft app <app> desktop [fields...]`

Print the parsed `.desktop` file for an application. With no fields, dumps all
sections and keys (use `--json` for machine output). With field names
(case-sensitive, from `[Desktop Entry]`), prints just those raw values, one per
line — handy for scripts without `jq`:

    madft app nvim desktop Exec        # → nvim %F
    madft app nvim desktop --json
```

- [ ] **Step 3: Bump the version**

In `Cargo.toml`, bump `version` one minor (e.g. `0.4.0` → `0.5.0`) since this adds a user-facing command.

- [ ] **Step 4: Run the full suite once more**

Run: `cargo test && cargo build --release`
Expected: PASS, clean build.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml README.md
git commit -m "docs: document app desktop command; bump version to 0.5.0"
```

---

## Self-Review

**Spec coverage:**
- Faithful parser (raw strings, verbatim keys, file order, first-key-wins) → Task 1. ✅
- Order-preserving JSON without new dep → Task 2. ✅
- `appindex` stores path + reuses parser → Task 3. ✅
- `engine.desktop(id)` with `UnknownApp` → Task 4. ✅
- CLI nested subcommand, case-sensitive `[Desktop Entry]` selection, human/JSON, missing→empty/null, `Terminal` raw → Task 5. ✅
- Tests incl. `X-`/locale/Action fixture, four output paths → Tasks 1, 5, 6. ✅
- Out-of-scope items (no coercion, no Exec split, no locale resolution) honored throughout. ✅

**Placeholder scan:** No TBD/TODO in steps. Task 5 was verified against the real
`src/cli.rs`: dispatcher `run_command -> Result<String, Error>`, `Outcome { code,
stdout, stderr }`, central `render_error` (JSON `kind` / human stderr), `run()`
adds the trailing newline via `println!`, cli test helper `engine()`. All
snippets use the real identifiers.

**Type consistency:** `parse` → `DesktopFile { path, sections }` used identically in Tasks 1-6; `DesktopSection::get` (case-sensitive) used in Tasks 1, 3, 5; `entry_section()` used in Tasks 1, 3, 5; `engine.desktop(id) -> Result<DesktopFile>` consumed in Task 5; `render_desktop(&DesktopFile, &[String], bool) -> String` defined and called in Task 5. `AppAction::Desktop { fields }` consistent in Tasks 5-6. ✅

**Newline convention:** Renderers return strings WITHOUT a trailing newline;
`run()` adds exactly one via `println!`. Test expectations capture `execute`'s
pre-`println!` stdout, so selected-field tests expect no trailing `\n` (verified
against the existing `get_human_prints_bare_default` test, which asserts
`"mpv.desktop"` with no newline). ✅
