//! Project-wide error type. All variants are defined here, including ones not
//! triggered until later plans (categories/engine), so the type is stable.

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("unknown category path: {0}")]
    UnknownPath(String),

    #[error("unknown application: {0}")]
    UnknownApp(String),

    #[error("'{app}' declares none of the types under '{umbrella}'")]
    AppHandlesNothingUnderUmbrella { app: String, umbrella: String },

    #[error("invalid category name: {0}")]
    InvalidCategoryName(String),

    #[error("mimetype '{mime}' is placed under both '{a}' and '{b}'")]
    DuplicatePlacement { mime: String, a: String, b: String },

    #[error("MIME database not found (looked under: {0})")]
    MimeDbNotFound(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error in {path}: {msg}")]
    Parse { path: String, msg: String },

    #[error("--types cannot be combined with a stdin type list")]
    ConflictingTypeSource,

    #[error("no mimetypes on stdin")]
    EmptyTypeList,

    #[error("no mimetype given (provide one, pipe a list, or use '-')")]
    MissingMimetype,
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_human_readable() {
        let e = Error::AppHandlesNothingUnderUmbrella {
            app: "mpv.desktop".into(),
            umbrella: "Images".into(),
        };
        assert_eq!(
            e.to_string(),
            "'mpv.desktop' declares none of the types under 'Images'"
        );
    }

    #[test]
    fn new_stdin_variants_display() {
        assert_eq!(
            Error::ConflictingTypeSource.to_string(),
            "--types cannot be combined with a stdin type list"
        );
        assert_eq!(Error::EmptyTypeList.to_string(), "no mimetypes on stdin");
        assert_eq!(
            Error::MissingMimetype.to_string(),
            "no mimetype given (provide one, pipe a list, or use '-')"
        );
    }
}