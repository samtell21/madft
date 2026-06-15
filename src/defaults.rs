//! Reads the effective current default per type from the `mimeapps.list`
//! precedence chain. `files` are highest-precedence first.
//!
//! Plan-1 scope: resolve `[Default Applications]` only — highest file that
//! lists a type wins (its first listed desktop-id). The "must be installed"
//! cross-check and `[Removed Associations]` handling are layered in by the
//! engine (Plan 3); for current-default DISPLAY this matches the dominant case.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::types::{DesktopId, MimeType};

#[derive(Debug, Default)]
pub struct Defaults {
    /// In precedence order (highest first): each file's [Default Applications].
    files: Vec<HashMap<MimeType, DesktopId>>,
}

/// Parse a single mimeapps.list into the [Default Applications] map
/// (type -> first listed desktop-id).
fn parse_default_apps(content: &str) -> HashMap<MimeType, DesktopId> {
    let mut map = HashMap::new();
    let mut in_section = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line == "[Default Applications]";
            continue;
        }
        if !in_section || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((mime, ids)) = line.split_once('=') {
            let first = ids.split(';').map(|s| s.trim()).find(|s| !s.is_empty());
            if let Some(id) = first {
                map.entry(MimeType::new(mime.trim()))
                    .or_insert_with(|| DesktopId::new(id.to_string()));
            }
        }
    }
    map
}

impl Defaults {
    pub fn load(files: &[PathBuf]) -> Result<Self> {
        let mut out = Defaults::default();
        for path in files {
            match std::fs::read_to_string(path) {
                Ok(content) => out.files.push(parse_default_apps(&content)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::Io(e)),
            }
        }
        Ok(out)
    }

    pub fn current_default(&self, t: &MimeType) -> Option<DesktopId> {
        self.files.iter().find_map(|m| m.get(t).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DesktopId, MimeType};
    use std::path::PathBuf;

    fn file(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    #[test]
    fn reads_default_from_single_file() {
        let d = Defaults::load(&[file("tests/fixtures/config/mimeapps.list")]).unwrap();
        assert_eq!(
            d.current_default(&MimeType::new("video/mp4")),
            Some(DesktopId::new("mpv"))
        );
        assert_eq!(d.current_default(&MimeType::new("image/png")), None);
    }

    #[test]
    fn higher_precedence_file_wins() {
        // config-high listed first => higher precedence
        let d = Defaults::load(&[
            file("tests/fixtures/config-high/mimeapps.list"),
            file("tests/fixtures/config/mimeapps.list"),
        ])
        .unwrap();
        assert_eq!(
            d.current_default(&MimeType::new("text/html")),
            Some(DesktopId::new("org.mozilla.firefox"))
        );
        // video/mp4 only exists in the lower file, still found
        assert_eq!(
            d.current_default(&MimeType::new("video/mp4")),
            Some(DesktopId::new("mpv"))
        );
    }

    #[test]
    fn missing_files_are_skipped() {
        let d = Defaults::load(&[file("tests/fixtures/does-not-exist.list")]).unwrap();
        assert_eq!(d.current_default(&MimeType::new("video/mp4")), None);
    }
}
