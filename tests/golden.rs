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
    // The named scenario: `madft set mpv Media --dry-run --json`.
    let cli = parse(&["madft", "set", "mpv", "Media", "--dry-run", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["app"], "mpv.desktop");
    assert_eq!(v["target"], "Media");
    assert_eq!(v["set_types"], serde_json::json!(["audio/mpeg", "video/mp4", "video/x-matroska"]));
    // Filtered umbrella (cli defaults to show_all=false): inert application/ogg is gone.
    assert_eq!(v["skipped_types"], serde_json::json!(["image/png", "image/jpeg"]));
    assert_eq!(v["dry_run"], serde_json::json!(true));
    assert_eq!(v["written"], serde_json::json!(false));
}

#[test]
fn golden_set_writes_file_and_preserves_unrelated() {
    let (engine, path) = writable_engine("set");
    let cli = parse(&["madft", "--json", "set", "mpv", "Media"]);
    let out = execute(&engine, &cli.command, cli.json, cli.all);
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
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["subcategories"], serde_json::json!(["Media", "Other", "Web"]));
    assert_eq!(v["types"], serde_json::json!([]));
}

#[test]
fn golden_apps_coverage_sorted_json() {
    let cli = parse(&["madft", "apps", "Media", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
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
    let cli = parse(&["madft", "set", "nvim", "Media", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 1);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "app-handles-nothing-under-umbrella");
}

#[test]
fn golden_get_is_scriptable() {
    let cli = parse(&["madft", "get", "video/mp4"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.stdout, "mpv.desktop");
    assert_eq!(out.code, 0);
}

#[test]
fn golden_info_includes_category_json() {
    let cli = parse(&["madft", "info", "video/mp4", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["mime"], "video/mp4");
    assert_eq!(v["category"], "Media.Video");
}

#[test]
fn golden_app_json() {
    let cli = parse(&["madft", "app", "mpv", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["id"], "mpv.desktop");
    assert_eq!(v["declares"], 3);
    assert_eq!(v["default_for"], 1);
    assert_eq!(v["types"][0]["mime"], "video/mp4");
    assert_eq!(v["types"][0]["is_default"], true);
    assert_eq!(v["types"][0]["category"], "Media.Video");
    // types[0] is video/mp4, which mpv declares.
    assert_eq!(v["types"][0]["declares"], true);
}

#[test]
fn golden_set_force_overrides_guard() {
    // mpv does not declare image/png: rejected without --force, set with it.
    let reject = parse(&["madft", "set", "mpv", "image/png", "--json"]);
    let out = execute(&read_engine(), &reject.command, reject.json, reject.all);
    assert_eq!(out.code, 1);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "app-handles-nothing-under-umbrella");

    let forced = parse(&["madft", "set", "mpv", "image/png", "--force", "--dry-run", "--json"]);
    let out2 = execute(&read_engine(), &forced.command, forced.json, forced.all);
    assert_eq!(out2.code, 0);
    let v2: serde_json::Value = serde_json::from_str(&out2.stdout).unwrap();
    assert_eq!(v2["forced"], serde_json::json!(true));
    assert_eq!(v2["set_types"], serde_json::json!(["image/png"]));
    assert_eq!(v2["skipped_types"], serde_json::json!([]));
}

#[test]
fn golden_set_root_target_is_system_wide() {
    // `madft set mpv` with no target = root; mpv sets only what it declares.
    let cli = parse(&["madft", "set", "mpv", "--dry-run", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["target"], "(root)");
    assert_eq!(v["set_types"], serde_json::json!(["audio/mpeg", "video/mp4", "video/x-matroska"]));
}

#[test]
fn golden_set_explicit_dot_target_is_root() {
    // `.` is the explicit-root alias on `set`, equivalent to omitting the target.
    let cli = parse(&["madft", "set", "mpv", ".", "--dry-run", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["target"], "(root)");
    assert_eq!(v["set_types"], serde_json::json!(["audio/mpeg", "video/mp4", "video/x-matroska"]));
}

#[test]
fn golden_apps_no_target_is_root() {
    let cli = parse(&["madft", "apps", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["target"], "(root)");
    // `.` is the explicit-root alias and must match.
    let dot = parse(&["madft", "apps", ".", "--json"]);
    let out2 = execute(&read_engine(), &dot.command, dot.json, dot.all);
    let v2: serde_json::Value = serde_json::from_str(&out2.stdout).unwrap();
    assert_eq!(v2["target"], "(root)");
    assert_eq!(v["types"], v2["types"]);
}

#[test]
fn golden_set_no_clobber_fills_only_blanks() {
    // video/mp4 is already mpv; --no-clobber leaves it, fills the rest.
    let cli = parse(&["madft", "set", "mpv", "Media", "--no-clobber", "--dry-run", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["set_types"], serde_json::json!(["audio/mpeg", "video/x-matroska"]));
    assert_eq!(v["unchanged_types"], serde_json::json!(["video/mp4"]));
    assert_eq!(v["no_clobber"], serde_json::json!(true));

    // The --if-unset alias parses to the same behavior.
    let aliased = parse(&["madft", "set", "mpv", "Media", "--if-unset", "--dry-run", "--json"]);
    let out2 = execute(&read_engine(), &aliased.command, aliased.json, aliased.all);
    let v2: serde_json::Value = serde_json::from_str(&out2.stdout).unwrap();
    assert_eq!(v2["set_types"], v["set_types"]);
    assert_eq!(v2["no_clobber"], serde_json::json!(true));
}

#[test]
fn golden_ls_hides_inert_by_default_shows_with_all() {
    // application/ogg under Media is inert (no app). Hidden by default, shown with --all.
    let def = parse(&["madft", "ls", "Media", "--json"]);
    let out = execute(&read_engine(), &def.command, def.json, def.all);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let mimes: Vec<&str> = v["types"].as_array().unwrap().iter().map(|t| t["mime"].as_str().unwrap()).collect();
    assert!(!mimes.contains(&"application/ogg"));

    let all = parse(&["madft", "ls", "Media", "--all", "--json"]);
    let out2 = execute(&read_engine(), &all.command, all.json, all.all);
    let v2: serde_json::Value = serde_json::from_str(&out2.stdout).unwrap();
    let mimes2: Vec<&str> = v2["types"].as_array().unwrap().iter().map(|t| t["mime"].as_str().unwrap()).collect();
    assert!(mimes2.contains(&"application/ogg"));
}

#[test]
fn golden_set_explicit_inert_type_bypasses_filter() {
    // application/pdf is inert; naming it explicitly + --force sets it even without --all.
    let cli = parse(&["madft", "set", "mpv", "application/pdf", "--force", "--dry-run", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["set_types"], serde_json::json!(["application/pdf"]));
}

#[test]
fn golden_short_a_flag_parses_as_all() {
    // -a is the short form of --all.
    let cli = parse(&["madft", "ls", "Media", "-a", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let mimes: Vec<&str> = v["types"].as_array().unwrap().iter().map(|t| t["mime"].as_str().unwrap()).collect();
    assert!(mimes.contains(&"application/ogg"));
}

#[test]
fn golden_app_includes_undeclared_default() {
    let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cfg = std::env::temp_dir().join("madft-golden-app-undeclared");
    let _ = std::fs::remove_dir_all(&cfg);
    std::fs::create_dir_all(&cfg).unwrap();
    // mpv is default for image/png but does not declare it.
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
    let cli = parse(&["madft", "app", "mpv", "--json"]);
    let out = execute(&e, &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 0);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    let png = v["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["mime"] == "image/png")
        .unwrap();
    assert_eq!(png["declares"], false);
    assert_eq!(png["is_default"], true);
    assert_eq!(png["current_default"], "mpv.desktop");
    assert_eq!(v["default_for"], 1);
}

#[test]
fn golden_info_shows_inherited_default_json() {
    let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cfg = std::env::temp_dir().join("madft-golden-info-inherit");
    let _ = std::fs::remove_dir_all(&cfg);
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("mimeapps.list"), "[Default Applications]\ntext/plain=nvim.desktop\n").unwrap();
    let roots = Roots {
        data_home: f.join("engine"),
        data_dirs: vec![f.clone()],
        config_home: cfg.clone(),
        config_dirs: vec![],
    };
    let e = Engine::load(&roots, &[]).unwrap();
    let cli = parse(&["madft", "info", "application/xml", "--json"]);
    let out = execute(&e, &cli.command, cli.json, cli.all);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["default"]["app"], "nvim.desktop");
    assert_eq!(v["default"]["via"], "text/plain");
    assert_eq!(v["applicable_count"], 0);
    assert!(v["inheritable_apps"].as_array().unwrap().iter().any(|a| a["id"] == "nvim.desktop"));
}

#[test]
fn golden_set_via_inheritance_human() {
    let cli = parse(&["madft", "set", "nvim", "image/svg+xml", "--dry-run"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 0);
    // nvim opens svg via text/plain (application/xml -> text/plain).
    assert!(out.stdout.contains("image/svg+xml  (via text/plain)"));
}

#[test]
fn golden_set_exact_rejects_inherited() {
    let cli = parse(&["madft", "set", "nvim", "image/svg+xml", "--exact", "--json"]);
    let out = execute(&read_engine(), &cli.command, cli.json, cli.all);
    assert_eq!(out.code, 1);
    let v: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "app-handles-nothing-under-umbrella");
}

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
