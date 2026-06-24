//! Scans `applications/*.desktop` across the XDG path and indexes each app's
//! EXACTLY declared `MimeType=` set. This is the sole authority for
//! "app X handles type T" — never subclass-aware.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::error::Result;
use crate::paths::Roots;
use crate::types::{DesktopId, MimeType};

#[derive(Debug, Clone)]
pub struct App {
    pub id: DesktopId,
    pub name: String,
    pub nodisplay: bool,
    pub mimetypes: HashSet<MimeType>,
    pub path: PathBuf,
}

#[derive(Debug, Default)]
pub struct AppIndex {
    apps: HashMap<DesktopId, App>,
    by_type: HashMap<MimeType, Vec<DesktopId>>,
}

/// Extract the [Desktop Entry] fields the index needs, via the shared parser.
/// Returns None if there is no [Desktop Entry] group.
fn parse_desktop(content: &str) -> Option<(String, bool, HashSet<MimeType>)> {
    let file = crate::desktop::parse(content);
    let entry = file.entry_section()?;

    let name = entry.get("Name").unwrap_or("").to_string();
    let nodisplay = entry.get("NoDisplay").is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let mut mimetypes = HashSet::new();
    if let Some(list) = entry.get("MimeType") {
        for t in list.split(';') {
            let t = t.trim();
            if !t.is_empty() {
                mimetypes.insert(MimeType::new(t));
            }
        }
    }
    Some((name, nodisplay, mimetypes))
}

impl AppIndex {
    pub fn load(roots: &Roots) -> Result<Self> {
        let mut idx = AppIndex::default();

        // Highest precedence first: first-seen desktop-id wins (correct XDG;
        // NOT wofi's inverted behavior).
        for dir in roots.app_dirs() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue, // missing dir is fine
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                let Some(stem) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let id = DesktopId::new(stem.to_string());
                if idx.apps.contains_key(&id) {
                    continue; // already seen at higher precedence
                }
                let content = std::fs::read_to_string(&path)?;
                if let Some((name, nodisplay, mimetypes)) = parse_desktop(&content) {
                    for t in &mimetypes {
                        idx.by_type.entry(t.clone()).or_default().push(id.clone());
                    }
                    idx.apps.insert(
                        id.clone(),
                        App { id, name, nodisplay, mimetypes, path: path.clone() },
                    );
                }
            }
        }
        Ok(idx)
    }

    pub fn app(&self, id: &DesktopId) -> Option<&App> {
        self.apps.get(id)
    }

    pub fn apps_for_type(&self, t: &MimeType) -> Vec<&App> {
        self.by_type
            .get(t)
            .map(|ids| ids.iter().filter_map(|id| self.apps.get(id)).collect())
            .unwrap_or_default()
    }

    pub fn declares(&self, id: &DesktopId, t: &MimeType) -> bool {
        self.apps
            .get(id)
            .map(|a| a.mimetypes.contains(t))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Roots;
    use crate::types::{DesktopId, MimeType};
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn index_single_dir() -> AppIndex {
        let roots = Roots {
            data_home: fixtures(),
            data_dirs: vec![],
            config_home: PathBuf::from("/unused"),
            config_dirs: vec![],
        };
        AppIndex::load(&roots).unwrap()
    }

    #[test]
    fn apps_for_type_uses_exact_declaration() {
        let idx = index_single_dir();
        let apps = idx.apps_for_type(&MimeType::new("video/mp4"));
        assert!(apps.iter().any(|a| a.id == DesktopId::new("mpv")));
        // eog does NOT declare video/mp4
        assert!(!apps.iter().any(|a| a.id == DesktopId::new("eog")));
    }

    #[test]
    fn declares_is_exact() {
        let idx = index_single_dir();
        assert!(idx.declares(&DesktopId::new("mpv"), &MimeType::new("audio/mpeg")));
        assert!(!idx.declares(&DesktopId::new("mpv"), &MimeType::new("image/png")));
    }

    #[test]
    fn home_dir_shadows_system_for_same_id() {
        // data_home = fixtures/local, data_dirs = [fixtures]; both have webcam.desktop
        let roots = Roots {
            data_home: fixtures().join("local"),
            data_dirs: vec![fixtures()],
            config_home: PathBuf::from("/unused"),
            config_dirs: vec![],
        };
        let idx = AppIndex::load(&roots).unwrap();
        let app = idx.app(&DesktopId::new("webcam")).unwrap();
        assert_eq!(app.name, "Webcam HOME");
    }

    #[test]
    fn app_records_its_source_path() {
        let idx = index_single_dir();
        let app = idx.app(&DesktopId::new("mpv")).unwrap();
        assert!(app.path.ends_with("mpv.desktop"), "got {:?}", app.path);
    }
}
