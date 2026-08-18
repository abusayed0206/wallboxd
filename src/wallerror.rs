/// Errors from local poster-wall generation, distinguishing transient
/// network failures (worth retrying) from permanent ones (bad username, no
/// diary entries) — mirrors the retry classification `wallpaper.rs` used to
/// apply to the old remote API's HTTP status codes.
#[derive(Debug)]
pub enum WallError {
    NotFound(String),
    Network(String),
    Other(String),
}

impl WallError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, WallError::Network(_))
    }
}

impl std::fmt::Display for WallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WallError::NotFound(s) | WallError::Network(s) | WallError::Other(s) => {
                write!(f, "{s}")
            }
        }
    }
}

impl From<reqwest::Error> for WallError {
    fn from(e: reqwest::Error) -> Self {
        let retryable = e.is_timeout()
            || e.is_connect()
            || e.is_request()
            || e.status().is_some_and(|s| s.is_server_error());
        if retryable {
            WallError::Network(e.to_string())
        } else {
            WallError::Other(e.to_string())
        }
    }
}
