//! The clap CLI: subcommands, argument parsing, and human vs `--json` rendering
//! over the engine. This is the stable machine-facing surface a future TUI
//! (ptui) shells out to (spec §1, §5). `run()` is the binary entry point;
//! `execute()` is the testable core that returns rendered output + an exit code.

use clap::{Parser, Subcommand};

use crate::engine::{AppReport, AppsResult, Engine, LsResult, SetOptions, SetPlan, TypeInfo};
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

    /// Show the full taxonomy, including types/categories with no installed app.
    #[arg(short = 'a', long, global = true)]
    pub all: bool,

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
    /// List apps that can handle a category path or mimetype (root if omitted).
    Apps { target: Option<String> },
    /// Show one app's declared types, their categories, and what it's default for.
    App {
        id: String,
        #[command(subcommand)]
        action: Option<AppAction>,
    },
    /// Set an app as the default for a category path or mimetype (root if omitted).
    Set {
        app: String,
        target: Option<String>,
        /// Restrict to a comma-separated subset of the umbrella's types.
        #[arg(long, value_delimiter = ',')]
        types: Vec<String>,
        /// Set even types the app doesn't declare (override the guard).
        #[arg(short = 'f', long)]
        force: bool,
        /// Only set types that currently have no default (don't overwrite).
        #[arg(long, visible_alias = "if-unset")]
        no_clobber: bool,
        /// Only set types the app declares EXACTLY (don't follow the subclass tree).
        #[arg(long)]
        exact: bool,
        /// Print the plan without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove the user default for a mimetype.
    Unset { mimetype: String },
    /// Print the bare current default for a mimetype (scriptable).
    Get { mimetype: String },
    /// Write the built-in default category tree to ~/.local/share/madft/categories.toml.
    Init {
        /// Overwrite an existing categories.toml.
        #[arg(short = 'f', long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum AppAction {
    /// Show the parsed .desktop file, or select specific fields
    Desktop {
        /// Specific keys to print (case-sensitive, from [Desktop Entry])
        fields: Vec<String>,
    },
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

/// Parse a newline-delimited mimetype list from stdin. Each line is trimmed;
/// blank/whitespace-only lines are dropped. Unlike `--types` (comma-split flag
/// value), stdin is a stream split on newlines — no comma/comment/quote magic.
fn parse_type_lines(input: &str) -> Vec<String> {
    input
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
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
        Error::ConflictingTypeSource => "conflicting-type-source",
        Error::EmptyTypeList => "empty-type-list",
        Error::MissingMimetype => "missing-mimetype",
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
        let hint = if matches!(e, Error::AppHandlesNothingUnderUmbrella { .. }) {
            " (use --force to override)"
        } else {
            ""
        };
        Outcome { code: 1, stdout: String::new(), stderr: format!("error: {e}{hint}") }
    }
}

/// Dispatch one command and render its stdout (or propagate an engine error).
fn run_command(engine: &Engine, command: &Command, json: bool, show_all: bool) -> Result<String, Error> {
    let out = match command {
        Command::Ls { path } => {
            let r = engine.ls(path.as_deref(), show_all)?;
            if json { to_json(&r) } else { human_ls(&r, show_all) }
        }
        Command::Types { path } => {
            let r = engine.types(path, show_all)?;
            if json { to_json(&r) } else { r.join("\n") }
        }
        Command::Info { mimetype } => {
            let r = engine.info(mimetype)?;
            if json { to_json(&r) } else { human_info(&r) }
        }
        Command::Apps { target } => {
            let r = engine.apps(target.as_deref(), show_all)?;
            if json { to_json(&r) } else { human_apps(&r) }
        }
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
        Command::Set { app, target, types, force, no_clobber, exact, dry_run } => {
            let filter = if types.is_empty() { None } else { Some(types.as_slice()) };
            let opts = SetOptions { force: *force, no_clobber: *no_clobber, exact: *exact, show_all, dry_run: *dry_run };
            let r = engine.set(app, target.as_deref(), filter, opts)?;
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
        Command::Init { .. } => unreachable!("`init` is handled in run() before the engine is built"),
    };
    Ok(out)
}

/// Run a command against the engine and capture the rendered output + exit code.
pub fn execute(engine: &Engine, command: &Command, json: bool, show_all: bool) -> Outcome {
    match run_command(engine, command, json, show_all) {
        Ok(stdout) => Outcome { code: 0, stdout, stderr: String::new() },
        Err(e) => render_error(&e, json),
    }
}

fn human_ls(r: &LsResult, show_all: bool) -> String {
    if r.subcategories.is_empty() && r.types.is_empty() {
        if show_all {
            return "(empty)".to_string();
        }
        let where_ = if r.path.is_empty() { String::new() } else { format!(" under {}", r.path) };
        return format!("(nothing installed{where_} — use --all to see the full taxonomy)");
    }
    let both = !r.subcategories.is_empty() && !r.types.is_empty();
    let indent = if both { "  " } else { "" };
    let mut s = String::new();
    if both {
        s.push_str("categories:\n");
    }
    for sub in &r.subcategories {
        s.push_str(&format!("{indent}{sub}\n"));
    }
    if both {
        s.push_str("types:\n");
    }
    for t in &r.types {
        let def = match &t.default {
            Some(d) => match &d.via {
                Some(v) => format!("{} (via {v})", d.app),
                None => d.app.clone(),
            },
            None => "(none)".to_string(),
        };
        let inh = if t.inheritable_count > 0 {
            format!(", +{} via inherit", t.inheritable_count)
        } else {
            String::new()
        };
        s.push_str(&format!(
            "{indent}{}  [default: {def}, apps: {}{inh}]\n",
            t.mime, t.applicable_count
        ));
    }
    s.trim_end().to_string()
}

fn human_info(i: &TypeInfo) -> String {
    let mut s = String::new();
    s.push_str(&format!("{}\n", i.mime));
    if let Some(cat) = &i.category {
        s.push_str(&format!("  category: {cat}\n"));
    }
    if let Some(c) = &i.comment {
        s.push_str(&format!("  comment: {c}\n"));
    }
    let def = match &i.default {
        Some(d) => match &d.via {
            Some(v) => format!("{} (via {v})", d.app),
            None => d.app.clone(),
        },
        None => "(none)".to_string(),
    };
    s.push_str(&format!("  default: {def}\n"));
    s.push_str(&format!("  applicable apps: {}\n", i.applicable_count));
    for a in &i.applicable_apps {
        s.push_str(&format!("    - {} ({})\n", a.id, a.name));
    }
    if !i.inheritable_apps.is_empty() {
        s.push_str(&format!("  inheritable apps: {}\n", i.inheritable_apps.len()));
        for a in &i.inheritable_apps {
            s.push_str(&format!("    - {} ({}) \u{2014} via {}\n", a.id, a.name, a.via));
        }
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

fn human_app(r: &AppReport) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "{} ({}) — declares {} types, default for {}:\n",
        r.id, r.name, r.declares, r.default_for
    ));
    for t in &r.types {
        let tag = if t.is_default { "DEFAULT" } else { "       " };
        let cat = t.category.as_deref().unwrap_or("—");
        let note = if !t.declares {
            "  (not declared)".to_string()
        } else if !t.is_default {
            match &t.current_default {
                Some(d) => format!("  (default: {d})"),
                None => String::new(),
            }
        } else {
            String::new()
        };
        s.push_str(&format!("  {tag}  {}  [{cat}]{note}\n", t.mime));
    }
    s.trim_end().to_string()
}

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
    let via: std::collections::HashMap<&str, &str> =
        p.inherited_via.iter().map(|i| (i.mime.as_str(), i.via.as_str())).collect();
    for t in &p.set_types {
        match via.get(t.as_str()) {
            Some(v) => s.push_str(&format!("  + {t}  (via {v})\n")),
            None => s.push_str(&format!("  + {t}\n")),
        }
    }
    if !p.skipped_types.is_empty() {
        s.push_str(&format!(
            "skipped (not declared by {}): {}\n",
            p.app,
            p.skipped_types.join(", ")
        ));
    }
    if !p.unchanged_types.is_empty() {
        s.push_str(&format!("kept (already set): {}\n", p.unchanged_types.join(", ")));
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

/// Write the built-in default category tree to `path` and render the result.
fn init_outcome(path: &std::path::Path, force: bool, json: bool) -> Outcome {
    match crate::categories::write_default_categories(path, force) {
        Ok(written) => {
            if json {
                let body = serde_json::json!({
                    "written": written,
                    "path": path.display().to_string(),
                });
                Outcome { code: 0, stdout: to_json(&body), stderr: String::new() }
            } else if written {
                Outcome {
                    code: 0,
                    stdout: format!("wrote default category tree to {}", path.display()),
                    stderr: String::new(),
                }
            } else {
                Outcome {
                    code: 0,
                    stdout: format!(
                        "{} already exists (use --force to overwrite)",
                        path.display()
                    ),
                    stderr: String::new(),
                }
            }
        }
        Err(e) => render_error(&e, json),
    }
}

/// Binary entry point: parse argv, build the engine from the live environment,
/// print the rendered output, and return the process exit code.
pub fn run() -> i32 {
    let cli = Cli::parse();
    let roots = Roots::from_env();
    let outcome = match &cli.command {
        Command::Init { force } => {
            init_outcome(&roots.data_home.join("madft/categories.toml"), *force, cli.json)
        }
        cmd => match Engine::load(&roots, &current_desktops()) {
            Ok(engine) => execute(&engine, cmd, cli.json, cli.all),
            Err(e) => render_error(&e, cli.json),
        },
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
    fn ls_root_human_lists_categories_without_slashes() {
        let out = execute(&engine(), &Command::Ls { path: None }, false, false);
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("Media"));
        assert!(out.stdout.contains("Web"));
        assert!(out.stdout.contains("Other"));
        assert!(!out.stdout.contains("Media/"));
        assert!(!out.stdout.contains("categories:"));
    }

    #[test]
    fn ls_root_json_has_sorted_subcategories() {
        let out = execute(&engine(), &Command::Ls { path: None }, true, true);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["subcategories"], serde_json::json!(["Media", "Other", "Web"]));
    }

    #[test]
    fn types_human_is_one_per_line() {
        // Filtered view (cli passes show_all=false): application/ogg (inert) is dropped.
        let out = execute(&engine(), &Command::Types { path: "Media".to_string() }, false, false);
        assert_eq!(
            out.stdout,
            "audio/mpeg\nimage/png\nimage/jpeg\nvideo/mp4\nvideo/x-matroska"
        );
    }

    #[test]
    fn info_json_canonicalizes_alias() {
        let out = execute(&engine(), &Command::Info { mimetype: "image/jpg".to_string() }, true, false);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["mime"], "image/jpeg");
        assert_eq!(v["comment"], serde_json::Value::Null);
    }

    #[test]
    fn get_human_prints_bare_default() {
        let out = execute(&engine(), &Command::Get { mimetype: "video/mp4".to_string() }, false, false);
        assert_eq!(out.stdout, "mpv.desktop");
        assert_eq!(out.code, 0);
    }

    #[test]
    fn unknown_path_json_error_envelope() {
        let out = execute(&engine(), &Command::Ls { path: Some("Nope".to_string()) }, true, false);
        assert_eq!(out.code, 1);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["error"]["kind"], "unknown-path");
        assert!(v["error"]["message"].as_str().unwrap().contains("Nope"));
    }

    #[test]
    fn unknown_path_human_error_to_stderr() {
        let out = execute(&engine(), &Command::Ls { path: Some("Nope".to_string()) }, false, false);
        assert_eq!(out.code, 1);
        assert!(out.stdout.is_empty());
        assert!(out.stderr.contains("error:"));
    }

    #[test]
    fn set_dry_run_json_reports_partition() {
        let cmd = Command::Set {
            app: "mpv".to_string(),
            target: Some("Media".to_string()),
            types: vec![],
            force: false,
            no_clobber: false,
            exact: false,
            dry_run: true,
        };
        let out = execute(&engine(), &cmd, true, false);
        assert_eq!(out.code, 0);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["set_types"], serde_json::json!(["audio/mpeg", "video/mp4", "video/x-matroska"]));
        assert_eq!(v["skipped_types"], serde_json::json!(["image/png", "image/jpeg"]));
        assert_eq!(v["unchanged_types"], serde_json::json!([]));
        assert_eq!(v["no_clobber"], serde_json::json!(false));
        assert_eq!(v["written"], serde_json::json!(false));
    }

    #[test]
    fn app_json_reports_rows() {
        let out = execute(&engine(), &Command::App { id: "mpv".to_string(), action: None }, true, false);
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
    fn set_no_clobber_human_shows_kept_line() {
        // video/mp4 is already mpv in the fixture; --no-clobber keeps it.
        let cmd = Command::Set {
            app: "mpv".to_string(),
            target: Some("Media".to_string()),
            types: vec![],
            force: false,
            no_clobber: true,
            exact: false,
            dry_run: true,
        };
        let out = execute(&engine(), &cmd, false, false);
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("kept (already set): video/mp4"));
    }

    #[test]
    fn init_writes_default_then_reports_existing() {
        let dir = std::env::temp_dir().join("madft-cli-init-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("madft/categories.toml");

        let out = init_outcome(&path, false, true);
        assert_eq!(out.code, 0);
        let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(v["written"], serde_json::json!(true));
        assert!(path.exists());

        // Second call without --force: not overwritten.
        let out2 = init_outcome(&path, false, true);
        let v2: serde_json::Value = serde_json::from_str(&out2.stdout).unwrap();
        assert_eq!(v2["written"], serde_json::json!(false));

        // With --force: overwritten.
        let out3 = init_outcome(&path, true, true);
        let v3: serde_json::Value = serde_json::from_str(&out3.stdout).unwrap();
        assert_eq!(v3["written"], serde_json::json!(true));
    }

    #[test]
    fn human_app_marks_undeclared_default_row() {
        use std::path::PathBuf;
        let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let cfg = std::env::temp_dir().join("madft-cli-app-undeclared");
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("mimeapps.list"),
            "[Default Applications]\nimage/png=mpv.desktop\n",
        )
        .unwrap();
        let roots = Roots {
            data_home: f.join("engine"),
            data_dirs: vec![f.clone()],
            config_home: cfg.clone(),
            config_dirs: vec![],
        };
        let e = Engine::load(&roots, &[]).unwrap();
        let out = execute(&e, &Command::App { id: "mpv".to_string(), action: None }, false, false);
        assert_eq!(out.code, 0);
        // mpv is default for image/png but doesn't declare it.
        assert!(out.stdout.contains("DEFAULT  image/png"));
        assert!(out.stdout.contains("(not declared)"));
        assert!(out.stdout.lines().filter(|l| l.contains("DEFAULT")).count() >= 1);
    }

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

    #[test]
    fn ls_empty_after_filter_prints_hint() {
        let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let tmp = std::env::temp_dir().join("madft-cli-empty-filter");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("madft")).unwrap();
        std::fs::write(
            tmp.join("madft/categories.toml"),
            "[\"Ghost\"]\ntypes = [\"application/pdf\", \"application/octet-stream\"]\n",
        )
        .unwrap();
        let roots = Roots {
            data_home: tmp.clone(),
            data_dirs: vec![f.clone()],
            config_home: tmp.clone(),
            config_dirs: vec![],
        };
        let e = Engine::load(&roots, &[]).unwrap();
        let out = execute(&e, &Command::Ls { path: Some("Ghost".to_string()) }, false, false);
        assert!(out.stdout.contains("nothing installed under Ghost"));
        let out_all = execute(&e, &Command::Ls { path: Some("Ghost".to_string()) }, false, true);
        assert!(out_all.stdout.contains("application/pdf"));
    }

    #[test]
    fn parse_type_lines_trims_and_skips_blanks() {
        let input = "  text/x-foo \n\nimage/png\n   \napplication/pdf\n";
        assert_eq!(
            parse_type_lines(input),
            vec!["text/x-foo", "image/png", "application/pdf"]
        );
    }

    #[test]
    fn parse_type_lines_does_not_split_on_commas() {
        // Unlike --types, a comma is part of the (here nonsensical) line, not a delimiter.
        assert_eq!(parse_type_lines("a/b,c/d\n"), vec!["a/b,c/d"]);
    }

    #[test]
    fn parse_type_lines_empty_input_is_empty() {
        assert_eq!(parse_type_lines("   \n\n").len(), 0);
    }
}
