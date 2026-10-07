//! Rendering backends.
//!
//! One module, multiple backends. They share the `Backend` trait from `renderer-core` and the
//! `libloading` dependency for opening the system driver, and they are the things the
//! renderer can be built on:
//!
//! * [`gles`] — OpenGL ES 3.x on the device's own driver. This is what actually renders:
//!   the bridge translates desktop GL onto it.
//! * [`directes`] — Direct OpenGL ES 3.x backend (bypasses GL compatibility layer).
//! * [`vulkan`] — Vulkan, currently discovery and device reporting only.
//! * [`directvk`] — Direct Vulkan rendering backend with pipeline/present path.
//! * [`angel`] — ANGLE (Almost Native Graphics Layer Engine) driver support.

/// Diagnostic output, mirroring the bridge's own logging so both appear in the same stream.
pub(crate) fn log(msg: &str) {
    eprintln!("[Backend] {msg}");
}

pub mod angel;
pub mod directes;
pub mod directvk;
pub mod gles;
#[cfg(feature = "spirv")]
pub mod spirv;
pub mod vulkan;

pub use angel::{detect_angle_from_env, is_angle_active, AngleBackend, AngleConfig, AngleDriver};
pub use directes::probe as probe_directes;
pub use directvk::probe as probe_directvk;
pub use gles::GlesBackend;
pub use vulkan::{probe as probe_vulkan, VulkanBackend};
