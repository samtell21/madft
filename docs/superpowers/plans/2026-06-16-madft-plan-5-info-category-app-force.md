# madft Plan 5 — `info` category, `app` query, `set --force`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `category` breadcrumb to `info`, a new app-centric `app <id>` command, and a `set --force/-f` escape hatch — all additive to the completed MVP.

**Architecture:** Edits to four existing files. `categories/tree.rs` gains `category_of`. `engine.rs` gains `TypeInfo.category`, the `app` op (+`AppReport`/`AppTypeRow`), and a `force` parameter on `set` (+`SetPlan.forced`). `cli.rs` gains the `App` subcommand + `human_app` renderer, the `--force` flag, and a guard-error hint. `tests/golden.rs` gains `--json` goldens. No new dependencies; all `--json` additions are additive (existing consumers unaffected).

**Tech Stack:** Rust (edition 2024); existing deps only.

**Spec:** `docs/superpowers/specs/2026-06-16-madft-info-category-and-app-query-design.md` (amends the MVP spec `2026-06-15-madft-design.md`).

**Plan series:** Plans 1–4 = MVP ✅. Plan 5 (this doc) = post-MVP increment.

**IMPORTANT for every task:** This plan EDITS existing files. Use the exact old→new snippets. The crate uses Rust edition 2024 (valid on cargo 1.96 — do NOT change it). After each task the build must stay green; clippy `-D warnings` is gated in the final task.

---

## File structure (this plan)

- Modify `src/categories/tree.rs` — `+ category_of` (+ unit test).
- Modify `src/engine.rs` — `TypeInfo.category`; `app` op + `AppReport`/`AppTypeRow` (+ tests); `set(force)` + `SetPlan.forced` (+ tests).
- Modify `src/cli.rs` — `App` subcommand + dispatch + `human_app`; `Set` `--force` + dispatch + guard hint; updated/added unit tests.
- Modify `tests/golden.rs` — `app`, info-category, and `set --force` `--json` goldens.

---

### Task 1: `category` in `info`

**Files:** Modify `src/categories/tree.rs`, `src/engine.rs`, `src/cli.rs`

- [ ] **Step 1: Add `category_of` to `CategoryTree`** — in `src/categories/tree.rs`, inside the existing `impl CategoryTree { … }` block, add this method immediately after the `collect_types_under` method (before `len`):

```rust
    /// The id of the node that DIRECTLY places `t` (its single home), if any.
    /// Single-placement (spec §4) guarantees at most one match. `t` should be
    /// alias-canonicalized by the caller.
    pub fn category_of(&self, t: &MimeType) -> Option<CategoryId> {
        self.arena
            .iter()
            .position(|n| n.types.contains(t))
            .map(CategoryId)
    }
```

- [ ] **Step 2: Add a `category_of` unit test** — in `src/categories/tree.rs`, inside the existing `#[cfg(test)] mod tests { … }` block, add this test after `types_under_is_recursive_union` (it reuses the existing `sample()` helper: Media[0]=application/ogg, Media.Video[1]=video/mp4, Other[2]=text/plain):

```rust
    #[test]
    fn category_of_finds_the_placing_node() {
        let t = sample();
        assert_eq!(t.category_of(&MimeType::new("video/mp4")), Some(CategoryId(1)));
        assert_eq!(t.category_of(&MimeType::new("application/ogg")), Some(CategoryId(0)));
        assert_eq!(t.category_of(&MimeType::new("text/plain")), Some(CategoryId(2)));
        assert_eq!(t.category_of(&MimeType::new("nope/none")), None);
    }
```

- [ ] **Step 3: Add the `category` field to `TypeInfo`** — in `src/engine.rs`, change the struct (add the `category` line right after `mime`):

Replace:
```rust
pub struct TypeInfo {
    pub mime: String,
    pub comment: Option<String>,
```
with:
```rust
pub struct TypeInfo {
    pub mime: String,
    pub category: Option<String>,
    pub comment: Option<String>,
```

- [ ] **Step 4: Populate `category` in `info`** — in `src/engine.rs`, in the `info` method's returned struct, add the `category` line after `mime`:

Replace:
```rust
        Ok(TypeInfo {
            mime: canon.to_string(),
            comment: None,
```
with:
```rust
        Ok(TypeInfo {
            mime: canon.to_string(),
            category: self.tree.category_of(&canon).map(|id| self.tree.path(id)),
            comment: None,
```

- [ ] **Step 5: Assert `category` in the engine `info` tests** — in `src/engine.rs`'s `#[cfg(test)] mod tests`:

In `info_canonicalizes_alias`, add after the existing `assert_eq!(info.mime, "image/jpeg");`:
```rust
        assert_eq!(info.category.as_deref(), Some("Media.Images"));
```
In `info_reports_transitive_ancestors`, add after the existing ancestor assertion (svg is unplaced in the fixture categories → falls to `Other`):
```rust
        assert_eq!(info.category.as_deref(), Some("Other"));
```

- [ ] **Step 6: Render `category` in human `info`** — in `src/cli.rs`, in `human_info`, add a category line after the mime line:

Replace:
```rust
    s.push_str(&format!("{}\n", i.mime));
    if let Some(c) = &i.comment {
```
with:
```rust
    s.push_str(&format!("{}\n", i.mime));
    if let Some(cat) = &i.category {
        s.push_str(&format!("  category: {cat}\n"));
    }
    if let Some(c) = &i.comment {
```

- [ ] **Step 7: Run the tests**

Run: `cargo test --lib categories::tree:: && cargo test --lib engine::tests::info && cargo test --lib`
Expected: the new `category_of_finds_the_placing_node` passes; both `info` tests pass with the category assertions; the full lib suite stays green (now 69 lib tests).

- [ ] **Step 8: Commit**
```bash
git add src/categories/tree.rs src/engine.rs src/cli.rs
git commit -m "feat(engine): category breadcrumb in info (+CategoryTree::category_of)"
```

---

### Task 2: `set --force` / `-f`

**Files:** Modify `src/engine.rs`, `src/cli.rs`

- [ ] **Step 1: Add `forced` to `SetPlan`** — in `src/engine.rs`, change the struct:

Replace:
```rust
    pub skipped_types: Vec<String>,
    pub dry_run: bool,
    pub written: bool,
}
```
with:
```rust
    pub skipped_types: Vec<String>,
    pub forced: bool,
    pub dry_run: bool,
    pub written: bool,
}
```

- [ ] **Step 2: Replace the entire `set` method** — in `src/engine.rs`, replace the whole `pub fn set(...) -> Result<SetPlan> { ... }` with:

```rust
    /// `set <PATH|mimetype> <app> [--types …] [--force] [--dry-run]`: set `app`
    /// as the default for the umbrella types it declares. Types the app does NOT
    /// declare are reported as `skipped_types` (informational, not an error).
    /// `types_filter`, when given, restricts to that subset (alias-canonicalized).
    /// `force` overrides the exact-declaration guard — every targeted type is set
    /// regardless of declaration (so nothing is skipped). Guards with
    /// `AppHandlesNothingUnderUmbrella` if `set_types` ends up empty.
    pub fn set(
        &self,
        target: &str,
        app: &str,
        types_filter: Option<&[String]>,
        force: bool,
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
            if let Some(ref f) = filter
                && !f.contains(t)
            {
                continue; // outside the --types restriction: ignore entirely
            }
            if force || self.appindex.declares(&app_id, t) {
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
            forced: force,
            dry_run,
            written,
        })
    }
```

- [ ] **Step 3: Replace the entire `write_tests` module** — in `src/engine.rs`, replace the whole `#[cfg(test)] mod write_tests { ... }` (the set/unset write tests) with the version below (updates every `set(...)` call for the new signature + adds two force tests):

```rust
#[cfg(test)]
mod write_tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

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

    fn read_only_engine() -> Engine {
        let roots = Roots {
            data_home: fixtures().join("engine"),
            data_dirs: vec![fixtures()],
            config_home: fixtures().join("engine/config"),
            config_dirs: vec![],
        };
        Engine::load(&roots, &[]).unwrap()
    }

    #[test]
    fn set_dry_run_partitions_without_writing() {
        let e = read_only_engine();
        let plan = e.set("Media", "mpv", None, false, true).unwrap();
        assert_eq!(plan.set_types, vec!["audio/mpeg", "video/mp4", "video/x-matroska"]);
        assert_eq!(plan.skipped_types, vec!["application/ogg", "image/png", "image/jpeg"]);
        assert!(!plan.written);
        assert!(plan.dry_run);
        assert!(!plan.forced);
    }

    #[test]
    fn set_guards_when_app_handles_nothing() {
        let e = read_only_engine();
        // nvim declares text/plain + text/html — neither is under Media.
        let err = e.set("Media", "nvim", None, false, false).unwrap_err();
        assert!(matches!(err, Error::AppHandlesNothingUnderUmbrella { .. }));
    }

    #[test]
    fn set_unknown_app_errors() {
        let e = read_only_engine();
        assert!(matches!(e.set("Media", "nope", None, false, false), Err(Error::UnknownApp(_))));
    }

    #[test]
    fn set_writes_backs_up_and_is_idempotent() {
        let (e, path) = engine_with_temp_config("set");
        let plan = e.set("Media", "mpv", None, false, false).unwrap();
        assert!(plan.written);

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("audio/mpeg=mpv.desktop"));
        assert!(content.contains("video/x-matroska=mpv.desktop"));
        assert!(content.contains("video/mp4=mpv.desktop"));
        assert!(content.contains("text/html=org.qutebrowser.qutebrowser.desktop"));
        assert!(path.with_file_name("mimeapps.list.bak").exists());

        let again = e.set("Media", "mpv", None, false, false).unwrap();
        assert!(!again.written);
    }

    #[test]
    fn set_with_types_filter_restricts() {
        let (e, _path) = engine_with_temp_config("filter");
        let only = ["video/mp4".to_string()];
        let plan = e.set("Media", "mpv", Some(&only), false, true).unwrap();
        assert_eq!(plan.set_types, vec!["video/mp4"]);
        assert!(plan.skipped_types.is_empty());
    }

    #[test]
    fn unset_removes_existing_default() {
        let (e, path) = engine_with_temp_config("unset");
        let wrote = e.unset("video/mp4").unwrap();
        assert!(wrote);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("video/mp4="));
        assert!(!e.unset("video/mp4").unwrap());
    }

    #[test]
    fn set_force_sets_undeclared_type() {
        let e = read_only_engine();
        // mpv does NOT declare image/png; --force sets it anyway (dry-run).
        let plan = e.set("image/png", "mpv", None, true, true).unwrap();
        assert_eq!(plan.set_types, vec!["image/png"]);
        assert!(plan.skipped_types.is_empty());
        assert!(plan.forced);
    }

    #[test]
    fn set_force_still_errors_on_unknown_app() {
        let e = read_only_engine();
        // force overrides the declaration guard, NOT the app-exists check.
        assert!(matches!(
            e.set("image/png", "nope", None, true, false),
            Err(Error::UnknownApp(_))
        ));
    }
}
```

- [ ] **Step 4: Add the `--force` flag to the CLI `Set` variant** — in `src/cli.rs`, in the `Command` enum, change the `Set` variant:

Replace:
```rust
    Set {
        target: String,
        app: String,
        /// Restrict to a comma-separated subset of the umbrella's types.
        #[arg(long, value_delimiter = ',')]
        types: Vec<String>,
        /// Print the plan without writing.
        #[arg(long)]
        dry_run: bool,
    },
```
with:
```rust
    Set {
        target: String,
        app: String,
        /// Restrict to a comma-separated subset of the umbrella's types.
        #[arg(long, value_delimiter = ',')]
        types: Vec<String>,
        /// Set even types the app doesn't declare (override the guard).
        #[arg(short = 'f', long)]
        force: bool,
        /// Print the plan without writing.
        #[arg(long)]
        dry_run: bool,
    },
```

- [ ] **Step 5: Pass `force` through dispatch** — in `src/cli.rs`, in `run_command`, change the `Set` arm:

Replace:
```rust
        Command::Set { target, app, types, dry_run } => {
            let filter = if types.is_empty() { None } else { Some(types.as_slice()) };
            let r = engine.set(target, app, filter, *dry_run)?;
            if json { to_json(&r) } else { human_set(&r) }
        }
```
with:
```rust
        Command::Set { target, app, types, force, dry_run } => {
            let filter = if types.is_empty() { None } else { Some(types.as_slice()) };
            let r = engine.set(target, app, filter, *force, *dry_run)?;
            if json { to_json(&r) } else { human_set(&r) }
        }
```

- [ ] **Step 6: Add the guard hint to human errors** — in `src/cli.rs`, in `render_error`, change the human (else) branch:

Replace:
```rust
    } else {
        Outcome { code: 1, stdout: String::new(), stderr: format!("error: {e}") }
    }
```
with:
```rust
    } else {
        let hint = if matches!(e, Error::AppHandlesNothingUnderUmbrella { .. }) {
            " (use --force to override)"
        } else {
            ""
        };
        Outcome { code: 1, stdout: String::new(), stderr: format!("error: {e}{hint}") }
    }
```

- [ ] **Step 7: Fix the cli `Set` test literal** — in `src/cli.rs`'s `#[cfg(test)] mod tests`, the `set_dry_run_json_reports_partition` test constructs `Command::Set { … }` and must add the new field. Replace:
```rust
        let cmd = Command::Set {
            target: "Media".to_string(),
            app: "mpv".to_string(),
            types: vec![],
            dry_run: true,
        };
```
with:
```rust
        let cmd = Command::Set {
            target: "Media".to_string(),
            app: "mpv".to_string(),
            types: vec![],
            force: false,
            dry_run: true,
        };
```

- [ ] **Step 8: Run the tests**

Run: `cargo test --lib`
Expected: green (now 71 lib tests — the two new `set_force_*` tests added; all updated call sites compile).

- [ ] **Step 9: Commit**
```bash
git add src/engine.rs src/cli.rs
git commit -m "feat(set): --force/-f to override the exact-declaration guard"
```

---

### Task 3: `madft app <id>`

**Files:** Modify `src/engine.rs`, `src/cli.rs`

- [ ] **Step 1: Append the `app` result structs, op, and tests to `src/engine.rs`** — add the following at the END of `src/engine.rs` (after the `write_tests` module):

```rust

/// One declared mimetype in an `AppReport`.
#[derive(serde::Serialize, Debug)]
pub struct AppTypeRow {
    pub mime: String,
    pub category: Option<String>,
    pub is_default: bool,
    pub current_default: Option<String>,
}

/// Result of `app`: an app's declared types, where each lives, and which it is
/// currently the default for.
#[derive(serde::Serialize, Debug)]
pub struct AppReport {
    pub id: String,
    pub name: String,
    pub declares: usize,
    pub default_for: usize,
    pub types: Vec<AppTypeRow>,
}

impl Engine {
    /// `app <id>`: the app's (canonical) declared mimetypes, the category each
    /// falls in, and whether this app is currently the default for it. Rows are
    /// ordered default-first, then by mimetype. Unknown app → `UnknownApp`.
    pub fn app(&self, id: &str) -> Result<AppReport> {
        let app_id = DesktopId::new(id);
        let app = self
            .appindex
            .app(&app_id)
            .ok_or_else(|| Error::UnknownApp(app_id.to_string()))?;

        // Distinct canonical declared types.
        let mut canon: Vec<MimeType> =
            app.mimetypes.iter().map(|t| self.mimedb.canonicalize(t)).collect();
        canon.sort();
        canon.dedup();

        let mut types: Vec<AppTypeRow> = canon
            .iter()
            .map(|t| {
                let cur = self.defaults.current_default(t);
                AppTypeRow {
                    mime: t.to_string(),
                    category: self.tree.category_of(t).map(|cid| self.tree.path(cid)),
                    is_default: cur.as_ref() == Some(&app_id),
                    current_default: cur.map(|d| d.to_string()),
                }
            })
            .collect();
        types.sort_by(|a, b| b.is_default.cmp(&a.is_default).then_with(|| a.mime.cmp(&b.mime)));

        let default_for = types.iter().filter(|r| r.is_default).count();
        Ok(AppReport {
            id: app_id.to_string(),
            name: app.name.clone(),
            declares: types.len(),
            default_for,
            types,
        })
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;
    use std::path::PathBuf;

    fn engine() -> Engine {
        let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let roots = Roots {
            data_home: f.join("engine"),
            data_dirs: vec![f.clone()],
            config_home: f.join("engine/config"),
            config_dirs: vec![],
        };
        Engine::load(&roots, &[]).unwrap()
    }

    #[test]
    fn app_reports_declared_types_defaults_and_categories() {
        let e = engine();
        let r = e.app("mpv").unwrap();
        assert_eq!(r.id, "mpv.desktop");
        // mpv declares video/mp4, video/x-matroska, audio/mpeg.
        assert_eq!(r.declares, 3);
        // fixture mimeapps.list has video/mp4=mpv.desktop → default for exactly 1.
        assert_eq!(r.default_for, 1);
        // ordering: the one it's default for comes first.
        assert_eq!(r.types[0].mime, "video/mp4");
        assert!(r.types[0].is_default);
        assert_eq!(r.types[0].category.as_deref(), Some("Media.Video"));
        // a non-default declared type, with its category.
        let audio = r.types.iter().find(|t| t.mime == "audio/mpeg").unwrap();
        assert!(!audio.is_default);
        assert_eq!(audio.category.as_deref(), Some("Media.Audio"));
    }

    #[test]
    fn app_unknown_errors() {
        assert!(matches!(engine().app("nope"), Err(Error::UnknownApp(_))));
    }
}
```

- [ ] **Step 2: Import `AppReport` in the CLI** — in `src/cli.rs`, add `AppReport` to the engine `use`:

Replace:
```rust
use crate::engine::{AppsResult, Engine, LsResult, SetPlan, TypeInfo};
```
with:
```rust
use crate::engine::{AppReport, AppsResult, Engine, LsResult, SetPlan, TypeInfo};
```

- [ ] **Step 3: Add the `App` subcommand** — in `src/cli.rs`, in the `Command` enum, add this variant (e.g. right after the `Apps { target }` variant):

```rust
    /// Show one app's declared types, their categories, and what it's default for.
    App { id: String },
```

- [ ] **Step 4: Dispatch `App`** — in `src/cli.rs`, in `run_command`'s match, add an arm (e.g. after the `Command::Apps` arm):

```rust
        Command::App { id } => {
            let r = engine.app(id)?;
            if json { to_json(&r) } else { human_app(&r) }
        }
```

- [ ] **Step 5: Add the `human_app` renderer** — in `src/cli.rs`, add this function next to the other `human_*` renderers (e.g. after `human_apps`):

```rust
fn human_app(r: &AppReport) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{} ({}) — declares {} types, default for {}:\n",
        r.id, r.name, r.declares, r.default_for
    ));
    for t in &r.types {
        let star = if t.is_default { "★" } else { " " };
        let cat = t.category.as_deref().unwrap_or("—");
        let def = t.current_default.as_deref().unwrap_or("—");
        s.push_str(&format!("  {star} {}  [{cat}]  (default: {def})\n", t.mime));
    }
    s.trim_end().to_string()
}
```

- [ ] **Step 6: Add a CLI `app` test** — in `src/cli.rs`'s `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn app_json_reports_rows() {
        let out = execute(&engine(), &Command::App { id: "mpv".to_string() }, true);
        assert_eq!(out.code, 0);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["id"], "mpv.desktop");
        assert_eq!(v["declares"], 3);
        assert_eq!(v["default_for"], 1);
        assert_eq!(v["types"][0]["mime"], "video/mp4");
        assert_eq!(v["types"][0]["is_default"], true);
        assert_eq!(v["types"][0]["category"], "Media.Video");
    }
```

- [ ] **Step 7: Run the tests**

Run: `cargo test --lib`
Expected: green (now 74 lib tests — `app_reports_*`, `app_unknown_errors`, `app_json_reports_rows` added).

- [ ] **Step 8: Sanity-check the new command on the binary**

Run: `cargo run -- app --help` (should show the `app` subcommand help; exit 0). No assertion needed.

- [ ] **Step 9: Commit**
```bash
git add src/engine.rs src/cli.rs
git commit -m "feat(cli): app <id> command — declared types, categories, defaults"
```

---

### Task 4: Golden integration + final gate

**Files:** Modify `tests/golden.rs`

- [ ] **Step 1: Append the new goldens to `tests/golden.rs`** — add these tests at the END of `tests/golden.rs` (they reuse the existing `read_engine` / `parse` helpers):

```rust

#[test]
fn golden_info_includes_category_json() {
    let cli = parse(&["madft", "info", "video/mp4", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["mime"], "video/mp4");
    assert_eq!(v["category"], "Media.Video");
}

#[test]
fn golden_app_json() {
    let cli = parse(&["madft", "app", "mpv", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["id"], "mpv.desktop");
    assert_eq!(v["declares"], 3);
    assert_eq!(v["default_for"], 1);
    assert_eq!(v["types"][0]["mime"], "video/mp4");
    assert_eq!(v["types"][0]["is_default"], true);
    assert_eq!(v["types"][0]["category"], "Media.Video");
}

#[test]
fn golden_set_force_overrides_guard() {
    // mpv does not declare image/png: rejected without --force, set with it.
    let reject = parse(&["madft", "set", "image/png", "mpv", "--json"]);
    let out = execute(&read_engine(), &reject.command, reject.json);
    assert_eq!(out.code, 1);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "app-handles-nothing-under-umbrella");

    let forced = parse(&["madft", "set", "image/png", "mpv", "--force", "--dry-run", "--json"]);
    let out2 = execute(&read_engine(), &forced.command, forced.json);
    assert_eq!(out2.code, 0);
    let v2: serde_json::Value = serde_json::from_str(&out2.stdout).unwrap();
    assert_eq!(v2["forced"], serde_json::json!(true));
    assert_eq!(v2["set_types"], serde_json::json!(["image/png"]));
    assert_eq!(v2["skipped_types"], serde_json::json!([]));
}
```

- [ ] **Step 2: Run the golden tests**

Run: `cargo test --test golden`
Expected: PASS (now 9 golden tests).

- [ ] **Step 3: Run the FULL suite + clippy (required gate)**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all tests PASS — 74 lib + 9 golden = 83. No clippy warnings. If clippy flags anything, fix it minimally inline and re-run until clean. (Edition 2024 is valid — do not change it.)

- [ ] **Step 4: Commit**
```bash
git add tests/golden.rs
git commit -m "test(golden): info category, app query, and set --force"
```

---

## Plan 5 Self-Review (completed during authoring)

- **Spec coverage:** §1 `category` in `info` → Task 1 (field + `category_of`). §2 `app <id>` command → Task 3 (engine op + structs + CLI). §3 `set --force` → Task 2 (param + `forced` + flag + hint). Golden `--json` for all three → Task 4.
- **Build-green discipline:** the only signature change (`set` gains `force`) is contained in Task 2, which updates ALL call sites in the same task (engine `write_tests` via full-module replacement, cli `run_command` arm, and the cli `Set` struct-literal test). The `TypeInfo.category` field (Task 1) and the `App` enum variant (Task 3) are additive but still update their consumers (engine `info` constructor / `run_command` match) in the same task.
- **Placeholder scan:** none — every step is an exact edit or complete code block with run/commit commands.
- **Type consistency:** `category_of(&MimeType) -> Option<CategoryId>` (Task 1) is consumed by `info` (Task 1) and `app` (Task 3). `TypeInfo.category`, `SetPlan.forced`, `AppReport`/`AppTypeRow`, `Command::App`, and the `set(…, force, dry_run)` signature line up across engine ↔ cli ↔ golden. Reuses MVP APIs (`tree.path`, `appindex.{app,declares}`, `defaults.current_default`, `mimedb.canonicalize`, `writer`).
- **Determinism:** `app.types` sorted (default-first, then mime); `category` is the deterministic tree path. No new read-dir surface. `--json` additions (`category`, `forced`, the whole `app` schema) are additive.

## Done criteria for Plan 5

`cargo test` green (74 lib + 9 golden = 83), `cargo clippy --all-targets -- -D warnings` clean, `madft info <type>` shows its `category`, `madft app <id>` lists declared types with category/default/owner (human + `--json`), and `madft set <type> <app> --force` sets a non-declared default while the un-forced form is rejected with a hint. All additive — existing `--json` consumers unaffected.
