use thiserror::Error;

/// Errors that can occur in the octx library.
#[derive(Error, Debug)]
pub enum OctxError {
    /// I/O error wrapper.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP request error.
    #[error("HTTP error: {0}")]
    Http(String),

    /// Registry operation error.
    #[error("Registry error: {0}")]
    Registry(String),

    /// Unsupported platform.
    #[error("Platform not supported: {0}")]
    UnsupportedPlatform(String),

    /// Checksum verification failure.
    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Expected checksum hex string.
        expected: String,
        /// Actual checksum hex string.
        actual: String,
    },

    /// Configuration error.
    #[error("{0}")]
    Config(String),

    /// Credential storage error.
    #[error("Credential error: {0}")]
    Creds(String),

    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Network error wrapper.
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    /// Serialization/deserialization error wrapper.
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl OctxError {
    /// Returns the appropriate process exit code for this error.
    ///
    /// * 0 = success (never returned by this method — use for reference)
    /// * 1 = generic failure
    /// * 2 = resource not found
    /// * 3 = network/connectivity failure
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::NotFound(_) => 2,
            Self::Network(_) => 3,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_io() {
        let err = OctxError::Io(std::io::Error::new(std::io::ErrorKind::Other, "disk full"));
        assert_eq!(err.to_string(), "I/O error: disk full");
    }

    #[test]
    fn test_error_display_http() {
        let err = OctxError::Http("404 Not Found".into());
        assert_eq!(err.to_string(), "HTTP error: 404 Not Found");
    }

    #[test]
    fn test_error_display_checksum_mismatch() {
        let err = OctxError::ChecksumMismatch {
            expected: "abc".into(),
            actual: "def".into(),
        };
        assert_eq!(err.to_string(), "Checksum mismatch: expected abc, got def");
    }

    #[test]
    fn test_exit_code_for_not_found_returns_2() {
        let err = OctxError::NotFound("fmt".into());
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn test_exit_code_for_network_returns_3() {
        // Build a reqwest error by mocking — use `reqwest::Error::without_url` for test
        // reqwest::Error::new(reqwest::StatusCode::SERVICE_UNAVAILABLE, "timeout")
        // We construct via an invalid request since reqwest's Error isn't directly constructible.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            reqwest::Client::new()
                .get("https://invalid.invalid")
                .send()
                .await
        });
        let err = match result {
            Err(e) => OctxError::Network(e),
            _ => panic!("expected network error"),
        };
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn test_error_can_be_converted_from_io_error_via_into() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no file");
        let octx_err: OctxError = io_err.into();
        assert!(matches!(octx_err, OctxError::Io(_)));
        assert_eq!(octx_err.to_string(), "I/O error: no file");
    }

    #[test]
    fn test_exit_code_for_generic_returns_1() {
        let err = OctxError::Http("bad request".into());
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn test_exit_code_for_config_returns_1() {
        let err = OctxError::Config("missing key".into());
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn test_exit_code_for_io_returns_1() {
        let err = OctxError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied",
        ));
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn test_error_can_be_converted_from_serde_error_via_into() {
        let serde_err = serde_json::from_str::<()>("not valid json").unwrap_err();
        let octx_err: OctxError = serde_err.into();
        assert!(matches!(octx_err, OctxError::Serde(_)));
    }
}
