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
