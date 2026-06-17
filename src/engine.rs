//! Orchestrates the facts (`MimeDb`, `AppIndex`, `Defaults`) and the category
//! `CategoryTree` into the user-facing operations (spec §5). Every operation
//! returns a `Serialize` result struct; rendering (human vs `--json`) is Plan
//! 4's CLI. All inputs derive from an injectable `Roots`, so tests run against
//! fixture trees with zero host reliance.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::appindex::AppIndex;
use crate::categories::{self, CategoryId, CategoryTree, FileSource};
use crate::defaults::Defaults;
use crate::error::{Error, Result};
use crate::mimedb::MimeDb;
use crate::paths::Roots;
use crate::types::{DesktopId, MimeType};

/// A leaf type as shown by `ls`: the type, its effective default, how many apps
/// declare it exactly, and how many more could open it via inheritance.
#[derive(Serialize, Debug)]
pub struct LeafType {
    pub mime: String,
    pub default: Option<DefaultRef>,
    pub applicable_count: usize,
    pub inheritable_count: usize,
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

/// The effective default for a type: which app opens it, and via which ancestor
/// (`via == None` means the type's own exact default). The OUTER `Option` being
/// `None` means no default at all — exact or inherited.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct DefaultRef {
    pub app: String,
    pub via: Option<String>,
}

/// An app that can open a type only via inheritance (it declares an ancestor, not
/// the type itself), tagged with the nearest such ancestor.
#[derive(Serialize, Debug)]
pub struct InheritableApp {
    pub id: String,
    pub name: String,
    pub via: String,
}

/// Result of `info` for one mimetype. `comment` is deferred (always `None`).
/// `default` is the effective default (exact or inherited). `inheritable_apps`
/// are apps that can open it via an ancestor. The mimetype is alias-canonicalized.
#[derive(Serialize, Debug)]
pub struct TypeInfo {
    pub mime: String,
    pub category: Option<String>,
    pub comment: Option<String>,
    pub default: Option<DefaultRef>,
    pub applicable_count: usize,
    pub inheritable_count: usize,
    pub ancestor_types: Vec<String>,
    pub applicable_apps: Vec<AppRef>,
    pub inheritable_apps: Vec<InheritableApp>,
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

/// Plan produced by `set`: which umbrella types will be / were set, which were
/// skipped because the app does not declare them, and which were kept untouched
/// by `--no-clobber` (they already had a default). Skips are informational, NOT
/// an error — partial coverage is success (spec §7).
#[derive(Serialize, Debug)]
pub struct SetPlan {
    pub app: String,
    pub target: String,
    pub set_types: Vec<String>,
    pub skipped_types: Vec<String>,
    pub unchanged_types: Vec<String>,
    pub inherited_via: Vec<InheritedSet>,
    pub forced: bool,
    pub no_clobber: bool,
    pub dry_run: bool,
    pub written: bool,
}

/// One inheritance-matched entry in a `SetPlan`: a `set_types` member matched
/// because the app declares an ancestor (not the type), plus that ancestor.
#[derive(Serialize, Debug)]
pub struct InheritedSet {
    pub mime: String,
    pub via: String,
}

/// Flags for `set`, bundled to keep the signature readable. `show_all` disables
/// the presence filter on a category/root umbrella; `force` overrides the
/// exact-declaration guard; `no_clobber` fills only types with no current
/// default; `exact` restricts to exact declarations only (no inheritance);
/// `dry_run` previews without writing.
#[derive(Clone, Copy, Debug, Default)]
pub struct SetOptions {
    pub force: bool,
    pub no_clobber: bool,
    pub exact: bool,
    pub show_all: bool,
    pub dry_run: bool,
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
            default: self.effective_default(t),
            applicable_count: self.appindex.apps_for_type(t).len(),
            inheritable_count: self.inheritable_apps(t).len(),
        }
    }

    /// True if any installed app can OPEN `t` — declares it exactly, or declares
    /// one of its ancestors in the subclass DAG (so it opens via inheritance).
    fn openable(&self, t: &MimeType) -> bool {
        !self.appindex.apps_for_type(t).is_empty()
            || self
                .mimedb
                .ancestor_types(t)
                .iter()
                .any(|a| !self.appindex.apps_for_type(a).is_empty())
    }

    /// A type is inert when nothing can open it (no exact handler and no
    /// inherited handler). Inert types are hidden by the default presence filter.
    fn type_is_inert(&self, t: &MimeType) -> bool {
        !self.openable(t)
    }

    /// The effective default for `t`: the exact `current_default` if set (via =
    /// None), else the default of the nearest ancestor that has one (via = that
    /// ancestor), else None.
    fn effective_default(&self, t: &MimeType) -> Option<DefaultRef> {
        if let Some(d) = self.defaults.current_default(t) {
            return Some(DefaultRef { app: d.to_string(), via: None });
        }
        for anc in self.mimedb.ancestor_types(t) {
            if let Some(d) = self.defaults.current_default(&anc) {
                return Some(DefaultRef { app: d.to_string(), via: Some(anc.to_string()) });
            }
        }
        None
    }

    /// Apps that can open `t` only via inheritance: they declare an ancestor of
    /// `t` but not `t` itself. Each is tagged with the NEAREST ancestor it
    /// declares (ancestors are walked nearest-first). Distinct apps, sorted by id.
    fn inheritable_apps(&self, t: &MimeType) -> Vec<InheritableApp> {
        let mut seen: std::collections::BTreeSet<DesktopId> =
            self.appindex.apps_for_type(t).iter().map(|a| a.id.clone()).collect();
        let mut out: Vec<InheritableApp> = Vec::new();
        for anc in self.mimedb.ancestor_types(t) {
            for a in self.appindex.apps_for_type(&anc) {
                if seen.insert(a.id.clone()) {
                    out.push(InheritableApp {
                        id: a.id.to_string(),
                        name: a.name.clone(),
                        via: anc.to_string(),
                    });
                }
            }
        }
        out.sort_by(|x, y| x.id.cmp(&y.id));
        out
    }

    /// True if any type anywhere under `id` is app-backed (not inert). Used to
    /// decide whether a category is worth showing in the default (filtered) view.
    fn subtree_has_app_backed_type(&self, id: CategoryId) -> bool {
        self.tree.types_under(id).iter().any(|t| !self.type_is_inert(t))
    }

    /// Apply the presence filter to a resolved umbrella. Inert types are dropped
    /// UNLESS `show_all`, the target is an explicit mimetype (contains '/'), or
    /// `explicit_types` is set (a `--types` list named exact types). Explicit
    /// selections always win.
    fn filter_umbrella(
        &self,
        target: Option<&str>,
        umbrella: Vec<MimeType>,
        show_all: bool,
        explicit_types: bool,
    ) -> Vec<MimeType> {
        let explicit_target = target.is_some_and(|t| t.contains('/'));
        if show_all || explicit_target || explicit_types {
            umbrella
        } else {
            umbrella.into_iter().filter(|t| !self.type_is_inert(t)).collect()
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

    /// `ls [PATH]`: child categories + direct leaf types at a node (roots if no
    /// PATH). With `show_all == false`, inert direct types and fully-inert
    /// subcategories are hidden (the default presence filter).
    pub fn ls(&self, path: Option<&str>, show_all: bool) -> Result<LsResult> {
        let (label, child_ids, direct) = match path {
            None => (String::new(), self.tree.roots(), Vec::new()),
            Some(p) => {
                let id = self
                    .tree
                    .node_by_path(p)
                    .ok_or_else(|| Error::UnknownPath(p.to_string()))?;
                (self.tree.path(id), self.tree.subcategories(id), self.tree.types(id).to_vec())
            }
        };
        let subcategories = child_ids
            .into_iter()
            .filter(|&c| show_all || self.subtree_has_app_backed_type(c))
            .map(|c| self.tree.path(c))
            .collect();
        let types = direct
            .iter()
            .filter(|t| show_all || !self.type_is_inert(t))
            .map(|t| self.leaf_type(t))
            .collect();
        Ok(LsResult { path: label, subcategories, types })
    }

    /// `types <PATH>`: all mimetypes under the umbrella (recursive,
    /// canonicalized). Inert types are dropped unless `show_all`.
    pub fn types(&self, path: &str, show_all: bool) -> Result<Vec<String>> {
        let id = self
            .tree
            .node_by_path(path)
            .ok_or_else(|| Error::UnknownPath(path.to_string()))?;
        Ok(self
            .tree
            .types_under(id)
            .into_iter()
            .filter(|t| show_all || !self.type_is_inert(t))
            .map(|t| t.to_string())
            .collect())
    }

    /// `info <mimetype>`: canonical name, effective default (exact or inherited),
    /// applicable apps, inheritable apps, and the `ancestor_types` chain. `comment`
    /// is deferred.
    pub fn info(&self, mime: &str) -> Result<TypeInfo> {
        let canon = self.mimedb.canonicalize(&MimeType::new(mime));
        let mut applicable_apps: Vec<AppRef> = self
            .appindex
            .apps_for_type(&canon)
            .iter()
            .map(|a| AppRef { id: a.id.to_string(), name: a.name.clone() })
            .collect();
        applicable_apps.sort_by(|a, b| a.id.cmp(&b.id));
        let inheritable_apps = self.inheritable_apps(&canon);
        Ok(TypeInfo {
            mime: canon.to_string(),
            category: self.tree.category_of(&canon).map(|id| self.tree.path(id)),
            comment: None,
            default: self.effective_default(&canon),
            applicable_count: applicable_apps.len(),
            inheritable_count: inheritable_apps.len(),
            ancestor_types: self
                .mimedb
                .ancestor_types(&canon)
                .iter()
                .map(|t| t.to_string())
                .collect(),
            applicable_apps,
            inheritable_apps,
        })
    }

    /// `apps <PATH|mimetype>`: apps declaring any of the umbrella's types, with
    /// their coverage, sorted by coverage (desc) then id (asc).
    pub fn apps(&self, target: Option<&str>, show_all: bool) -> Result<AppsResult> {
        let (label, raw) = self.resolve_umbrella(target)?;
        let umbrella = self.filter_umbrella(target, raw, show_all, false);
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

    /// "App handles type T" in the inheritance-aware sense: it declares T, or
    /// declares some ancestor of T (so it opens T via the subclass chain).
    fn handles(&self, app_id: &DesktopId, t: &MimeType) -> bool {
        self.appindex.declares(app_id, t)
            || self.mimedb.ancestor_types(t).iter().any(|a| self.appindex.declares(app_id, a))
    }

    /// The nearest ancestor of `t` that `app_id` declares, if any (for "via X").
    fn nearest_declared_ancestor(&self, app_id: &DesktopId, t: &MimeType) -> Option<MimeType> {
        self.mimedb.ancestor_types(t).into_iter().find(|a| self.appindex.declares(app_id, a))
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
        let r = e.ls(None, true).unwrap();
        assert_eq!(r.subcategories, vec!["Media", "Other", "Web"]);
        assert!(r.types.is_empty());
    }

    #[test]
    fn ls_node_lists_subcategories_and_direct_types() {
        let e = engine();
        let r = e.ls(Some("Media"), true).unwrap();
        assert_eq!(r.path, "Media");
        assert_eq!(r.subcategories, vec!["Media.Audio", "Media.Images", "Media.Video"]);
        assert_eq!(r.types.len(), 1);
        assert_eq!(r.types[0].mime, "application/ogg");
        assert!(r.types[0].default.is_none());
        assert_eq!(r.types[0].applicable_count, 0);
        assert_eq!(r.types[0].inheritable_count, 0);
    }

    #[test]
    fn ls_unknown_path_errors() {
        let e = engine();
        assert!(matches!(e.ls(Some("Nope"), false), Err(Error::UnknownPath(_))));
    }

    #[test]
    fn types_under_umbrella_is_recursive() {
        let e = engine();
        let t = e.types("Media", true).unwrap();
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
    fn ls_hides_inert_direct_type_by_default() {
        let e = engine();
        // Media's direct type application/ogg has no installed app -> inert.
        let hidden = e.ls(Some("Media"), false).unwrap();
        assert!(hidden.types.iter().all(|t| t.mime != "application/ogg"));
        let shown = e.ls(Some("Media"), true).unwrap();
        assert!(shown.types.iter().any(|t| t.mime == "application/ogg"));
    }

    #[test]
    fn ls_other_presence_is_inheritance_aware() {
        let e = engine();
        let r = e.ls(Some("Other"), false).unwrap();
        let mimes: Vec<&str> = r.types.iter().map(|t| t.mime.as_str()).collect();
        assert!(mimes.contains(&"text/plain")); // exact (nvim)
        // application/xml has no exact declarer but inherits text/plain (nvim) -> now shown.
        assert!(mimes.contains(&"application/xml"));
        // application/octet-stream: no declarer, no ancestors -> still hidden.
        assert!(!mimes.contains(&"application/octet-stream"));
    }

    #[test]
    fn openable_via_inheritance() {
        let e = engine();
        // application/xml: no exact declarer, but inherits text/plain (nvim) -> openable.
        assert!(!e.type_is_inert(&MimeType::new("application/xml")));
        // application/octet-stream: no declarer, no ancestors -> inert.
        assert!(e.type_is_inert(&MimeType::new("application/octet-stream")));
        // image/png: exact declarer (eog) -> openable.
        assert!(!e.type_is_inert(&MimeType::new("image/png")));
    }

    #[test]
    fn types_drops_inert_unless_show_all() {
        let e = engine();
        let filtered = e.types("Media", false).unwrap();
        assert!(!filtered.contains(&"application/ogg".to_string()));
        assert!(filtered.contains(&"video/mp4".to_string()));
        let all = e.types("Media", true).unwrap();
        assert!(all.contains(&"application/ogg".to_string()));
    }

    #[test]
    fn ls_root_hides_fully_inert_top_level_category() {
        // A top-level category whose entire subtree is inert is hidden from the
        // root `ls` by default, and shown with show_all. (The fixture's own roots
        // are all app-backed, so this builds a temp tree to exercise the case.)
        let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let tmp = std::env::temp_dir().join("madft-engine-inert-root");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("madft")).unwrap();
        // application/pdf + application/octet-stream are inert; audio/mpeg is app-backed.
        std::fs::write(
            tmp.join("madft/categories.toml"),
            "[\"Ghost\"]\ntypes = [\"application/pdf\", \"application/octet-stream\"]\n\n\
             [\"Live\"]\ntypes = [\"audio/mpeg\"]\n",
        )
        .unwrap();
        let roots = Roots {
            data_home: tmp.clone(),
            data_dirs: vec![f.clone()],
            config_home: tmp.clone(),
            config_dirs: vec![],
        };
        let e = Engine::load(&roots, &[]).unwrap();
        let filtered = e.ls(None, false).unwrap();
        assert!(filtered.subcategories.contains(&"Live".to_string()));
        assert!(!filtered.subcategories.contains(&"Ghost".to_string()));
        let all = e.ls(None, true).unwrap();
        assert!(all.subcategories.contains(&"Ghost".to_string()));
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
    fn info_effective_default_is_inherited() {
        let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let cfg = std::env::temp_dir().join("madft-info-effective");
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

        let xml = e.info("application/xml").unwrap();
        let d = xml.default.expect("effective default");
        assert_eq!(d.app, "nvim.desktop");
        assert_eq!(d.via.as_deref(), Some("text/plain"));
        assert!(xml.inheritable_apps.iter().any(|a| a.id == "nvim.desktop" && a.via == "text/plain"));
        assert_eq!(xml.applicable_count, 0);

        let plain = e.info("text/plain").unwrap();
        let dp = plain.default.expect("exact default");
        assert_eq!(dp.app, "nvim.desktop");
        assert_eq!(dp.via, None);
    }

    #[test]
    fn apps_sorted_by_coverage() {
        let e = engine();
        let r = e.apps(Some("Media"), true).unwrap();
        let ids: Vec<&str> = r.apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["mpv.desktop", "eog.desktop", "webcam.desktop"]);
        assert_eq!(r.apps[0].coverage, 3);
        assert_eq!(r.apps[1].coverage, 2);
        assert_eq!(r.apps[2].coverage, 1);
    }

    #[test]
    fn apps_for_a_mimetype_target() {
        let e = engine();
        let r = e.apps(Some("video/mp4"), false).unwrap();
        assert_eq!(r.target, "video/mp4");
        assert_eq!(r.types, vec!["video/mp4"]);
        let ids: Vec<&str> = r.apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["mpv.desktop", "webcam.desktop"]);
    }

    #[test]
    fn apps_root_target_covers_whole_tree() {
        let e = engine();
        let none = e.apps(None, true).unwrap();
        assert_eq!(none.target, "(root)");
        // `.` is an explicit alias for the same root umbrella.
        let dot = e.apps(Some("."), true).unwrap();
        assert_eq!(dot.target, "(root)");
        assert_eq!(none.types, dot.types);
        // Root umbrella is every placed type (Media subtree + Other + Web).
        assert!(none.types.contains(&"video/mp4".to_string()));
        assert!(none.types.contains(&"text/html".to_string()));
        // mpv still leads coverage somewhere in the ranking.
        assert!(none.apps.iter().any(|a| a.id == "mpv.desktop"));
    }

    #[test]
    fn apps_category_target_filters_inert_types() {
        let e = engine();
        let r = e.apps(Some("Media"), false).unwrap();
        assert!(!r.types.contains(&"application/ogg".to_string()));
        let all = e.apps(Some("Media"), true).unwrap();
        assert!(all.types.contains(&"application/ogg".to_string()));
    }

    #[test]
    fn apps_explicit_mimetype_target_is_never_filtered() {
        let e = engine();
        let r = e.apps(Some("application/pdf"), false).unwrap();
        assert_eq!(r.target, "application/pdf");
        assert_eq!(r.types, vec!["application/pdf"]);
    }

    #[test]
    fn get_returns_current_default() {
        let e = engine();
        assert_eq!(e.get("video/mp4"), Some("mpv.desktop".to_string()));
        assert_eq!(e.get("image/png"), None);
    }

    #[test]
    fn ls_leaf_carries_effective_default_and_inheritable_count() {
        let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let cfg = std::env::temp_dir().join("madft-ls-effective");
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(cfg.join("madft")).unwrap();
        std::fs::write(cfg.join("madft/categories.toml"), "[\"Docs\"]\ntypes = [\"application/xml\"]\n").unwrap();
        std::fs::write(cfg.join("mimeapps.list"), "[Default Applications]\ntext/plain=nvim.desktop\n").unwrap();
        let roots = Roots {
            data_home: cfg.clone(),
            data_dirs: vec![f.clone()],
            config_home: cfg.clone(),
            config_dirs: vec![],
        };
        let e = Engine::load(&roots, &[]).unwrap();
        let r = e.ls(Some("Docs"), false).unwrap();
        let xml = r.types.iter().find(|t| t.mime == "application/xml").expect("xml shown (openable via inherit)");
        let d = xml.default.as_ref().expect("inherited default");
        assert_eq!(d.app, "nvim.desktop");
        assert_eq!(d.via.as_deref(), Some("text/plain"));
        assert_eq!(xml.applicable_count, 0);
        assert_eq!(xml.inheritable_count, 1);
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
        assert!(e.ls(Some("Media.Video"), false).is_ok());
        assert!(e.ls(Some("Images"), false).is_ok());
    }
}

impl Engine {
    /// `set <app> [target] [--types …] [--force] [--no-clobber] [--dry-run]`:
    /// set `app` as the default for the umbrella types it declares. Target is
    /// optional (root/whole-tree when omitted or `"."`). Types the app does NOT
    /// declare are reported as `skipped_types` (informational, not an error).
    /// `types_filter`, when given, restricts to that subset (alias-canonicalized).
    /// `force` overrides the exact-declaration guard. `no_clobber` keeps any
    /// candidate that already has a current default (reported as
    /// `unchanged_types`), so only blanks are filled.
    ///
    /// The empty-candidate guard runs BEFORE the no-clobber split: an app that
    /// declares nothing under the umbrella errors `AppHandlesNothingUnderUmbrella`
    /// whether or not `no_clobber` is set. By contrast, a no-clobber call whose
    /// candidates are all already set is SUCCESS (writes nothing).
    pub fn set(
        &self,
        app: &str,
        target: Option<&str>,
        types_filter: Option<&[String]>,
        opts: SetOptions,
    ) -> Result<SetPlan> {
        let (label, raw) = self.resolve_umbrella(target)?;
        let umbrella = self.filter_umbrella(target, raw, opts.show_all, types_filter.is_some());
        let app_id = DesktopId::new(app);
        if self.appindex.app(&app_id).is_none() {
            return Err(Error::UnknownApp(app_id.to_string()));
        }
        let filter: Option<Vec<MimeType>> = types_filter.map(|fs| {
            fs.iter()
                .map(|s| self.mimedb.canonicalize(&MimeType::new(s.as_str())))
                .collect()
        });

        let mut candidates: Vec<MimeType> = Vec::new();
        let mut skipped: Vec<MimeType> = Vec::new();
        for t in &umbrella {
            if filter.as_ref().is_some_and(|f| !f.contains(t)) {
                continue;
            }
            // Inheritance-aware handling, unless --exact restricts to exact declares.
            let handled = if opts.exact {
                self.appindex.declares(&app_id, t)
            } else {
                self.handles(&app_id, t)
            };
            if opts.force || handled {
                candidates.push(t.clone());
            } else {
                skipped.push(t.clone());
            }
        }

        if candidates.is_empty() {
            return Err(Error::AppHandlesNothingUnderUmbrella {
                app: app_id.to_string(),
                umbrella: label,
            });
        }

        let (set_types, unchanged): (Vec<MimeType>, Vec<MimeType>) = if opts.no_clobber {
            candidates
                .into_iter()
                .partition(|t| self.defaults.current_default(t).is_none())
        } else {
            (candidates, Vec::new())
        };

        // Provenance: set_types matched via an ancestor (not exactly declared).
        let inherited_via: Vec<InheritedSet> = set_types
            .iter()
            .filter(|t| !self.appindex.declares(&app_id, t))
            .filter_map(|t| {
                self.nearest_declared_ancestor(&app_id, t)
                    .map(|via| InheritedSet { mime: t.to_string(), via: via.to_string() })
            })
            .collect();

        let edits: Vec<crate::writer::Edit> = set_types
            .iter()
            .map(|t| crate::writer::Edit::Set(t.clone(), app_id.clone()))
            .collect();
        let written = if opts.dry_run || edits.is_empty() {
            false
        } else {
            crate::writer::write_user_defaults(&self.roots.user_mimeapps(), &edits)?
        };

        Ok(SetPlan {
            app: app_id.to_string(),
            target: label,
            set_types: set_types.iter().map(|t| t.to_string()).collect(),
            skipped_types: skipped.iter().map(|t| t.to_string()).collect(),
            unchanged_types: unchanged.iter().map(|t| t.to_string()).collect(),
            inherited_via,
            forced: opts.force,
            no_clobber: opts.no_clobber,
            dry_run: opts.dry_run,
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
        let plan = e.set("mpv", Some("Media"), None, SetOptions { show_all: true, dry_run: true, ..Default::default() }).unwrap();
        assert_eq!(plan.set_types, vec!["audio/mpeg", "video/mp4", "video/x-matroska"]);
        assert_eq!(plan.skipped_types, vec!["application/ogg", "image/png", "image/jpeg"]);
        assert!(plan.unchanged_types.is_empty());
        assert!(!plan.written);
        assert!(plan.dry_run);
        assert!(!plan.forced);
        assert!(!plan.no_clobber);
    }

    #[test]
    fn set_guards_when_app_handles_nothing() {
        let e = read_only_engine();
        let err = e.set("nvim", Some("Media"), None, SetOptions { show_all: true, ..Default::default() }).unwrap_err();
        assert!(matches!(err, Error::AppHandlesNothingUnderUmbrella { .. }));
    }

    #[test]
    fn set_unknown_app_errors() {
        let e = read_only_engine();
        assert!(matches!(e.set("nope", Some("Media"), None, SetOptions { show_all: true, ..Default::default() }), Err(Error::UnknownApp(_))));
    }

    #[test]
    fn set_writes_backs_up_and_is_idempotent() {
        let (e, path) = engine_with_temp_config("set");
        let plan = e.set("mpv", Some("Media"), None, SetOptions::default()).unwrap();
        assert!(plan.written);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("audio/mpeg=mpv.desktop"));
        assert!(content.contains("video/x-matroska=mpv.desktop"));
        assert!(content.contains("video/mp4=mpv.desktop"));
        assert!(content.contains("text/html=org.qutebrowser.qutebrowser.desktop"));
        assert!(path.with_file_name("mimeapps.list.bak").exists());
        let again = e.set("mpv", Some("Media"), None, SetOptions::default()).unwrap();
        assert!(!again.written);
    }

    #[test]
    fn set_with_types_filter_restricts() {
        let (e, _path) = engine_with_temp_config("filter");
        let only = ["video/mp4".to_string()];
        let plan = e.set("mpv", Some("Media"), Some(&only), SetOptions { dry_run: true, ..Default::default() }).unwrap();
        assert_eq!(plan.set_types, vec!["video/mp4"]);
        assert!(plan.skipped_types.is_empty());
    }

    #[test]
    fn set_force_sets_undeclared_type() {
        let e = read_only_engine();
        let plan = e.set("mpv", Some("image/png"), None, SetOptions { force: true, dry_run: true, ..Default::default() }).unwrap();
        assert_eq!(plan.set_types, vec!["image/png"]);
        assert!(plan.skipped_types.is_empty());
        assert!(plan.forced);
    }

    #[test]
    fn set_force_still_errors_on_unknown_app() {
        let e = read_only_engine();
        assert!(matches!(
            e.set("nope", Some("image/png"), None, SetOptions { force: true, ..Default::default() }),
            Err(Error::UnknownApp(_))
        ));
    }

    #[test]
    fn set_no_clobber_only_fills_unset_declared_types() {
        let e = read_only_engine();
        let plan = e.set("mpv", Some("Media"), None, SetOptions { no_clobber: true, dry_run: true, ..Default::default() }).unwrap();
        assert_eq!(plan.set_types, vec!["audio/mpeg", "video/x-matroska"]);
        assert_eq!(plan.unchanged_types, vec!["video/mp4"]);
        assert!(plan.no_clobber);
    }

    #[test]
    fn set_no_clobber_all_already_set_is_success_not_error() {
        let e = read_only_engine();
        let only = ["video/mp4".to_string()];
        let plan = e.set("mpv", Some("Media"), Some(&only), SetOptions { no_clobber: true, dry_run: true, ..Default::default() }).unwrap();
        assert!(plan.set_types.is_empty());
        assert_eq!(plan.unchanged_types, vec!["video/mp4"]);
        assert!(!plan.written);
    }

    #[test]
    fn set_no_clobber_still_guards_when_app_declares_nothing() {
        let e = read_only_engine();
        let err = e.set("nvim", Some("Media"), None, SetOptions { no_clobber: true, ..Default::default() }).unwrap_err();
        assert!(matches!(err, Error::AppHandlesNothingUnderUmbrella { .. }));
    }

    #[test]
    fn set_root_target_labels_root() {
        let e = read_only_engine();
        let plan = e.set("mpv", None, None, SetOptions { show_all: true, dry_run: true, ..Default::default() }).unwrap();
        assert_eq!(plan.target, "(root)");
        assert_eq!(plan.set_types, vec!["audio/mpeg", "video/mp4", "video/x-matroska"]);
    }

    #[test]
    fn set_category_umbrella_excludes_inert_by_default() {
        let e = read_only_engine();
        let plan = e.set("mpv", Some("Media"), None, SetOptions { dry_run: true, ..Default::default() }).unwrap();
        assert_eq!(plan.set_types, vec!["audio/mpeg", "video/mp4", "video/x-matroska"]);
        assert_eq!(plan.skipped_types, vec!["image/png", "image/jpeg"]);
        assert!(!plan.skipped_types.contains(&"application/ogg".to_string()));
    }

    #[test]
    fn set_show_all_restores_inert_into_umbrella() {
        let e = read_only_engine();
        let plan = e.set("mpv", Some("Media"), None, SetOptions { show_all: true, dry_run: true, ..Default::default() }).unwrap();
        assert!(plan.skipped_types.contains(&"application/ogg".to_string()));
    }

    #[test]
    fn set_explicit_mimetype_target_bypasses_filter() {
        let e = read_only_engine();
        let plan = e.set("mpv", Some("application/pdf"), None, SetOptions { force: true, dry_run: true, ..Default::default() }).unwrap();
        assert_eq!(plan.set_types, vec!["application/pdf"]);
    }

    #[test]
    fn set_types_list_bypasses_filter() {
        let e = read_only_engine();
        let only = ["application/ogg".to_string()];
        let plan = e.set("mpv", Some("Media"), Some(&only), SetOptions { force: true, dry_run: true, ..Default::default() }).unwrap();
        assert_eq!(plan.set_types, vec!["application/ogg"]);
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
    fn set_relaxed_guard_matches_via_inheritance() {
        let e = read_only_engine();
        let plan = e.set("nvim", Some("image/svg+xml"), None, SetOptions { dry_run: true, ..Default::default() }).unwrap();
        assert_eq!(plan.set_types, vec!["image/svg+xml"]);
        assert_eq!(plan.inherited_via.len(), 1);
        assert_eq!(plan.inherited_via[0].mime, "image/svg+xml");
        assert_eq!(plan.inherited_via[0].via, "text/plain");
    }

    #[test]
    fn set_exact_rejects_inherited_only_target() {
        let e = read_only_engine();
        let err = e.set("nvim", Some("image/svg+xml"), None, SetOptions { exact: true, ..Default::default() }).unwrap_err();
        assert!(matches!(err, Error::AppHandlesNothingUnderUmbrella { .. }));
    }

    #[test]
    fn set_exact_force_writes_inherited_only_target() {
        let e = read_only_engine();
        let plan = e.set("nvim", Some("image/svg+xml"), None, SetOptions { exact: true, force: true, dry_run: true, ..Default::default() }).unwrap();
        assert_eq!(plan.set_types, vec!["image/svg+xml"]);
        assert_eq!(plan.inherited_via.len(), 1);
    }

    #[test]
    fn set_exact_declared_target_still_works() {
        let e = read_only_engine();
        let plan = e.set("nvim", Some("text/plain"), None, SetOptions { exact: true, dry_run: true, ..Default::default() }).unwrap();
        assert_eq!(plan.set_types, vec!["text/plain"]);
        assert!(plan.inherited_via.is_empty());
    }
}

/// One mimetype row in an `AppReport`: a type the app declares and/or is the
/// current default for. `declares` is false for default-for-but-undeclared rows.
#[derive(serde::Serialize, Debug)]
pub struct AppTypeRow {
    pub mime: String,
    pub category: Option<String>,
    pub declares: bool,
    pub is_default: bool,
    pub current_default: Option<String>,
}

/// Result of `app`: the union of an app's declared types and the types it is
/// the current default for, where each lives, and which it is the default for.
#[derive(serde::Serialize, Debug)]
pub struct AppReport {
    pub id: String,
    pub name: String,
    pub declares: usize,
    pub default_for: usize,
    pub types: Vec<AppTypeRow>,
}

impl Engine {
    /// `app <id>`: the app's declared mimetypes UNION the types it is currently
    /// the default for (even undeclared ones), each tagged with `declares`. The
    /// category each falls in, and whether this app is currently the default for
    /// it, are included. Rows are ordered default-first, then by mimetype.
    /// Unknown app → `UnknownApp`.
    pub fn app(&self, id: &str) -> Result<AppReport> {
        let app_id = DesktopId::new(id);
        let app = self
            .appindex
            .app(&app_id)
            .ok_or_else(|| Error::UnknownApp(app_id.to_string()))?;

        // Effective default per CANONICAL type (highest-precedence wins; first
        // also wins on canonical collision). Used for both the reverse lookup and
        // per-row default detection, so alias-keyed defaults resolve consistently.
        let mut effective: BTreeMap<MimeType, DesktopId> = BTreeMap::new();
        for (t, d) in self.defaults.effective_defaults() {
            effective.entry(self.mimedb.canonicalize(&t)).or_insert(d);
        }

        // Declared (canonical, deduped).
        let declared_set: BTreeSet<MimeType> =
            app.mimetypes.iter().map(|t| self.mimedb.canonicalize(t)).collect();

        // Row set = declared ∪ types this app is the effective default for.
        let mut row_types: BTreeSet<MimeType> = declared_set.clone();
        for (t, d) in &effective {
            if *d == app_id {
                row_types.insert(t.clone());
            }
        }

        let mut types: Vec<AppTypeRow> = row_types
            .iter()
            .map(|t| {
                let cur = effective.get(t).cloned();
                AppTypeRow {
                    mime: t.to_string(),
                    category: self.tree.category_of(t).map(|cid| self.tree.path(cid)),
                    declares: declared_set.contains(t),
                    is_default: cur.as_ref() == Some(&app_id),
                    current_default: cur.map(|d| d.to_string()),
                }
            })
            .collect();
        types.sort_by(|a, b| b.is_default.cmp(&a.is_default).then_with(|| a.mime.cmp(&b.mime)));

        let declares = types.iter().filter(|r| r.declares).count();
        let default_for = types.iter().filter(|r| r.is_default).count();
        Ok(AppReport {
            id: app_id.to_string(),
            name: app.name.clone(),
            declares,
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
        assert!(r.types[0].declares);
        assert_eq!(r.types[0].category.as_deref(), Some("Media.Video"));
        let audio = r.types.iter().find(|t| t.mime == "audio/mpeg").unwrap();
        assert!(!audio.is_default);
        assert!(audio.declares);
        assert_eq!(audio.category.as_deref(), Some("Media.Audio"));
    }

    #[test]
    fn app_unknown_errors() {
        assert!(matches!(engine().app("nope"), Err(Error::UnknownApp(_))));
    }

    #[test]
    fn app_includes_undeclared_default_type() {
        // mpv is set as default for image/png, which it does NOT declare.
        let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let cfg = std::env::temp_dir().join("madft-app-undeclared");
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("mimeapps.list"),
            "[Default Applications]\nvideo/mp4=mpv.desktop\nimage/png=mpv.desktop\n",
        )
        .unwrap();
        let roots = Roots {
            data_home: f.join("engine"),
            data_dirs: vec![f.clone()],
            config_home: cfg.clone(),
            config_dirs: vec![],
        };
        let e = Engine::load(&roots, &[]).unwrap();
        let r = e.app("mpv").unwrap();
        // 3 declared types; default for video/mp4 (declared) + image/png (undeclared).
        assert_eq!(r.declares, 3);
        assert_eq!(r.default_for, 2);
        let png = r.types.iter().find(|t| t.mime == "image/png").unwrap();
        assert!(!png.declares);
        assert!(png.is_default);
        assert_eq!(png.current_default.as_deref(), Some("mpv.desktop"));
        assert_eq!(png.category.as_deref(), Some("Media.Images"));
        let mp4 = r.types.iter().find(|t| t.mime == "video/mp4").unwrap();
        assert!(mp4.declares);
        assert!(mp4.is_default);
        let audio = r.types.iter().find(|t| t.mime == "audio/mpeg").unwrap();
        assert!(audio.declares);
        assert!(!audio.is_default);
    }
}
