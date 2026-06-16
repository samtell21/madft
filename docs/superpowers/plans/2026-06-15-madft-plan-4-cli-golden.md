# madft Plan 4 — CLI + Golden Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the engine to a clap CLI with the 7 subcommands, human + `--json` rendering (the stable ptui contract) and the `{"error":{kind,message}}` envelope, replace the stub `main`, and lock the behavior with golden integration tests (including the named mpv-in-Media `--json` scenario).

**Architecture:** One new `src/cli.rs` module + a real `src/main.rs`. clap (derive) parses argv into a `Cli { json, command }`; `--json` is a global flag (valid before or after the subcommand). The testable core `execute(engine, command, json) -> Outcome { code, stdout, stderr }` dispatches to the Plan 3 engine and renders each result struct either as human text or, via serde_json, as the stable JSON schema. Errors map to a kebab-case `kind` + message: JSON errors print the envelope on stdout (so a `--json` consumer always parses stdout), human errors print to stderr; both exit non-zero. `run()` is the thin binary entry (reads `Roots::from_env()` + `$XDG_CURRENT_DESKTOP`, prints, returns the exit code); `main` is `std::process::exit(madft::cli::run())`.

**Tech Stack:** Rust (edition 2024); existing `thiserror`/`toml`/`serde`; new deps `clap = { version = "4", features = ["derive"] }` (4.6.1) and `serde_json = "1"` (1.0.150). Golden tests live in `tests/golden.rs` (an integration test against the public API) and exercise the CLI via `Cli::try_parse_from(...)` + `execute(...)`.

**Design decisions (continuing the series):**
- `--json` errors print on **stdout** (machine consumer reads stdout uniformly); human errors on **stderr**. All engine/load errors exit `1` (clap's own arg/usage errors exit `2`).
- `get` is scriptable: bare default on stdout, empty + exit `0` when unset.
- Rendering reads only the Plan 3 `Serialize` structs; list ordering is already deterministic there (no new read-dir leakage).

**Plan series:** Plan 1 (facts) ✅ → Plan 2 (categories) ✅ → Plan 3 (engine + writer) ✅ → Plan 4 (cli + golden, this doc — the MVP finish line).

**Spec:** `docs/superpowers/specs/2026-06-15-madft-design.md`. Implements the `cli` module row of §3, the full command surface of §5 (rendering — both human and `--json`), the error envelope + exit-code rules of §5/§7, and the golden `--json` integration of §8 (the mpv-in-Media named test).

---

## File structure (this plan)

- `Cargo.toml` — add `clap = { version = "4", features = ["derive"] }` and `serde_json = "1"`.
- `src/lib.rs` — add `pub mod cli;`.
- `src/cli.rs` — clap `Cli`/`Command`, `Outcome`, `execute`, human + JSON renderers, error envelope, `run`.
- `src/main.rs` — replace the stub with `std::process::exit(madft::cli::run())`.
- `tests/golden.rs` — golden integration tests over the public API (the mpv-in-Media `--json` named test + read-op JSON + error-envelope + real-write assertions).
- Reused: the `tests/fixtures/engine/` tree from Plan 3.

**Key signatures (defined once):**
- `Cli { json: bool, command: Command }` (clap `Parser`); `Command` (clap `Subcommand`): `Ls{path: Option<String>}`, `Types{path: String}`, `Info{mimetype: String}`, `Apps{target: String}`, `Set{target, app, types: Vec<String>, dry_run: bool}`, `Unset{mimetype: String}`, `Get{mimetype: String}`.
- `pub struct Outcome { code: i32, stdout: String, stderr: String }`.
- `pub fn execute(engine: &Engine, command: &Command, json: bool) -> Outcome`.
- `pub fn run() -> i32`.

---

### Task 1: Scaffold the cli module

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/cli.rs`

- [ ] **Step 1: Add the dependencies**

Run: `cargo add clap --features derive` then `cargo add serde_json`
Expected: `Cargo.toml` gains `clap = { version = "4", features = ["derive"] }` and `serde_json = "1"`; `Cargo.lock` updates (clap 4.6.1, serde_json 1.0.150). If `cargo add` is unavailable, add manually under `[dependencies]`:
```toml
clap = { version = "4", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 2: Declare the module in `src/lib.rs`**

Add `pub mod cli;` as the last line. After the edit the module list reads:
```rust
pub mod types;
pub mod error;
pub mod paths;
pub mod mimedb;
pub mod appindex;
pub mod defaults;
pub mod categories;
pub mod writer;
pub mod engine;
pub mod cli;
```
(Keep the existing crate-level `//!` doc comment lines at the top unchanged.)

- [ ] **Step 3: Create the stub file**

Create `src/cli.rs` with the single line `// implemented in a later task`. (Leave `src/main.rs` as-is for now — it is replaced in Task 2.)

- [ ] **Step 4: Verify it builds**

Run: `cargo build`
Expected: compiles (clippy not gated here).

- [ ] **Step 5: Commit**
```bash
git add Cargo.toml Cargo.lock src/lib.rs src/cli.rs
git commit -m "scaffold: cli module + clap and serde_json deps"
```

---

### Task 2: `cli.rs` — clap surface, rendering, error envelope, `run` + `main`

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write the implementation + tests** (replace the entire contents of `src/cli.rs`)

```rust
//! The clap CLI: subcommands, argument parsing, and human vs `--json` rendering
//! over the engine. This is the stable machine-facing surface a future TUI
//! (ptui) shells out to (spec §1, §5). `run()` is the binary entry point;
//! `execute()` is the testable core that returns rendered output + an exit code.

use clap::{Parser, Subcommand};

use crate::engine::{AppsResult, Engine, LsResult, SetPlan, TypeInfo};
use crate::error::Error;
use crate::paths::Roots;

#[derive(Parser, Debug)]
#[command(
    name = "madft",
    about = "Inspect and set XDG default applications via a curated category tree"
)]
pub struct Cli {
    /// Emit machine-readable JSON instead of human text.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List child categories and leaf types at a category path (root if omitted).
    Ls { path: Option<String> },
    /// List all mimetypes under a category path (recursive).
    Types { path: String },
    /// Show details for a mimetype.
    Info { mimetype: String },
    /// List apps that can handle a category path or mimetype.
    Apps { target: String },
    /// Set an app as the default for a category path or mimetype.
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
    /// Remove the user default for a mimetype.
    Unset { mimetype: String },
    /// Print the bare current default for a mimetype (scriptable).
    Get { mimetype: String },
}

/// Captured result of a command: output streams + the process exit code.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".to_string())
}

/// Stable kebab-case error kind for the `--json` envelope (spec §7).
fn error_kind(e: &Error) -> &'static str {
    match e {
        Error::UnknownPath(_) => "unknown-path",
        Error::UnknownApp(_) => "unknown-app",
        Error::AppHandlesNothingUnderUmbrella { .. } => "app-handles-nothing-under-umbrella",
        Error::InvalidCategoryName(_) => "invalid-category-name",
        Error::DuplicatePlacement { .. } => "duplicate-placement",
        Error::MimeDbNotFound(_) => "mime-db-not-found",
        Error::Io(_) => "io",
        Error::Parse { .. } => "parse",
    }
}

fn render_error(e: &Error, json: bool) -> Outcome {
    if json {
        let body = serde_json::json!({
            "error": { "kind": error_kind(e), "message": e.to_string() }
        });
        Outcome { code: 1, stdout: to_json(&body), stderr: String::new() }
    } else {
        Outcome { code: 1, stdout: String::new(), stderr: format!("error: {e}") }
    }
}

/// Dispatch one command and render its stdout (or propagate an engine error).
fn run_command(engine: &Engine, command: &Command, json: bool) -> Result<String, Error> {
    let out = match command {
        Command::Ls { path } => {
            let r = engine.ls(path.as_deref())?;
            if json { to_json(&r) } else { human_ls(&r) }
        }
        Command::Types { path } => {
            let r = engine.types(path)?;
            if json { to_json(&r) } else { r.join("\n") }
        }
        Command::Info { mimetype } => {
            let r = engine.info(mimetype)?;
            if json { to_json(&r) } else { human_info(&r) }
        }
        Command::Apps { target } => {
            let r = engine.apps(target)?;
            if json { to_json(&r) } else { human_apps(&r) }
        }
        Command::Set { target, app, types, dry_run } => {
            let filter = if types.is_empty() { None } else { Some(types.as_slice()) };
            let r = engine.set(target, app, filter, *dry_run)?;
            if json { to_json(&r) } else { human_set(&r) }
        }
        Command::Unset { mimetype } => {
            let wrote = engine.unset(mimetype)?;
            if json {
                to_json(&serde_json::json!({ "unset": mimetype, "written": wrote }))
            } else if wrote {
                format!("unset {mimetype}")
            } else {
                format!("{mimetype}: no user default to remove")
            }
        }
        Command::Get { mimetype } => {
            let d = engine.get(mimetype);
            if json {
                to_json(&serde_json::json!({ "default": d }))
            } else {
                d.unwrap_or_default()
            }
        }
    };
    Ok(out)
}

/// Run a command against the engine and capture the rendered output + exit code.
pub fn execute(engine: &Engine, command: &Command, json: bool) -> Outcome {
    match run_command(engine, command, json) {
        Ok(stdout) => Outcome { code: 0, stdout, stderr: String::new() },
        Err(e) => render_error(&e, json),
    }
}

fn human_ls(r: &LsResult) -> String {
    let mut s = String::new();
    for sub in &r.subcategories {
        s.push_str(&format!("{sub}/\n"));
    }
    for t in &r.types {
        let def = t.current_default.as_deref().unwrap_or("(none)");
        s.push_str(&format!("{}  [default: {def}, apps: {}]\n", t.mime, t.applicable_count));
    }
    s.trim_end().to_string()
}

fn human_info(i: &TypeInfo) -> String {
    let mut s = String::new();
    s.push_str(&format!("{}\n", i.mime));
    if let Some(c) = &i.comment {
        s.push_str(&format!("  comment: {c}\n"));
    }
    s.push_str(&format!("  default: {}\n", i.current_default.as_deref().unwrap_or("(none)")));
    s.push_str(&format!("  applicable apps: {}\n", i.applicable_count));
    for a in &i.applicable_apps {
        s.push_str(&format!("    - {} ({})\n", a.id, a.name));
    }
    if !i.ancestor_types.is_empty() {
        s.push_str(&format!("  inherits if unset: {}\n", i.ancestor_types.join(", ")));
    }
    s.trim_end().to_string()
}

fn human_apps(r: &AppsResult) -> String {
    let mut s = String::new();
    s.push_str(&format!("apps for {} ({} types):\n", r.target, r.types.len()));
    for a in &r.apps {
        s.push_str(&format!(
            "  {} ({}) — {}/{}: {}\n",
            a.id,
            a.name,
            a.coverage,
            r.types.len(),
            a.declared_types.join(", ")
        ));
    }
    s.trim_end().to_string()
}

fn human_set(p: &SetPlan) -> String {
    let mut s = String::new();
    let verb = if p.dry_run {
        "would set"
    } else if p.written {
        "set"
    } else {
        "already set"
    };
    s.push_str(&format!(
        "{verb} {} as default for {} ({} types):\n",
        p.app,
        p.target,
        p.set_types.len()
    ));
    for t in &p.set_types {
        s.push_str(&format!("  + {t}\n"));
    }
    if !p.skipped_types.is_empty() {
        s.push_str(&format!(
            "skipped (not declared by {}): {}\n",
            p.app,
            p.skipped_types.join(", ")
        ));
    }
    s.trim_end().to_string()
}

/// The lowercased `$XDG_CURRENT_DESKTOP` list (for mimeapps.list precedence).
fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// Binary entry point: parse argv, build the engine from the live environment,
/// print the rendered output, and return the process exit code.
pub fn run() -> i32 {
    let cli = Cli::parse();
    let roots = Roots::from_env();
    let outcome = match Engine::load(&roots, &current_desktops()) {
        Ok(engine) => execute(&engine, &cli.command, cli.json),
        Err(e) => render_error(&e, cli.json),
    };
    if !outcome.stdout.is_empty() {
        println!("{}", outcome.stdout);
    }
    if !outcome.stderr.is_empty() {
        eprintln!("{}", outcome.stderr);
    }
    outcome.code
}

#[cfg(test)]
mod tests {
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
    fn ls_root_human_lists_categories() {
        let out = execute(&engine(), &Command::Ls { path: None }, false);
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("Media/"));
        assert!(out.stdout.contains("Web/"));
        assert!(out.stdout.contains("Other/"));
    }

    #[test]
    fn ls_root_json_has_sorted_subcategories() {
        let out = execute(&engine(), &Command::Ls { path: None }, true);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["subcategories"], serde_json::json!(["Media", "Other", "Web"]));
    }

    #[test]
    fn types_human_is_one_per_line() {
        let out = execute(&engine(), &Command::Types { path: "Media".to_string() }, false);
        assert_eq!(
            out.stdout,
            "application/ogg\naudio/mpeg\nimage/png\nimage/jpeg\nvideo/mp4\nvideo/x-matroska"
        );
    }

    #[test]
    fn info_json_canonicalizes_alias() {
        let out = execute(&engine(), &Command::Info { mimetype: "image/jpg".to_string() }, true);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["mime"], "image/jpeg");
        assert_eq!(v["comment"], serde_json::Value::Null);
    }

    #[test]
    fn get_human_prints_bare_default() {
        let out = execute(&engine(), &Command::Get { mimetype: "video/mp4".to_string() }, false);
        assert_eq!(out.stdout, "mpv.desktop");
        assert_eq!(out.code, 0);
    }

    #[test]
    fn unknown_path_json_error_envelope() {
        let out = execute(&engine(), &Command::Ls { path: Some("Nope".to_string()) }, true);
        assert_eq!(out.code, 1);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["error"]["kind"], "unknown-path");
        assert!(v["error"]["message"].as_str().unwrap().contains("Nope"));
    }

    #[test]
    fn unknown_path_human_error_to_stderr() {
        let out = execute(&engine(), &Command::Ls { path: Some("Nope".to_string()) }, false);
        assert_eq!(out.code, 1);
        assert!(out.stdout.is_empty());
        assert!(out.stderr.contains("error:"));
    }

    #[test]
    fn set_dry_run_json_reports_partition() {
        let cmd = Command::Set {
            target: "Media".to_string(),
            app: "mpv".to_string(),
            types: vec![],
            dry_run: true,
        };
        let out = execute(&engine(), &cmd, true);
        assert_eq!(out.code, 0);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["set_types"], serde_json::json!(["audio/mpeg", "video/mp4", "video/x-matroska"]));
        assert_eq!(v["skipped_types"], serde_json::json!(["application/ogg", "image/png", "image/jpeg"]));
        assert_eq!(v["written"], serde_json::json!(false));
    }
}
```

- [ ] **Step 2: Replace the entire contents of `src/main.rs` with:**

```rust
fn main() {
    std::process::exit(madft::cli::run());
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --lib cli::`
Expected: PASS (8 tests).

- [ ] **Step 4: Sanity-check the real binary builds and runs**

Run: `cargo run -- --help`
Expected: clap prints usage with the 7 subcommands and the `--json` flag; exit code 0. (This just confirms `main` is wired; no assertion needed.)

- [ ] **Step 5: Commit**
```bash
git add src/cli.rs src/main.rs
git commit -m "feat(cli): clap subcommands, human + --json rendering, error envelope"
```

---

### Task 3: Golden integration tests + final gate

**Files:**
- Create: `tests/golden.rs`

- [ ] **Step 1: Write the golden integration test** (create `tests/golden.rs`)

```rust
//! Golden integration tests: drive the CLI exactly as a caller would
//! (`Cli::try_parse_from` → `execute`) against the committed engine fixture
//! tree, and assert the stable `--json` schema. Includes the named
//! mpv-in-Media scenario (spec §8): sets the declared video/audio types,
//! reports the images skipped, writes nothing for them.

use std::path::PathBuf;

use madft::cli::{execute, Cli};
use madft::engine::Engine;
use madft::paths::Roots;
use clap::Parser;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Read-only engine over the committed fixtures.
fn read_engine() -> Engine {
    let f = fixtures();
    let roots = Roots {
        data_home: f.join("engine"),
        data_dirs: vec![f.clone()],
        config_home: f.join("engine/config"),
        config_dirs: vec![],
    };
    Engine::load(&roots, &[]).unwrap()
}

/// Engine whose writes go to a disposable temp config seeded from the fixture.
fn writable_engine(tag: &str) -> (Engine, PathBuf) {
    let f = fixtures();
    let cfg = std::env::temp_dir().join(format!("madft-golden-{tag}"));
    let _ = std::fs::remove_dir_all(&cfg);
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::copy(
        f.join("engine/config/mimeapps.list"),
        cfg.join("mimeapps.list"),
    )
    .unwrap();
    let roots = Roots {
        data_home: f.join("engine"),
        data_dirs: vec![f.clone()],
        config_home: cfg.clone(),
        config_dirs: vec![],
    };
    (Engine::load(&roots, &[]).unwrap(), cfg.join("mimeapps.list"))
}

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(args).expect("parse args")
}

#[test]
fn golden_mpv_in_media_dry_run_json() {
    // The named scenario: `madft set Media mpv --dry-run --json`.
    let cli = parse(&["madft", "set", "Media", "mpv", "--dry-run", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["app"], "mpv.desktop");
    assert_eq!(v["target"], "Media");
    assert_eq!(v["set_types"], serde_json::json!(["audio/mpeg", "video/mp4", "video/x-matroska"]));
    assert_eq!(v["skipped_types"], serde_json::json!(["application/ogg", "image/png", "image/jpeg"]));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    assert_eq!(v["written"], serde_json::json!(false));
}

#[test]
fn golden_set_writes_file_and_preserves_unrelated() {
    let (engine, path) = writable_engine("set");
    let cli = parse(&["madft", "--json", "set", "Media", "mpv"]);
    let out = execute(&engine, &cli.command, cli.json);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["written"], serde_json::json!(true));

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("audio/mpeg=mpv.desktop"));
    assert!(content.contains("video/x-matroska=mpv.desktop"));
    assert!(content.contains("video/mp4=mpv.desktop"));
    // The unrelated existing default survives; no image lines were written.
    assert!(content.contains("text/html=org.qutebrowser.qutebrowser.desktop"));
    assert!(!content.contains("image/png="));
    assert!(!content.contains("image/jpeg="));
    // Backup of the pre-write file exists.
    assert!(path.with_file_name("mimeapps.list.bak").exists());
}

#[test]
fn golden_ls_root_json() {
    let cli = parse(&["madft", "ls", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["subcategories"], serde_json::json!(["Media", "Other", "Web"]));
    assert_eq!(v["types"], serde_json::json!([]));
}

#[test]
fn golden_apps_coverage_sorted_json() {
    let cli = parse(&["madft", "apps", "Media", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let ids: Vec<&str> = v["apps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["mpv.desktop", "eog.desktop", "webcam.desktop"]);
}

#[test]
fn golden_guard_error_envelope_json() {
    // nvim declares nothing under Media -> guard error.
    let cli = parse(&["madft", "set", "Media", "nvim", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json);
    assert_eq!(out.code, 1);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "app-handles-nothing-under-umbrella");
}

#[test]
fn golden_get_is_scriptable() {
    let cli = parse(&["madft", "get", "video/mp4"]);
    let out = execute(&read_engine(), &cli.command, cli.json);
    assert_eq!(out.stdout, "mpv.desktop");
    assert_eq!(out.code, 0);
}
```

- [ ] **Step 2: Run the golden tests**

Run: `cargo test --test golden`
Expected: PASS (6 tests).

- [ ] **Step 3: Run the FULL suite + clippy (required gate)**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all tests PASS — Plan 1 (17) + Plan 2 (19) + Plan 3 (24) + this plan's lib `cli::` (8) = 68 lib tests, plus the 6 `golden` integration tests. No clippy warnings. If clippy flags anything, fix it minimally inline and re-run until clean. (Edition 2024 is valid on this toolchain — do not change it.)

- [ ] **Step 4: Commit**
```bash
git add tests/golden.rs
git commit -m "test(golden): CLI --json integration incl. mpv-in-Media scenario"
```

---

## Plan 4 Self-Review (completed during authoring)

- **Spec coverage:**
  - §3 `cli` row (clap subcommands; human vs `--json` render) → Tasks 1–2.
  - §5 command surface — all 7 commands rendered both human and `--json`, `set` with `--types`/`--dry-run`, partial-coverage reporting, `get` scriptable → Task 2.
  - §5/§7 error model — typed errors → `{"error":{kind,message}}` envelope (JSON on stdout) or human stderr; exit `0` success / `1` on guard/unknown/load error (clap handles `2` for usage) → Task 2.
  - §8 golden/integration — `tests/golden.rs` drives the CLI via parsed argv + `execute`, asserts the `--json` schema, and includes the **named mpv-in-Media** test (sets video/audio, reports images skipped, writes nothing for images) plus a real-write assertion (file written, unrelated default preserved, `.bak` made) → Task 3.
- **Out of scope (deferred seams, unchanged):** ptui wiring, `RemoteSource`, `comment(t)`/reverse-alias listing, `[Added]`/`[Removed]` management, fuzzy app-name matching, file locking (spec §9).
- **Placeholder scan:** none — every step ships complete code and exact commands.
- **Type consistency:** `Cli`/`Command`/`Outcome`/`execute`/`run` defined in Task 2 and consumed by `main` (Task 2) and `tests/golden.rs` (Task 3). Rendering reads only the Plan 3 `Serialize` structs (`LsResult`/`LeafType`/`TypeInfo`/`AppRef`/`AppsResult`/`AppCoverage`/`SetPlan`) and `Engine::{ls,types,info,apps,set,unset,get,load}`; errors map over the Plan 1 `Error` variants. Determinism is inherited from Plan 3 (no new read-dir-order surface).
- **JSON-stability check:** the `--json` outputs are the serde-serialized Plan 3 structs verbatim (field names = struct fields), so the schema is exactly what Plan 3 froze; the error envelope is the only hand-built JSON and is asserted in golden tests.

## Done criteria for Plan 4 (and the MVP)

`cargo test` green (68 lib + 6 golden), `cargo clippy --all-targets -- -D warnings` clean, `cargo run -- --help` shows the 7 subcommands, and the binary can run every command against the live environment with human or `--json` output and the documented exit codes. The golden mpv-in-Media `--json` scenario passes. With Plans 1–4 merged, the madft MVP is complete: a working CLI that inspects and sets XDG default applications over a curated, total category tree, with exact-declaration semantics, correct XDG precedence, atomic backed-up writes, and a stable machine-facing JSON contract.
