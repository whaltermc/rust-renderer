//! Rendering backends.
//!
//! One module, two backends. They share the `Backend` trait from `renderer-core` and the
//! `libloading` dependency for opening the system driver, and they are the two things the
//! renderer can be built on:
//!
//! * [`gles`] — OpenGL ES 3.x on the device's own driver. This is what actually renders:
//!   the bridge translates desktop GL onto it.
//! * [`vulkan`] — Vulkan, currently discovery and device reporting only. It finds a physical
//!   device, picks the best one, creates a logical device with a graphics queue and reports
//!   real limits, but `can_render()` is `false` because there is no SPIR-V, pipeline or
//!   present path yet, so backend selection never chooses it to draw a frame.
//!
//! They were separate crates until they shared enough to be worth one module; the split was
//! costing a dependency edge and two import paths for no isolation either of them used.

/// Diagnostic output, mirroring the bridge's own logging so both appear in the same stream.
pub(crate) fn log(msg: &str) {
    eprintln!("[Backend] {msg}");
}

pub mod gles;
#[cfg(feature = "spirv")]
pub mod spirv;
pub mod vulkan;

pub use gles::GlesBackend;
pub use vulkan::{probe as probe_vulkan, VulkanBackend};
