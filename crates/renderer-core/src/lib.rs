//! Backend-independent renderer concepts. No GLES or Vulkan dependency here.

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    Auto,
    Gles,
    Vulkan,
}

impl BackendKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "gles" => Some(Self::Gles),
            "vulkan" => Some(Self::Vulkan),
            _ => None,
        }
    }
}

/// Subset of `renderer.toml` implemented so far (env overrides only; no TOML parser yet).
#[derive(Clone, Debug)]
pub struct Config {
    pub backend: BackendKind,
    pub debug: bool,
}

impl Config {
    /// Reads `RENDERER_BACKEND` and `RENDERER_DEBUG`. Unknown values fall back to defaults.
    pub fn from_env() -> Self {
        let backend = std::env::var("RENDERER_BACKEND")
            .ok()
            .and_then(|v| BackendKind::parse(&v))
            .unwrap_or(BackendKind::Auto);
        let debug = std::env::var("RENDERER_DEBUG").map(|v| v == "1").unwrap_or(false);
        Self { backend, debug }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DeviceInfo {
    pub vendor: String,
    pub renderer: String,
    pub api_version: String,
    pub glsl_version: String,
}

#[derive(Debug)]
pub enum BackendError {
    /// Not implemented / not available. Never silently succeeds.
    Unsupported(String),
    InitFailed(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(s) => write!(f, "unsupported: {s}"),
            Self::InitFailed(s) => write!(f, "init failed: {s}"),
        }
    }
}
impl std::error::Error for BackendError {}

/// Phase 1 backend interface. Grows in Phase 2 (buffers, textures, shaders, pipelines...).
pub trait Backend: Send + Sync {
    fn kind(&self) -> BackendKind;
    fn device_info(&self) -> &DeviceInfo;

    fn clear_color(&self, r: f32, g: f32, b: f32, a: f32);
    fn clear(&self, mask: u32);
    fn viewport(&self, x: i32, y: i32, w: i32, h: i32);
    fn enable(&self, cap: u32);
    fn disable(&self, cap: u32);
    /// Raw backend error (0 if none).
    fn get_error(&self) -> u32;
    /// NUL-terminated string owned by the backend/driver; null if unavailable.
    fn get_string(&self, name: u32) -> *const u8;
}

/// OpenGL-style sticky error state (spec section 7). First error wins until read.
#[derive(Default)]
pub struct GlErrorState(AtomicU32);

impl GlErrorState {
    pub fn set(&self, e: u32) {
        let _ = self.0.compare_exchange(0, e, Ordering::SeqCst, Ordering::SeqCst);
    }
    pub fn take(&self) -> u32 {
        self.0.swap(0, Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_error_wins_and_clears() {
        let s = GlErrorState::default();
        s.set(0x0500);
        s.set(0x0502);
        assert_eq!(s.take(), 0x0500);
        assert_eq!(s.take(), 0);
    }
    #[test]
    fn parses_backend() {
        assert_eq!(BackendKind::parse(" Vulkan "), Some(BackendKind::Vulkan));
        assert_eq!(BackendKind::parse("metal"), None);
    }
}
