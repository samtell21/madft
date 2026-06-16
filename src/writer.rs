//! Mutates the user's `mimeapps.list`. `apply` is a PURE transform over file
//! content; `write_user_defaults` wraps it with a `.bak` copy and an atomic
//! temp+rename. Edits touch ONLY the `[Default Applications]` section —
//! everything else (other sections, keys, ordering, comments) round-trips
//! verbatim (spec §6). Never creates `[Added]`/`[Removed]` sections.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::types::{DesktopId, MimeType};

const DEFAULT_APPS: &str = "[Default Applications]";

/// One change to the `[Default Applications]` section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Upsert `mime=app` (replace in place if present, else insert).
    Set(MimeType, DesktopId),
    /// Remove the `mime` key if present.
    Unset(MimeType),
}

fn is_section_header(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('[') && t.ends_with(']')
}

/// The key of a `key=value` line (trimmed), or `None` for comments, blanks, and
/// section headers.
fn line_key(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') || is_section_header(t) {
        return None;
    }
    t.split_once('=').map(|(k, _)| k.trim())
}

/// Apply `edits` to `mimeapps.list` `content`, returning the new content. Pure:
/// no I/O. Preserves all sections/keys/order/comments; edits only
/// `[Default Applications]`. Creates that section (or a minimal file) if absent.
pub fn apply(content: &str, edits: &[Edit]) -> String {
    let lines: Vec<&str> = content.lines().collect();

    let header_idx = lines.iter().position(|l| l.trim() == DEFAULT_APPS);

    let mut out: Vec<String> = Vec::new();
    match header_idx {
        Some(h) => {
            for line in &lines[..=h] {
                out.push((*line).to_string());
            }
            // Section body runs to the next section header (or EOF).
            let end = lines
                .iter()
                .enumerate()
                .skip(h + 1)
                .find(|(_, l)| is_section_header(l))
                .map(|(j, _)| j)
                .unwrap_or(lines.len());
            let mut section: Vec<String> =
                lines[h + 1..end].iter().map(|s| (*s).to_string()).collect();
            apply_to_section(&mut section, edits);
            out.extend(section);
            for line in &lines[end..] {
                out.push((*line).to_string());
            }
        }
        None => {
            for line in &lines {
                out.push((*line).to_string());
            }
            // Separate any prior content from the new section with one blank line.
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(DEFAULT_APPS.to_string());
            for edit in edits {
                if let Edit::Set(m, d) = edit {
                    out.push(format!("{}={}", m.as_str(), d.as_str()));
                }
            }
        }
    }

    let mut result = out.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }
    result
}

/// Apply edits within a single section body (lines between the header and the
/// next header/EOF). `Set` replaces in place or inserts; `Unset` deletes.
fn apply_to_section(section: &mut Vec<String>, edits: &[Edit]) {
    for edit in edits {
        match edit {
            Edit::Set(mime, app) => {
                let key = mime.as_str();
                let new_line = format!("{}={}", key, app.as_str());
                if let Some(pos) = section.iter().position(|l| line_key(l) == Some(key)) {
                    section[pos] = new_line;
                } else {
                    // Insert after the last non-blank line (before trailing blanks).
                    let at = section
                        .iter()
                        .rposition(|l| !l.trim().is_empty())
                        .map(|p| p + 1)
                        .unwrap_or(section.len());
                    section.insert(at, new_line);
                }
            }
            Edit::Unset(mime) => {
                let key = mime.as_str();
                section.retain(|l| line_key(l) != Some(key));
            }
        }
    }
}

fn bak_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(".bak");
    PathBuf::from(p)
}

/// Write `content` to `path` atomically: temp file in the same directory →
/// fsync → rename over the target (spec §6).
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let fname = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("mimeapps.list");
    let tmp = dir.join(format!(".{fname}.madft.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Read → apply → write the user defaults file. Idempotent: if applying the
/// edits does not change the content, nothing is written (returns `Ok(false)`).
/// Backs the file up to `<path>.bak` before writing. Creates the file (minimal)
/// if it does not exist. Returns `Ok(true)` if a write occurred.
pub fn write_user_defaults(path: &Path, edits: &[Edit]) -> Result<bool> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Error::Io(e)),
    };
    let updated = apply(&existing, edits);
    if updated == existing {
        return Ok(false);
    }
    if !existing.is_empty() {
        std::fs::copy(path, bak_path(path))?;
    }
    atomic_write(path, &updated)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(m: &str, a: &str) -> Edit {
        Edit::Set(MimeType::new(m), DesktopId::new(a))
    }

    #[test]
    fn upsert_replaces_in_place() {
        let before = "[Default Applications]\nvideo/mp4=old.desktop\n";
        let after = apply(before, &[set("video/mp4", "mpv")]);
        assert_eq!(after, "[Default Applications]\nvideo/mp4=mpv.desktop\n");
    }

    #[test]
    fn upsert_appends_new_key() {
        let before = "[Default Applications]\nvideo/mp4=mpv.desktop\n";
        let after = apply(before, &[set("audio/mpeg", "mpv")]);
        assert_eq!(
            after,
            "[Default Applications]\nvideo/mp4=mpv.desktop\naudio/mpeg=mpv.desktop\n"
        );
    }

    #[test]
    fn unset_removes_key() {
        let before = "[Default Applications]\nvideo/mp4=mpv.desktop\ntext/html=ff.desktop\n";
        let after = apply(before, &[Edit::Unset(MimeType::new("video/mp4"))]);
        assert_eq!(after, "[Default Applications]\ntext/html=ff.desktop\n");
    }

    #[test]
    fn preserves_other_sections_and_comments() {
        let before = "# my file\n[Default Applications]\nvideo/mp4=mpv.desktop\n\n\
                      [Added Associations]\nvideo/mp4=mpv.desktop;vlc.desktop\n";
        let after = apply(before, &[set("text/html", "ff")]);
        assert!(after.contains("# my file\n"));
        assert!(after.contains("[Added Associations]\nvideo/mp4=mpv.desktop;vlc.desktop\n"));
        assert!(after.contains("video/mp4=mpv.desktop\ntext/html=ff.desktop\n\n[Added Associations]"));
    }

    #[test]
    fn idempotent_when_value_unchanged() {
        let before = "[Default Applications]\nvideo/mp4=mpv.desktop\n";
        let after = apply(before, &[set("video/mp4", "mpv")]);
        assert_eq!(after, before);
    }

    #[test]
    fn creates_section_when_absent() {
        let after = apply("", &[set("video/mp4", "mpv")]);
        assert_eq!(after, "[Default Applications]\nvideo/mp4=mpv.desktop\n");
    }

    #[test]
    fn creates_section_after_existing_unrelated_content() {
        let before = "[Added Associations]\nvideo/mp4=vlc.desktop\n";
        let after = apply(before, &[set("video/mp4", "mpv")]);
        assert_eq!(
            after,
            "[Added Associations]\nvideo/mp4=vlc.desktop\n\n[Default Applications]\nvideo/mp4=mpv.desktop\n"
        );
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("madft-writer-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_user_defaults_backs_up_and_is_idempotent() {
        let dir = temp_dir("backup");
        let path = dir.join("mimeapps.list");
        std::fs::write(&path, "[Default Applications]\nvideo/mp4=old.desktop\n").unwrap();

        let wrote = write_user_defaults(&path, &[set("video/mp4", "mpv")]).unwrap();
        assert!(wrote);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[Default Applications]\nvideo/mp4=mpv.desktop\n"
        );
        let bak = path.with_file_name("mimeapps.list.bak");
        assert_eq!(
            std::fs::read_to_string(&bak).unwrap(),
            "[Default Applications]\nvideo/mp4=old.desktop\n"
        );

        let wrote_again = write_user_defaults(&path, &[set("video/mp4", "mpv")]).unwrap();
        assert!(!wrote_again);
    }

    #[test]
    fn write_user_defaults_creates_missing_file() {
        let dir = temp_dir("create");
        let path = dir.join("mimeapps.list");
        assert!(!path.exists());
        let wrote = write_user_defaults(&path, &[set("video/mp4", "mpv")]).unwrap();
        assert!(wrote);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[Default Applications]\nvideo/mp4=mpv.desktop\n"
        );
        assert!(!path.with_file_name("mimeapps.list.bak").exists());
    }
}
