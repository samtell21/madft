//! The `Source` trait abstracts "load one layer of category placements" so the
//! file backend can later be swapped for a remote, community-maintained DB
//! (spec §4). MVP ships `FileSource` (TOML) only; the trait seam is the only
//! remote-readiness required now.
//!
//! The loader walks a generic `toml::Table` rather than deriving serde structs,
//! so it can attach precise `DuplicatePlacement` / `InvalidCategoryName` /
//! `Parse` messages (spec §7). Category keys MUST be quoted dotted paths
//! (`["Media.Video"]`); an unquoted `[Media.Video]` nests in TOML and is
//! rejected with a guiding Parse error.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::types::MimeType;

/// One category's directly-placed types, as declared by a single source layer.
/// `path` is the dotted category path (e.g. "Media.Video"); ancestors are
/// derived later (in `merge`) from its prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategorySpec {
    pub path: String,
    pub types: Vec<MimeType>,
}

/// A layer of the category tree (`defaults` or `overrides`).
pub trait Source {
    /// Load this layer's placements. An ABSENT file is NOT an error — it yields
    /// an empty layer (a user with no `overrides.toml` is the common case).
    fn load(&self) -> Result<Vec<CategorySpec>>;
}

/// Reads a single TOML file in the category grammar. Used for both the
/// `defaults` (categories.toml) and `overrides` (overrides.toml) layers.
#[derive(Clone, Debug)]
pub struct FileSource {
    pub path: PathBuf,
}

impl FileSource {
    pub fn new(path: PathBuf) -> Self {
        FileSource { path }
    }
}

impl Source for FileSource {
    fn load(&self) -> Result<Vec<CategorySpec>> {
        let content = match std::fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::Io(e)),
        };
        parse_categories(&content, &self.path.display().to_string())
    }
}

/// The built-in default category tree, compiled into the binary.
pub const DEFAULT_CATEGORIES: &str = include_str!("../../data/categories.toml");

/// A `Source` backed by an in-memory string — used for the built-in default
/// tree when no on-disk `categories.toml` exists, so the tree is never empty.
#[derive(Clone, Debug)]
pub struct StaticSource {
    content: &'static str,
}

impl StaticSource {
    pub fn new(content: &'static str) -> Self {
        StaticSource { content }
    }
}

impl Source for StaticSource {
    fn load(&self) -> Result<Vec<CategorySpec>> {
        parse_categories(self.content, "<built-in default>")
    }
}

/// Write the built-in default category tree to `path` (creating parent dirs).
/// Returns `Ok(true)` if written, `Ok(false)` if the file already exists and
/// `force` is false (left untouched).
pub fn write_default_categories(path: &Path, force: bool) -> Result<bool> {
    if path.exists() && !force {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, DEFAULT_CATEGORIES)?;
    Ok(true)
}

/// A category-name segment may contain only `[A-Za-z0-9 _-]` (no '.', ':', '/')
/// and may not be empty (spec §2).
fn valid_category_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '_' || c == '-')
}

/// Parse the category TOML grammar into validated specs. `where_` labels error
/// messages (the file path). Validates: well-formed TOML; category-name charset
/// on every dotted segment; single-placement (a type may not appear under two
/// different category paths within THIS file). Type strings are stored
/// as-written; alias canonicalization happens later in `merge` (which has the
/// `MimeDb`).
pub fn parse_categories(content: &str, where_: &str) -> Result<Vec<CategorySpec>> {
    let table: toml::Table = content.parse().map_err(|e: toml::de::Error| Error::Parse {
        path: where_.to_string(),
        msg: e.to_string(),
    })?;

    let mut specs = Vec::new();
    // type -> the (single) path it was placed under, for the single-placement guard
    let mut placed: HashMap<MimeType, String> = HashMap::new();

    for (path, value) in &table {
        // Every dotted segment must satisfy the name charset.
        for segment in path.split('.') {
            if !valid_category_name(segment) {
                return Err(Error::InvalidCategoryName(path.clone()));
            }
        }

        // The value must be a table whose only key is an optional `types` array.
        // A nested table (what an UNQUOTED `[Media.Video]` produces) trips the
        // unexpected-key arm with a message pointing at quoted dotted keys.
        let item = value.as_table().ok_or_else(|| Error::Parse {
            path: where_.to_string(),
            msg: format!("category '{path}' must be a table with a `types` array"),
        })?;

        let mut types = Vec::new();
        for (key, val) in item {
            if key != "types" {
                return Err(Error::Parse {
                    path: where_.to_string(),
                    msg: format!(
                        "category '{path}' has unexpected key '{key}'; use quoted dotted keys \
                         like [\"Media.Video\"] and only a `types` array"
                    ),
                });
            }
            let arr = val.as_array().ok_or_else(|| Error::Parse {
                path: where_.to_string(),
                msg: format!("`types` of '{path}' must be an array of strings"),
            })?;
            for entry in arr {
                let s = entry.as_str().ok_or_else(|| Error::Parse {
                    path: where_.to_string(),
                    msg: format!("`types` of '{path}' must contain only strings"),
                })?;
                let mime = MimeType::new(s);
                match placed.get(&mime) {
                    Some(other) if other != path => {
                        return Err(Error::DuplicatePlacement {
                            mime: mime.to_string(),
                            a: other.clone(),
                            b: path.clone(),
                        });
                    }
                    Some(_) => {} // same path repeat: dedupe below
                    None => {
                        placed.insert(mime.clone(), path.clone());
                    }
                }
                if !types.contains(&mime) {
                    types.push(mime);
                }
            }
        }
        specs.push(CategorySpec { path: path.clone(), types });
    }
    Ok(specs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn file_source_loads_specs() {
        let src = FileSource::new(fixtures().join("categories/categories.toml"));
        let specs = src.load().unwrap();
        // toml::Table iterates in sorted key order, so specs are sorted by path.
        let media_video = specs.iter().find(|s| s.path == "Media.Video").unwrap();
        assert_eq!(
            media_video.types,
            vec![MimeType::new("video/mp4"), MimeType::new("video/x-matroska")]
        );
        // image/jpg is stored as-written here (canonicalization is merge's job).
        let images = specs.iter().find(|s| s.path == "Images").unwrap();
        assert_eq!(
            images.types,
            vec![MimeType::new("image/png"), MimeType::new("image/jpg")]
        );
    }

    #[test]
    fn missing_file_is_an_empty_layer() {
        let src = FileSource::new(fixtures().join("categories/does-not-exist.toml"));
        assert_eq!(src.load().unwrap(), vec![]);
    }

    #[test]
    fn duplicate_placement_within_a_file_errors() {
        let toml = r#"
["Media"]
types = ["video/mp4"]

["Films"]
types = ["video/mp4"]
"#;
        let err = parse_categories(toml, "test").unwrap_err();
        match err {
            Error::DuplicatePlacement { mime, .. } => assert_eq!(mime, "video/mp4"),
            other => panic!("expected DuplicatePlacement, got {other:?}"),
        }
    }

    #[test]
    fn same_path_repeat_is_deduped_not_an_error() {
        let toml = r#"
["Media"]
types = ["video/mp4", "video/mp4"]
"#;
        let specs = parse_categories(toml, "test").unwrap();
        assert_eq!(specs[0].types, vec![MimeType::new("video/mp4")]);
    }

    #[test]
    fn invalid_category_name_errors() {
        // ':' is forbidden in a category name.
        let toml = "[\"Bad:Name\"]\ntypes = [\"video/mp4\"]\n";
        let err = parse_categories(toml, "test").unwrap_err();
        assert!(matches!(err, Error::InvalidCategoryName(_)));
    }

    #[test]
    fn malformed_toml_is_a_parse_error() {
        let err = parse_categories("this is not = = toml [[[", "test").unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }

    #[test]
    fn unquoted_nested_key_is_rejected() {
        // Unquoted [Media.Video] nests as Media -> Video, so the Media table has
        // an unexpected key 'Video' instead of `types`.
        let toml = "[Media.Video]\ntypes = [\"video/mp4\"]\n";
        let err = parse_categories(toml, "test").unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }

    #[test]
    fn default_categories_is_valid() {
        let specs = parse_categories(DEFAULT_CATEGORIES, "<built-in>").unwrap();
        assert!(!specs.is_empty());
        assert!(specs.iter().any(|s| s.path == "Media.Video"));
        assert!(specs.iter().any(|s| s.path == "Images"));
    }
}
