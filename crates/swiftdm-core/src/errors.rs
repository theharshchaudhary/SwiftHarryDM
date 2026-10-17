//! Error taxonomy. Every failure carries a user message, redacted detail and whether
//! retrying can help.
use serde::{Deserialize, Serialize};
use std::io;

use crate::logging::redact;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Network,
    Timeout,
    HttpStatus,
    RateLimited,
    Forbidden,
    RangeViolation,
    ValidatorMismatch,
    DiskFull,
    PermissionDenied,
    PathInvalid,
    Tls,
    RedirectRefused,
    NotAFile,
    Extractor,
    AuthRequired,
    Unavailable,
    GeoBlocked,
    Unsupported,
    FormatGone,
    Postprocess,
    ToolMissing,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default)]
    pub http_status: Option<u16>,
    #[serde(default)]
    pub retry_after: Option<f64>,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{}", .0.message)]
pub struct SdmError(pub ErrorInfo);

impl SdmError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        SdmError(ErrorInfo {
            code,
            message: message.into(),
            detail: String::new(),
            retryable: false,
            http_status: None,
            retry_after: None,
        })
    }
    pub fn retryable(mut self) -> Self {
        self.0.retryable = true;
        self
    }
    pub fn detail(mut self, d: impl AsRef<str>) -> Self {
        let mut d = redact(d.as_ref());
        d.truncate(2000);
        self.0.detail = d;
        self
    }
    pub fn status(mut self, s: u16) -> Self {
        self.0.http_status = Some(s);
        self
    }
    pub fn retry_after(mut self, s: Option<f64>) -> Self {
        self.0.retry_after = s;
        self
    }
    pub fn code(&self) -> ErrorCode {
        self.0.code
    }
    pub fn info(&self) -> &ErrorInfo {
        &self.0
    }
}

pub fn is_disk_full(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(39) | Some(112)) || e.kind() == io::ErrorKind::StorageFull
}

/// Map filesystem errors to actionable messages. Never silently fall back to another folder.
pub fn from_io(e: &io::Error, path: &str) -> SdmError {
    let at = if path.is_empty() { String::new() } else { format!(" ({path})") };
    let detail = e.to_string();
    if is_disk_full(e) {
        return SdmError::new(ErrorCode::DiskFull, format!("The disk is full{at}. Free some space, then resume."))
            .detail(detail);
    }
    match e.raw_os_error() {
        Some(32) | Some(33) => {
            return SdmError::new(ErrorCode::PermissionDenied, format!("The file is in use by another program{at}."))
                .retryable()
                .detail(detail)
        }
        Some(5) => {
            return SdmError::new(
                ErrorCode::PermissionDenied,
                format!("Windows denied access to the download folder{at}. Choose a folder you can write to."),
            )
            .detail(detail)
        }
        Some(3) | Some(123) | Some(206) | Some(161) => {
            return SdmError::new(ErrorCode::PathInvalid, format!("The download path is invalid or no longer exists{at}."))
                .detail(detail)
        }
        _ => {}
    }
    match e.kind() {
        io::ErrorKind::PermissionDenied => SdmError::new(
            ErrorCode::PermissionDenied,
            format!("Windows denied access to the download folder{at}. Choose a folder you can write to."),
        )
        .detail(detail),
        io::ErrorKind::NotFound | io::ErrorKind::InvalidInput => {
            SdmError::new(ErrorCode::PathInvalid, format!("The download path is invalid or no longer exists{at}."))
                .detail(detail)
        }
        _ => SdmError::new(ErrorCode::Internal, format!("File system error{at}: {e}")).detail(detail),
    }
}
