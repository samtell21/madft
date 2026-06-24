//! Faithful, order-preserving parser for freedesktop `.desktop` files.
//! Values are raw strings — no type coercion, no `Exec` splitting, keys verbatim.

#[derive(Debug, Clone)]
pub struct DesktopFile {
    pub path: String,
    pub sections: Vec<DesktopSection>,
}

#[derive(Debug, Clone)]
pub struct DesktopSection {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

impl DesktopSection {
    /// Exact, case-sensitive lookup of a key's value within this section.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

impl DesktopFile {
    /// The `[Desktop Entry]` section, if present.
    pub fn entry_section(&self) -> Option<&DesktopSection> {
        self.sections.iter().find(|s| s.name == "Desktop Entry")
    }
}

/// Parse `.desktop` content into ordered sections of ordered key/value pairs.
/// Faithful: keys verbatim, values raw, file order preserved. The first
/// occurrence of a key within a section wins (keeps emitted JSON objects valid).
/// `path` is left empty for the caller to populate.
pub fn parse(content: &str) -> DesktopFile {
    let mut sections: Vec<DesktopSection> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let name = line[1..line.len() - 1].to_string();
            sections.push(DesktopSection { name, entries: Vec::new() });
            continue;
        }
        let Some(section) = sections.last_mut() else {
            continue; // key/value before the first header — ignore
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = value.trim().to_string();
        if section.entries.iter().any(|(k, _)| *k == key) {
            continue; // first occurrence wins
        }
        section.entries.push((key, value));
    }

    DesktopFile { path: String::new(), sections }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_and_keys_in_order() {
        let f = parse("[Desktop Entry]\nName=Neovim\nExec=nvim %F\nTerminal=true\n");
        assert_eq!(f.sections.len(), 1);
        let s = &f.sections[0];
        assert_eq!(s.name, "Desktop Entry");
        assert_eq!(
            s.entries,
            vec![
                ("Name".to_string(), "Neovim".to_string()),
                ("Exec".to_string(), "nvim %F".to_string()),
                ("Terminal".to_string(), "true".to_string()),
            ]
        );
    }

    #[test]
    fn skips_comments_blanks_and_preamble() {
        let f = parse("# a comment\npreamble=ignored\n\n[Desktop Entry]\n# inner\nName=X\n\n");
        assert_eq!(f.sections.len(), 1);
        assert_eq!(f.sections[0].entries, vec![("Name".to_string(), "X".to_string())]);
    }

    #[test]
    fn keeps_verbatim_keys_locales_and_extensions() {
        let f = parse("[Desktop Entry]\nName[de]=Editor\nX-GNOME-Autostart=true\n");
        let s = &f.sections[0];
        assert_eq!(s.get("Name[de]"), Some("Editor"));
        assert_eq!(s.get("X-GNOME-Autostart"), Some("true"));
    }

    #[test]
    fn first_key_wins_and_case_is_distinct() {
        let f = parse("[Desktop Entry]\nExec=first\nExec=second\nexec=lower\n");
        let s = &f.sections[0];
        assert_eq!(s.get("Exec"), Some("first"));
        assert_eq!(s.get("exec"), Some("lower"));
    }

    #[test]
    fn splits_value_on_first_equals_only() {
        let f = parse("[Desktop Entry]\nExec=env A=b app %U\n");
        assert_eq!(f.sections[0].get("Exec"), Some("env A=b app %U"));
    }

    #[test]
    fn captures_action_sections() {
        let f = parse("[Desktop Entry]\nName=X\n[Desktop Action new-window]\nName=New Window\nExec=app --new\n");
        assert_eq!(f.sections.len(), 2);
        assert_eq!(f.sections[1].name, "Desktop Action new-window");
        assert_eq!(f.sections[1].get("Exec"), Some("app --new"));
    }

    #[test]
    fn entry_section_finds_desktop_entry() {
        let f = parse("[Desktop Action x]\nName=A\n[Desktop Entry]\nName=B\n");
        assert_eq!(f.entry_section().unwrap().get("Name"), Some("B"));
    }
}
