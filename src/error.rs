use std::fmt;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("http status {status} from OpenAPI")]
    HttpStatus { status: u16 },

    #[error("api error {code}: {message}")]
    Api { code: i64, message: String },

    #[error("validation error: {0}")]
    Validation(String),

    #[error("JSON error at line {}, column {}", .0.line(), .0.column())]
    Serde(serde_json::Error),

    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),
}

impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) => formatter.debug_tuple("Config").field(message).finish(),
            Self::Transport(message) => formatter.debug_tuple("Transport").field(message).finish(),
            Self::HttpStatus { status } => formatter
                .debug_struct("HttpStatus")
                .field("status", status)
                .finish(),
            Self::Api { code, message } => formatter
                .debug_struct("Api")
                .field("code", code)
                .field("message", message)
                .finish(),
            Self::Validation(message) => {
                formatter.debug_tuple("Validation").field(message).finish()
            }
            Self::Serde(error) => formatter
                .debug_struct("Serde")
                .field("category", &error.classify())
                .field("line", &error.line())
                .field("column", &error.column())
                .finish(),
            Self::Url(error) => formatter.debug_tuple("Url").field(error).finish(),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Serde(error)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn serde_error_diagnostics_hide_offending_values() {
        let source = serde_json::from_str::<bool>(r#""body-secret""#).expect_err("type mismatch");
        assert!(source.to_string().contains("body-secret"));

        let error = Error::from(source);
        let rendered = format!("{error:?} {error}");

        assert!(rendered.contains("Serde"));
        assert!(rendered.contains("JSON error at line 1"));
        assert!(!rendered.contains("body-secret"));
        assert!(error.source().is_none());

        let Error::Serde(source) = &error else {
            panic!("expected serde error");
        };
        assert!(source.to_string().contains("body-secret"));
    }
}
