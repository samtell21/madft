//! The XDG directory set, injectable so tests never touch the real system.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Roots {
    pub data_home: PathBuf,
    pub data_dirs: Vec<PathBuf>,
    pub config_home: PathBuf,
    pub config_dirs: Vec<PathBuf>,
}

fn split_paths(var: &str, default: &str) -> Vec<PathBuf> {
    let raw = std::env::var(var).unwrap_or_default();
    let raw = if raw.is_empty() { default.to_string() } else { raw };
    raw.split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

impl Roots {
    /// Build from the live environment, applying XDG defaults.
    pub fn from_env() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let data_home = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(".local/share"));
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(&home).join(".config"));
        Roots {
            data_home,
            data_dirs: split_paths("XDG_DATA_DIRS", "/usr/local/share:/usr/share"),
            config_home,
            config_dirs: split_paths("XDG_CONFIG_DIRS", "/etc/xdg"),
        }
    }

    /// `applications/` dirs, highest precedence first (data_home, then data_dirs).
    pub fn app_dirs(&self) -> Vec<PathBuf> {
        let mut v = vec![self.data_home.join("applications")];
        v.extend(self.data_dirs.iter().map(|d| d.join("applications")));
        v
    }

    /// `mime/` base dirs (shared-mime-info), user first then system. Order is not
    /// critical: the MIME DB is a union of these.
    pub fn mime_dirs(&self) -> Vec<PathBuf> {
        let mut v = vec![self.data_home.join("mime")];
        v.extend(self.data_dirs.iter().map(|d| d.join("mime")));
        v
    }

    /// `mimeapps.list` candidate files, highest precedence first.
    /// `desktops` is the lowercased $XDG_CURRENT_DESKTOP list (may be empty).
    pub fn mimeapps_files(&self, desktops: &[String]) -> Vec<PathBuf> {
        let mut v = Vec::new();
        // config_home: desktop-prefixed first, then plain
        for d in desktops {
            v.push(self.config_home.join(format!("{d}-mimeapps.list")));
        }
        v.push(self.config_home.join("mimeapps.list"));
        // config_dirs
        for dir in &self.config_dirs {
            for d in desktops {
                v.push(dir.join(format!("{d}-mimeapps.list")));
            }
            v.push(dir.join("mimeapps.list"));
        }
        // data_home/applications
        for d in desktops {
            v.push(self.data_home.join("applications").join(format!("{d}-mimeapps.list")));
        }
        v.push(self.data_home.join("applications/mimeapps.list"));
        // data_dirs/applications
        for dir in &self.data_dirs {
            let apps = dir.join("applications");
            for d in desktops {
                v.push(apps.join(format!("{d}-mimeapps.list")));
            }
            v.push(apps.join("mimeapps.list"));
        }
        v
    }

    /// Where madft WRITES user defaults.
    pub fn user_mimeapps(&self) -> PathBuf {
        self.config_home.join("mimeapps.list")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_roots() -> Roots {
        Roots {
            data_home: PathBuf::from("/home/u/.local/share"),
            data_dirs: vec![PathBuf::from("/usr/share")],
            config_home: PathBuf::from("/home/u/.config"),
            config_dirs: vec![PathBuf::from("/etc/xdg")],
        }
    }

    #[test]
    fn app_dirs_put_home_first() {
        let r = fixture_roots();
        assert_eq!(
            r.app_dirs(),
            vec![
                PathBuf::from("/home/u/.local/share/applications"),
                PathBuf::from("/usr/share/applications"),
            ]
        );
    }

    #[test]
    fn mimeapps_precedence_config_home_first() {
        let r = fixture_roots();
        let files = r.mimeapps_files(&["sway".to_string()]);
        assert_eq!(files[0], PathBuf::from("/home/u/.config/sway-mimeapps.list"));
        assert_eq!(files[1], PathBuf::from("/home/u/.config/mimeapps.list"));
        // user write target is config_home/mimeapps.list
        assert_eq!(r.user_mimeapps(), PathBuf::from("/home/u/.config/mimeapps.list"));
    }

    #[test]
    fn no_desktop_skips_prefixed_files() {
        let r = fixture_roots();
        let files = r.mimeapps_files(&[]);
        assert_eq!(files[0], PathBuf::from("/home/u/.config/mimeapps.list"));
    }
}
