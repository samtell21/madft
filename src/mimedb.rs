//! Reads the freedesktop shared-mime-info data files: `types`, `subclasses`,
//! `aliases`. Provides the type universe, alias canonicalization, and the
//! subclass DAG (`supertypes` direct, `ancestor_types` transitive).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::types::MimeType;

#[derive(Debug, Default)]
pub struct MimeDb {
    types: HashSet<MimeType>,
    /// child -> its direct supertypes (parents in the subclass DAG)
    supertypes: HashMap<MimeType, Vec<MimeType>>,
    /// alias -> canonical
    aliases: HashMap<MimeType, MimeType>,
}

fn read_lines(path: &PathBuf) -> Result<Vec<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.to_string())
            .collect()),
        // a missing optional file is not an error; return nothing
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(Error::Io(e)),
    }
}

impl MimeDb {
    pub fn load(mime_dirs: &[PathBuf]) -> Result<Self> {
        let mut db = MimeDb::default();
        let mut found_any_types = false;

        for dir in mime_dirs {
            let types_file = dir.join("types");
            let lines = read_lines(&types_file)?;
            if !lines.is_empty() {
                found_any_types = true;
            }
            for t in lines {
                db.types.insert(MimeType::new(t));
            }

            for line in read_lines(&dir.join("subclasses"))? {
                if let Some((child, parent)) = line.split_once(char::is_whitespace) {
                    db.supertypes
                        .entry(MimeType::new(child.trim()))
                        .or_default()
                        .push(MimeType::new(parent.trim()));
                }
            }

            for line in read_lines(&dir.join("aliases"))? {
                if let Some((alias, canonical)) = line.split_once(char::is_whitespace) {
                    db.aliases
                        .insert(MimeType::new(alias.trim()), MimeType::new(canonical.trim()));
                }
            }
        }

        if !found_any_types {
            let looked: Vec<String> =
                mime_dirs.iter().map(|d| d.display().to_string()).collect();
            return Err(Error::MimeDbNotFound(looked.join(", ")));
        }
        Ok(db)
    }

    pub fn all_types(&self) -> impl Iterator<Item = &MimeType> {
        self.types.iter()
    }

    pub fn canonicalize(&self, t: &MimeType) -> MimeType {
        self.aliases.get(t).cloned().unwrap_or_else(|| t.clone())
    }

    /// Direct supertypes (one level up the DAG), alias-canonicalized.
    pub fn supertypes(&self, t: &MimeType) -> Vec<MimeType> {
        let t = self.canonicalize(t);
        self.supertypes
            .get(&t)
            .map(|v| v.iter().map(|p| self.canonicalize(p)).collect())
            .unwrap_or_default()
    }

    /// Transitive supertypes (breadth-first, deduped, excluding `t` itself).
    /// This is the "what you'd inherit if unset" chain.
    pub fn ancestor_types(&self, t: &MimeType) -> Vec<MimeType> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut queue: VecDeque<MimeType> = self.supertypes(t).into_iter().collect();
        while let Some(cur) = queue.pop_front() {
            if !seen.insert(cur.clone()) {
                continue;
            }
            out.push(cur.clone());
            for p in self.supertypes(&cur) {
                queue.push_back(p);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MimeType;
    use std::path::PathBuf;

    fn db() -> MimeDb {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mime");
        MimeDb::load(&[dir]).expect("load fixture mime db")
    }

    #[test]
    fn universe_contains_known_types() {
        let db = db();
        assert!(db.all_types().any(|t| t.as_str() == "video/mp4"));
        assert!(db.all_types().any(|t| t.as_str() == "text/html"));
    }

    #[test]
    fn canonicalize_resolves_alias() {
        let db = db();
        assert_eq!(
            db.canonicalize(&MimeType::new("image/jpg")),
            MimeType::new("image/jpeg")
        );
        // non-alias passes through
        assert_eq!(
            db.canonicalize(&MimeType::new("image/png")),
            MimeType::new("image/png")
        );
    }

    #[test]
    fn supertypes_are_direct_parents() {
        let db = db();
        assert_eq!(
            db.supertypes(&MimeType::new("text/html")),
            vec![MimeType::new("text/plain")]
        );
    }

    #[test]
    fn ancestor_types_are_transitive() {
        let db = db();
        // svg -> application/xml -> text/plain
        assert_eq!(
            db.ancestor_types(&MimeType::new("image/svg+xml")),
            vec![MimeType::new("application/xml"), MimeType::new("text/plain")]
        );
    }

    #[test]
    fn missing_db_is_error() {
        let bad = PathBuf::from("/nonexistent/mime/dir");
        assert!(MimeDb::load(&[bad]).is_err());
    }
}
