use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct MimeType(pub String);

impl MimeType {
    pub fn new(s: impl Into<String>) -> Self {
        MimeType(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MimeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct DesktopId(pub String);

impl DesktopId {
    /// Accepts "mpv" or "mpv.desktop"; always stores the `.desktop` form.
    pub fn new(s: impl Into<String>) -> Self {
        let s = s.into();
        if s.ends_with(".desktop") {
            DesktopId(s)
        } else {
            DesktopId(format!("{s}.desktop"))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DesktopId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_id_normalizes_suffix() {
        assert_eq!(DesktopId::new("mpv"), DesktopId::new("mpv.desktop"));
        assert_eq!(DesktopId::new("mpv").as_str(), "mpv.desktop");
    }

    #[test]
    fn display_roundtrips() {
        assert_eq!(MimeType::new("video/mp4").to_string(), "video/mp4");
        assert_eq!(DesktopId::new("mpv").to_string(), "mpv.desktop");
    }
}
