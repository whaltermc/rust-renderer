//! Shader compilation and translation error types.
//!
//! Classifies errors so the pipeline can recover or fall back instead of hard-failing.

use std::fmt;

/// Shader error type.
#[derive(Debug, Clone)]
pub enum ShaderError {
    /// Shader failed to translate (unsupported feature for GLES).
    Translate(String),
    /// Shader compiled but linked with errors.
    Link(String),
    /// Backend is not supported for this shader.
    Backend(String),
    /// Timeout during translation.
    Timeout,
    /// Cache error.
    Cache(String),
    /// IO error.
    Io(String),
    /// Unknown error.
    Unknown(String),
}

impl fmt::Display for ShaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Translate(s) => write!(f, "translate: {s}"),
            Self::Link(s) => write!(f, "link: {s}"),
            Self::Backend(s) => write!(f, "backend: {s}"),
            Self::Timeout => write!(f, "translation timeout"),
            Self::Cache(s) => write!(f, "cache: {s}"),
            Self::Io(s) => write!(f, "io: {s}"),
            Self::Unknown(s) => write!(f, "unknown: {s}"),
        }
    }
}

impl std::error::Error for ShaderError {}

impl From<String> for ShaderError {
    fn from(s: String) -> Self {
        Self::Unknown(s)
    }
}

impl From<&str> for ShaderError {
    fn from(s: &str) -> Self {
        Self::Unknown(s.to_string())
    }
}

impl From<std::io::Error> for ShaderError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<BackendError> for ShaderError {
    fn from(e: BackendError) -> Self {
        Self::Backend(e.to_string())
    }
}

use renderer_core::BackendError;

/// Classify a shader compile error message into a recoverable category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorClass {
    /// Feature not supported on current backend (e.g. geometry shader on GLES).
    UnsupportedFeature,
    /// Syntax error in shader source (translation bug).
    Syntax,
    /// Internal compiler error (driver bug).
    Internal,
    /// Resource limit exceeded.
    ResourceLimit,
    /// Unknown/unclassified error.
    Unknown,
}

pub fn classify_error(message: &str) -> ErrorClass {
    let lower = message.to_ascii_lowercase();
    if lower.contains("not supported")
        || lower.contains("unsupported")
        || lower.contains("no such extension")
        || lower.contains("invalid operation")
    {
        ErrorClass::UnsupportedFeature
    } else if lower.contains("error")
        || lower.contains("syntax")
        || lower.contains("invalid token")
        || lower.contains("undeclared")
    {
        ErrorClass::Syntax
    } else if lower.contains("internal")
        || lower.contains("segmentation")
        || lower.contains("abort")
    {
        ErrorClass::Internal
    } else if lower.contains("limit")
        || lower.contains("exceeded")
        || lower.contains("out of memory")
    {
        ErrorClass::ResourceLimit
    } else {
        ErrorClass::Unknown
    }
}
