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
    if let Some(cat) = &i.category {
        s.push_str(&format!("  category: {cat}\n"));
    }
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
