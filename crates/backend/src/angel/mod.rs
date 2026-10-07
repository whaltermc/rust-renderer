//! ANGEL (Almost Native Graphics Layer Engine) driver support.
//!
//! ANGLE is a graphics engine that translates OpenGL ES calls to:
//! - Vulkan
//! - DirectX 11
//! - DirectX 12
//! - OpenGL (desktop)
//!
//! This module detects ANGLE, negotiates the appropriate backend,
//! and provides fallback translation when needed.

use std::ffi::{c_char, c_void, CStr};
use std::path::PathBuf;
use std::sync::OnceLock;

use renderer_core::{Backend, BackendError, BackendKind, Capabilities, DeviceInfo};

use libloading::Library;

/// ANGLE backend variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AngleBackend {
    /// Vulkan backend.
    Vulkan,
    /// DirectX 11 backend.
    D3D11,
    /// DirectX 12 backend.
    D3D12,
    /// OpenGL (desktop) backend.
    OpenGL,
    /// SwiftShader (software) backend.
    SwiftShader,
    /// Auto-detect.
    Auto,
}

impl std::fmt::Display for AngleBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vulkan => write!(f, "vulkan"),
            Self::D3D11 => write!(f, "d3d11"),
            Self::D3D12 => write!(f, "d3d12"),
            Self::OpenGL => write!(f, "opengl"),
            Self::SwiftShader => write!(f, "swiftshader"),
            Self::Auto => write!(f, "auto"),
        }
    }
}

impl AngleBackend {
    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "vulkan" | "vk" => Self::Vulkan,
            "d3d11" | "directx11" | "dx11" => Self::D3D11,
            "d3d12" | "directx12" | "dx12" => Self::D3D12,
            "opengl" | "gl" | "desktop" => Self::OpenGL,
            "swiftshader" | "sw" | "software" => Self::SwiftShader,
            _ => Self::Auto,
        }
    }
}

/// ANGLE driver configuration.
#[derive(Debug, Clone)]
pub struct AngleConfig {
    pub backend: AngleBackend,
    pub renderer: String,
    pub lib_path: PathBuf,
    pub feature_level: String,
    pub adapter_name: String,
}

impl Default for AngleConfig {
    fn default() -> Self {
        Self {
            backend: AngleBackend::Auto,
            renderer: String::new(),
            lib_path: PathBuf::new(),
            feature_level: String::new(),
            adapter_name: String::new(),
        }
    }
}

/// ANGLE driver wrapper.
///
/// Loads libANGLE and provides the EGL-like entry points that Minecraft expects.
pub struct AngleDriver {
    config: AngleConfig,
    lib: Option<libloading::Library>,
    detected: bool,
}

impl AngleDriver {
    pub fn new(config: AngleConfig) -> Self {
        Self {
            config,
            lib: None,
            detected: false,
        }
    }

    /// Probe for ANGLE on the system.
    pub fn probe(&mut self) -> bool {
        if self.detected {
            return self.lib.is_some();
        }
        self.detected = true;

        let candidates = self.candidate_paths();
        for path in &candidates {
            match unsafe { libloading::Library::new(path) } {
                Ok(lib) => {
                    self.config.lib_path = path.clone();
                    self.lib = Some(lib);
                    return true;
                }
                Err(_) => continue,
            }
        }
        false
    }

    /// Candidate library paths for ANGLE.
    fn candidate_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let backend_suffix = match self.config.backend {
            AngleBackend::Vulkan => "vk",
            AngleBackend::D3D11 => "d3d11",
            AngleBackend::D3D12 => "d3d12",
            AngleBackend::OpenGL => "gl",
            AngleBackend::SwiftShader => "sw",
            AngleBackend::Auto => "",
        };

        paths.push(PathBuf::from(format!("libEGL_angle_{backend_suffix}.so")));
        paths.push(PathBuf::from(format!("libGLESv2_angle_{backend_suffix}.so")));
        paths.push(PathBuf::from("/system/lib64/libEGL_angle.so"));
        paths.push(PathBuf::from("/system/lib64/libGLESv2_angle.so"));
        paths.push(PathBuf::from("libEGL.so"));
        paths.push(PathBuf::from("libGLESv2.so"));
        paths
    }

    pub fn is_detected(&self) -> bool {
        self.detected && self.lib.is_some()
    }

    pub fn config(&self) -> &AngleConfig {
        &self.config
    }
}

impl Drop for AngleDriver {
    fn drop(&mut self) {
        self.lib = None;
    }
}

/// Detect ANGLE from environment variables.
pub fn detect_angle_from_env() -> Option<AngleConfig> {
    let backend = std::env::var("RENDERER_ANGLE_BACKEND")
        .ok()
        .map(|s| AngleBackend::from_str(&s))
        .unwrap_or(AngleBackend::Auto);

    let renderer = std::env::var("RENDERER_ANGLE_RENDERER")
        .ok()
        .unwrap_or_default();

    let mut driver = AngleDriver::new(AngleConfig {
        backend,
        renderer,
        ..Default::default()
    });

    if driver.probe() {
        let cfg = driver.config().clone();
        Some(cfg)
    } else {
        None
    }
}

/// Check if the current driver is ANGLE.
pub fn is_angle_active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| {
        detect_angle_from_env().is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angle_backend_parsing() {
        assert_eq!(AngleBackend::from_str("vulkan"), AngleBackend::Vulkan);
        assert_eq!(AngleBackend::from_str("d3d11"), AngleBackend::D3D11);
        assert_eq!(AngleBackend::from_str("d3d12"), AngleBackend::D3D12);
        assert_eq!(AngleBackend::from_str("opengl"), AngleBackend::OpenGL);
        assert_eq!(AngleBackend::from_str("swiftshader"), AngleBackend::SwiftShader);
        assert_eq!(AngleBackend::from_str("auto"), AngleBackend::Auto);
    }
}
