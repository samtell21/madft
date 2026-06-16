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
        let cat_defaults = FileSource::new(roots.data_home.join("madft/categories.toml"));
        let cat_overrides = FileSource::new(roots.config_home.join("madft/overrides.toml"));
        let tree = categories::build(&cat_defaults, &cat_overrides, &mimedb)?;
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

    /// Resolve a `<PATH|mimetype>` target. A target containing '/' is a mimetype
    /// (umbrella = just that canonical type); otherwise it is a category path
    /// (umbrella = its recursive types). `'/'` is the mimetype's own delimiter
    /// and never appears in a category name (spec §2).
    fn resolve_umbrella(&self, target: &str) -> Result<(String, Vec<MimeType>)> {
        if target.contains('/') {
            let canon = self.mimedb.canonicalize(&MimeType::new(target));
            Ok((canon.to_string(), vec![canon]))
        } else {
            let id = self
                .tree
                .node_by_path(target)
                .ok_or_else(|| Error::UnknownPath(target.to_string()))?;
            Ok((self.tree.path(id), self.tree.types_under(id)))
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
    pub fn apps(&self, target: &str) -> Result<AppsResult> {
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
        assert_eq!(info.comment, None);
        assert_eq!(info.applicable_count, 1);
        assert_eq!(info.applicable_apps[0].id, "eog.desktop");
    }

    #[test]
    fn info_reports_transitive_ancestors() {
        let e = engine();
        let info = e.info("image/svg+xml").unwrap();
        assert_eq!(info.ancestor_types, vec!["application/xml", "text/plain"]);
    }

    #[test]
    fn apps_sorted_by_coverage() {
        let e = engine();
        let r = e.apps("Media").unwrap();
        let ids: Vec<&str> = r.apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["mpv.desktop", "eog.desktop", "webcam.desktop"]);
        assert_eq!(r.apps[0].coverage, 3);
        assert_eq!(r.apps[1].coverage, 2);
        assert_eq!(r.apps[2].coverage, 1);
    }

    #[test]
    fn apps_for_a_mimetype_target() {
        let e = engine();
        let r = e.apps("video/mp4").unwrap();
        assert_eq!(r.target, "video/mp4");
        assert_eq!(r.types, vec!["video/mp4"]);
        let ids: Vec<&str> = r.apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["mpv.desktop", "webcam.desktop"]);
    }

    #[test]
    fn get_returns_current_default() {
        let e = engine();
        assert_eq!(e.get("video/mp4"), Some("mpv.desktop".to_string()));
        assert_eq!(e.get("image/png"), None);
    }
}
