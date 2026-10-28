use axum::http::StatusCode;

/// Everything that can go wrong in the store, mapped to an HTTP status in one place so the API
/// and MCP layers cannot drift apart.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid session id")]
    InvalidId,
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("session already exists: {0}")]
    Exists(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn status(&self) -> StatusCode {
        match self {
            Error::InvalidId | Error::NotFound(_) => StatusCode::NOT_FOUND,
            Error::Exists(_) => StatusCode::CONFLICT,
            Error::BadRequest(_) => StatusCode::BAD_REQUEST,
            Error::Json(_) => StatusCode::BAD_REQUEST,
            Error::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// Errors render as `{"error": "…"}` with a matching status, so the editor and agents see the
/// same shape no matter which layer rejected the request.
impl axum::response::IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        let status = self.status();
        let body = serde_json::json!({ "error": self.to_string() });
        (status, axum::Json(body)).into_response()
    }
}
