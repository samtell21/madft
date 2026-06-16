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
    pub category: Option<String>,
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
    pub forced: bool,
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
        let cat_path = roots.data_home.join("madft/categories.toml");
        let cat_overrides = FileSource::new(roots.config_home.join("madft/overrides.toml"));
        // Fall back to the built-in default tree if the user has no categories.toml,
        // so `ls` is never empty out of the box (no file is written).
        let tree = if cat_path.exists() {
            categories::build(&FileSource::new(cat_path), &cat_overrides, &mimedb)?
        } else {
            categories::build(
                &categories::StaticSource::new(categories::DEFAULT_CATEGORIES),
                &cat_overrides,
                &mimedb,
            )?
        };
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

    /// Resolve a `[PATH|mimetype]` target. `None` or `"."` is the root (whole
    /// tree); a target containing '/' is a mimetype (umbrella = just that
    /// canonical type); otherwise it is a category path (umbrella = its
    /// recursive types). `'/'` is the mimetype's own delimiter and never
    /// appears in a category name (spec §2). Root's display label is `(root)`.
    fn resolve_umbrella(&self, target: Option<&str>) -> Result<(String, Vec<MimeType>)> {
        match target {
            None | Some(".") => Ok(("(root)".to_string(), self.tree.all_types())),
            Some(t) if t.contains('/') => {
                let canon = self.mimedb.canonicalize(&MimeType::new(t));
                Ok((canon.to_string(), vec![canon]))
            }
            Some(t) => {
                let id = self
                    .tree
                    .node_by_path(t)
                    .ok_or_else(|| Error::UnknownPath(t.to_string()))?;
                Ok((self.tree.path(id), self.tree.types_under(id)))
            }
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
        let mut applicable_apps: Vec<AppRef> = self
            .appindex
            .apps_for_type(&canon)
            .iter()
            .map(|a| AppRef { id: a.id.to_string(), name: a.name.clone() })
            .collect();
        applicable_apps.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(TypeInfo {
            mime: canon.to_string(),
            category: self.tree.category_of(&canon).map(|id| self.tree.path(id)),
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
    pub fn apps(&self, target: Option<&str>) -> Result<AppsResult> {
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
        assert_eq!(info.category.as_deref(), Some("Media.Images"));
        assert_eq!(info.comment, None);
        assert_eq!(info.applicable_count, 1);
        assert_eq!(info.applicable_apps[0].id, "eog.desktop");
    }

    #[test]
    fn info_reports_transitive_ancestors() {
        let e = engine();
        let info = e.info("image/svg+xml").unwrap();
        assert_eq!(info.ancestor_types, vec!["application/xml", "text/plain"]);
        assert_eq!(info.category.as_deref(), Some("Other"));
    }

    #[test]
    fn apps_sorted_by_coverage() {
        let e = engine();
        let r = e.apps(Some("Media")).unwrap();
        let ids: Vec<&str> = r.apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["mpv.desktop", "eog.desktop", "webcam.desktop"]);
        assert_eq!(r.apps[0].coverage, 3);
        assert_eq!(r.apps[1].coverage, 2);
        assert_eq!(r.apps[2].coverage, 1);
    }

    #[test]
    fn apps_for_a_mimetype_target() {
        let e = engine();
        let r = e.apps(Some("video/mp4")).unwrap();
        assert_eq!(r.target, "video/mp4");
        assert_eq!(r.types, vec!["video/mp4"]);
        let ids: Vec<&str> = r.apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["mpv.desktop", "webcam.desktop"]);
    }

    #[test]
    fn apps_root_target_covers_whole_tree() {
        let e = engine();
        let none = e.apps(None).unwrap();
        assert_eq!(none.target, "(root)");
        // `.` is an explicit alias for the same root umbrella.
        let dot = e.apps(Some(".")).unwrap();
        assert_eq!(dot.target, "(root)");
        assert_eq!(none.types, dot.types);
        // Root umbrella is every placed type (Media subtree + Other + Web).
        assert!(none.types.contains(&"video/mp4".to_string()));
        assert!(none.types.contains(&"text/html".to_string()));
        // mpv still leads coverage somewhere in the ranking.
        assert!(none.apps.iter().any(|a| a.id == "mpv.desktop"));
    }

    #[test]
    fn get_returns_current_default() {
        let e = engine();
        assert_eq!(e.get("video/mp4"), Some("mpv.desktop".to_string()));
        assert_eq!(e.get("image/png"), None);
    }

    #[test]
    fn builtin_default_tree_used_when_no_config() {
        let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let empty = std::env::temp_dir().join("madft-no-config-test");
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        let roots = Roots {
            data_home: empty.clone(), // no madft/categories.toml here
            data_dirs: vec![f.clone()], // mime + applications come from fixtures
            config_home: empty,
            config_dirs: vec![],
        };
        let e = Engine::load(&roots, &[]).unwrap();
        // The built-in default tree provides these categories.
        assert!(e.ls(Some("Media.Video")).is_ok());
        assert!(e.ls(Some("Images")).is_ok());
    }
}

impl Engine {
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
        let (label, umbrella) = self.resolve_umbrella(Some(target))?;
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
            // Outside the --types restriction (filter present and type not in it):
            // ignore entirely. Written without a `let`-chain to keep the MSRV at
            // Rust 1.85 (edition 2024's floor); let-chains need 1.88.
            if filter.as_ref().is_some_and(|f| !f.contains(t)) {
                continue;
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
        assert!(matches!(
            e.set("image/png", "nope", None, true, false),
            Err(Error::UnknownApp(_))
        ));
    }
}

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
        assert_eq!(r.declares, 3);
        assert_eq!(r.default_for, 1);
        assert_eq!(r.types[0].mime, "video/mp4");
        assert!(r.types[0].is_default);
        assert_eq!(r.types[0].category.as_deref(), Some("Media.Video"));
        let audio = r.types.iter().find(|t| t.mime == "audio/mpeg").unwrap();
        assert!(!audio.is_default);
        assert_eq!(audio.category.as_deref(), Some("Media.Audio"));
    }

    #[test]
    fn app_unknown_errors() {
        assert!(matches!(engine().app("nope"), Err(Error::UnknownApp(_))));
    }
}
