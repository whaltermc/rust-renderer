//! Vulkan backend: **device discovery and reporting only**.
//!
//! What this crate does today: loads `libvulkan`, creates an instance, enumerates physical
//! devices, picks the best one by device class / queue families / memory, creates a logical
//! device with a graphics queue, and reports real `DeviceInfo` / `Capabilities` from the
//! driver.
//!
//! What it does *not* do: render. There is no SPIR-V compilation, pipeline creation,
//! descriptor-set management, command recording or swapchain presentation here, so
//! [`VulkanBackend::can_render`] returns `false` and backend selection falls back to GLES
//! rather than reporting a renderer that cannot draw a triangle.
//!
//! The distinction matters for the game's own drawing: Minecraft loads `librust_gl.so` and
//! speaks desktop GL 3.3, and every one of those entry points is served by the GLES driver
//! passthrough in `gl-compat`. A Vulkan backend becomes the renderer only when the game
//! itself uses Vulkan, or when this renderer owns the drawing code (fixed-function
//! emulation) and compiles for Vulkan.

mod raw;

use renderer_core::{Backend, BackendError, BackendKind, Capabilities, DeviceInfo};
use std::ffi::c_void;
use std::ptr;

use raw::{Api, VkDeviceCreateInfo, VkDeviceQueueCreateInfo};

const APP_VERSION: u32 = 1;

/// Startup cap on the physical devices we consider.
const MAX_DEVICES: usize = 16;

pub struct VulkanBackend {
    api: Api,
    device: *mut c_void,
    /// Graphics queue from the selected family, captured for future command submission.
    #[allow(dead_code)]
    queue: *mut c_void,
    info: DeviceInfo,
    caps: Capabilities,
    /// Set once a path that can actually submit work exists. Not yet.
    render_ready: bool,
}

impl VulkanBackend {
    fn describe(info: &DeviceInfo) -> BackendError {
        BackendError::Unsupported(format!(
            "Vulkan device '{}' was initialized but cannot render yet \
             (no SPIR-V/pipeline/present path); the GL surface is still served by GLES",
            info.renderer
        ))
    }
}

impl Backend for VulkanBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Vulkan
    }

    fn device_info(&self) -> &DeviceInfo {
        &self.info
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn can_render(&self) -> bool {
        self.render_ready
    }

    // Every resource operation needs pipeline and command-buffer machinery that does not
    // exist yet. Reporting a precise Unsupported beats a GL error from a driver that was
    // never given the objects these calls assume.

    fn clear_color(&self, _r: f32, _g: f32, _b: f32, _a: f32) {}
    fn clear(&self, _mask: u32) {}
    fn viewport(&self, _x: i32, _y: i32, _w: i32, _h: i32) {}
    fn scissor(&self, _x: i32, _y: i32, _w: i32, _h: i32) {}
    fn enable(&self, _cap: u32) {}
    fn disable(&self, _cap: u32) {}
    fn blend_func(&self, _src: u32, _dst: u32) {}
    fn depth_func(&self, _func: u32) {}
    fn depth_mask(&self, _enabled: bool) {}
    fn cull_face(&self, _mode: u32) {}

    fn create_buffer(&self) -> Result<renderer_core::BufferId, BackendError> {
        Err(Self::describe(&self.info))
    }
    fn delete_buffer(&self, _id: renderer_core::BufferId) {}
    fn bind_buffer(&self, _target: u32, _id: Option<renderer_core::BufferId>) {}
    fn buffer_data(
        &self,
        _target: u32,
        _data: &[u8],
        _usage: u32,
    ) -> Result<(), BackendError> {
        Err(Self::describe(&self.info))
    }
    fn buffer_sub_data(
        &self,
        _target: u32,
        _offset: usize,
        _data: &[u8],
    ) -> Result<(), BackendError> {
        Err(Self::describe(&self.info))
    }

    fn create_texture(&self) -> Result<renderer_core::TextureId, BackendError> {
        Err(Self::describe(&self.info))
    }
    fn delete_texture(&self, _id: renderer_core::TextureId) {}
    fn active_texture(&self, _unit: u32) {}
    fn bind_texture(&self, _target: u32, _id: Option<renderer_core::TextureId>) {}
    #[allow(clippy::too_many_arguments)]
    fn tex_image_2d(
        &self,
        _target: u32,
        _level: i32,
        _internal_format: i32,
        _width: i32,
        _height: i32,
        _format: u32,
        _ty: u32,
        _data: Option<&[u8]>,
    ) -> Result<(), BackendError> {
        Err(Self::describe(&self.info))
    }
    fn tex_parameter_i(&self, _target: u32, _pname: u32, _value: i32) {}

    fn compile_shader(
        &self,
        kind: u32,
        source: &str,
    ) -> Result<renderer_core::ShaderId, BackendError> {
        #[cfg(not(feature = "spirv"))]
        {
            Err(BackendError::Unsupported(
                "Vulkan needs GLSL compiled to SPIR-V; build with the `spirv` feature to \
                 enable the compiler"
                    .into(),
            ))
        }
        #[cfg(feature = "spirv")]
        {
            let stage = crate::spirv::stage_for_gl_enum(kind)
                .map_err(BackendError::Unsupported)?;
            match crate::spirv::compile(source, stage, &[]) {
                Ok(words) => {
                    // Words in hand, but nothing consumes them yet: there is no
                    // VkShaderModule, pipeline layout or submit path. Handing back an id
                    // would pretend otherwise, so this stays unsupported -- and says why.
                    crate::log(&format!(
                        "[Vulkan] compiled {} SPIR-V words, but cannot create a pipeline yet",
                        words.len()
                    ));
                    Err(BackendError::Unsupported(
                        "Vulkan can compile SPIR-V but has no pipeline path yet".into(),
                    ))
                }
                Err(e) => Err(BackendError::Unsupported(e)),
            }
        }
    }
    fn delete_shader(&self, _id: renderer_core::ShaderId) {}
    fn link_program(
        &self,
        _shaders: &[renderer_core::ShaderId],
    ) -> Result<renderer_core::ProgramId, BackendError> {
        Err(Self::describe(&self.info))
    }
    fn delete_program(&self, _id: renderer_core::ProgramId) {}
    fn use_program(&self, _id: Option<renderer_core::ProgramId>) {}
    fn uniform_location(&self, _program: renderer_core::ProgramId, _name: &str) -> Option<i32> {
        None
    }
    fn uniform_1i(&self, _location: i32, _v: i32) {}
    fn uniform_1f(&self, _location: i32, _v: f32) {}
    fn uniform_4f(&self, _location: i32, _x: f32, _y: f32, _z: f32, _w: f32) {}
    fn uniform_matrix_4(&self, _location: i32, _m: &[f32; 16], _transpose: bool) {}

    fn create_vertex_array(&self) -> Result<renderer_core::VertexArrayId, BackendError> {
        Err(Self::describe(&self.info))
    }
    fn delete_vertex_array(&self, _id: renderer_core::VertexArrayId) {}
    fn bind_vertex_array(&self, _id: Option<renderer_core::VertexArrayId>) {}
    fn vertex_attrib_pointer(
        &self,
        _index: u32,
        _size: i32,
        _ty: u32,
        _normalized: bool,
        _stride: i32,
        _offset: usize,
    ) {
    }
    fn set_vertex_attrib_enabled(&self, _index: u32, _enabled: bool) {}

    fn create_framebuffer(&self) -> Result<renderer_core::FramebufferId, BackendError> {
        Err(Self::describe(&self.info))
    }
    fn delete_framebuffer(&self, _id: renderer_core::FramebufferId) {}
    fn bind_framebuffer(&self, _target: u32, _id: Option<renderer_core::FramebufferId>) {}
    fn framebuffer_texture_2d(
        &self,
        _target: u32,
        _attachment: u32,
        _tex_target: u32,
        _tex: renderer_core::TextureId,
        _level: i32,
    ) {
    }
    fn check_framebuffer_status(&self, _target: u32) -> u32 {
        0x8CDD // GL_FRAMEBUFFER_UNSUPPORTED — honest: nothing to check
    }

    fn draw_arrays(&self, _mode: u32, _first: i32, _count: i32) {}
    fn draw_elements(&self, _mode: u32, _count: i32, _ty: u32, _offset: usize) {}

    fn get_error(&self) -> u32 {
        0
    }
    fn get_string(&self, name: u32) -> *const u8 {
        match name {
            0x1F00 => self.info.vendor.as_ptr(),          // GL_VENDOR
            0x1F01 => self.info.renderer.as_ptr(),        // GL_RENDERER
            0x1F02 => self.info.api_version.as_ptr(),    // GL_VERSION
            0x8B8C => self.info.glsl_version.as_ptr(),   // GL_SHADING_LANGUAGE_VERSION
            _ => ptr::null(),
        }
    }

    /// Vulkan entry points resolved through the loader, for code that needs them directly.
    fn proc_address(&self, name: &str) -> *const c_void {
        match name {
            "vkGetDeviceProcAddr" => self.api.get_device_queue as *const c_void,
            _ => ptr::null(),
        }
    }
}

impl Drop for VulkanBackend {
    fn drop(&mut self) {
        // SAFETY: both handles come from this backend and are destroyed exactly once.
        unsafe {
            if !self.device.is_null() {
                (self.api.destroy_device)(self.device, ptr::null());
                self.device = ptr::null_mut();
            }
            if !self.api.instance.is_null() {
                (self.api.destroy_instance)(self.api.instance, ptr::null());
                self.api.instance = ptr::null_mut();
            }
        }
    }
}

unsafe impl Send for VulkanBackend {}
unsafe impl Sync for VulkanBackend {}

/// Opens the Vulkan loader, picks the best physical device, and creates a logical device with
/// one graphics queue.
///
/// Returns `Err` when there is no loader, no physical device, or no graphics-capable queue,
/// so `RENDERER_BACKEND=auto` can fall back to GLES. A successful result still reports
/// `can_render() == false`: this is device discovery, not a renderer.
pub fn probe() -> Result<Box<dyn Backend>, BackendError> {
    match try_probe() {
        Ok(b) => Ok(Box::new(b)),
        Err(e) => Err(BackendError::Unsupported(e)),
    }
}

fn try_probe() -> Result<VulkanBackend, String> {
    // SAFETY: raw::Api documents the contracts of every FFI call it makes; all pointers
    // come from Vulkan itself and stay owned by the returned backend.
    unsafe {
        let api = Api::new("rust-renderer", APP_VERSION).map_err(|e| format!("{e}"))?;

        let handles = api.physical_devices();
        if handles.is_empty() {
            return Err("Vulkan loader present but no physical device".into());
        }

        let mut best: Option<(u32, u64, *mut c_void, raw::Properties, Vec<raw::VkQueueFamilyProperties>)> = None;
        for handle in handles.iter().take(MAX_DEVICES).copied() {
            let props = api.properties(handle);
            let families = api.queue_families(handle);
            if !families
                .iter()
                .any(|f| f.queue_flags & raw::VK_QUEUE_GRAPHICS_BIT != 0)
            {
                continue; // no graphics queue: unusable for a renderer
            }
            let graphics = families
                .iter()
                .filter(|f| f.queue_flags & raw::VK_QUEUE_GRAPHICS_BIT != 0)
                .count() as u32;
            let memory = api.device_local_memory(handle);
            // Class dominates, then queue count, then memory.
            let score = raw::device_type_rank(props.device_type) * 1_000_000 + graphics * 1_000;
            let better = match &best {
                None => true,
                Some((best_score, best_mem, _, _, _)) => {
                    score > *best_score || (score == *best_score && memory > *best_mem)
                }
            };
            if better {
                best = Some((score, memory, handle, props, families));
            }
        }

        let Some((_, memory, handle, props, families)) = best else {
            return Err("no Vulkan device exposes a graphics-capable queue family".into());
        };

        let graphics_family = families
            .iter()
            .position(|f| f.queue_flags & raw::VK_QUEUE_GRAPHICS_BIT != 0)
            .expect("checked above") as u32;
        let priority = [1.0f32];
        let queue_info = VkDeviceQueueCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            queue_family_index: graphics_family,
            queue_count: 1,
            p_queue_priorities: priority.as_ptr(),
        };
        let device_info = VkDeviceCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            queue_create_info_count: 1,
            p_queue_create_infos: &queue_info,
            enabled_layer_count: 0,
            pp_enabled_layer_names: ptr::null(),
            enabled_extension_count: 0,
            pp_enabled_extension_names: ptr::null(),
            p_enabled_features: ptr::null(),
        };
        let mut device: *mut c_void = ptr::null_mut();
        let res = (api.create_device)(handle, &device_info, ptr::null(), &mut device);
        if res != raw::VK_SUCCESS || device.is_null() {
            return Err(format!("vkCreateDevice failed (VkResult {res})"));
        }
        let mut queue: *mut c_void = ptr::null_mut();
        (api.get_device_queue)(device, graphics_family, 0, &mut queue);

        let api_str = format!(
            "Vulkan {} (driver {}.{}.{}, max 3D {}x{}x{}, {} array layers{})",
            raw::api_version_string(props.api_version),
            props.driver_version >> 22,
            (props.driver_version >> 12) & 0x3FF,
            props.driver_version & 0xFFF,
            props.max_image_dimension_2d,
            props.max_image_dimension_3d,
            props.max_image_dimension_3d,
            props.max_image_array_layers,
            if memory > 0 {
                format!(", {} MiB device-local", memory / (1024 * 1024))
            } else {
                String::new()
            }
        );
        let info = DeviceInfo {
            vendor: raw::vendor_name(props.vendor_id),
            renderer: format!(
                "{} ({}, id 0x{:04X}:0x{:04X})",
                props.name,
                raw::device_type_name(props.device_type),
                props.vendor_id,
                props.device_id
            ),
            api_version: api_str,
            // Vulkan has no GLSL; shaders are SPIR-V. Saying so beats inventing a number.
            glsl_version: "none (SPIR-V only)".into(),
        };

        // Only maxImageDimension2D is read out of the driver's limits, because the other
        // offsets this crate would need sit deep inside VkPhysicalDeviceLimits and are not
        // worth hardcoding for a backend that cannot draw. The rest are Vulkan 1.0 *required
        // minimums*, which are true lower bounds rather than invented measurements.
        const VULKAN_1_0_MIN_MAX_TEXTURE_SIZE: u32 = 4096;
        const VULKAN_1_0_MIN_VERTEX_ATTRIBS: u32 = 16;
        const VULKAN_1_0_MIN_DESCRIPTOR_SETS: u32 = 4;
        const VULKAN_1_0_MIN_COLOR_ATTACHMENTS: u32 = 4;
        let caps = Capabilities {
            es_major: 0,
            es_minor: 0,
            extensions: Vec::new(),
            max_texture_size: props
                .max_image_dimension_2d
                .max(VULKAN_1_0_MIN_MAX_TEXTURE_SIZE) as i32,
            max_vertex_attribs: VULKAN_1_0_MIN_VERTEX_ATTRIBS as i32,
            max_draw_buffers: VULKAN_1_0_MIN_COLOR_ATTACHMENTS as i32,
            max_color_attachments: VULKAN_1_0_MIN_COLOR_ATTACHMENTS as i32,
            max_texture_units: VULKAN_1_0_MIN_DESCRIPTOR_SETS as i32,
            max_uniform_block_size: 16_384,
            max_samples: 4,
        };

        Ok(VulkanBackend {
            api,
            device,
            queue,
            info,
            caps,
            render_ready: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_never_panics_and_reports_a_reason() {
        // Either a device is found (and must honestly report that it cannot render), or a
        // message explains why not. Both are acceptable; a panic is not.
        match probe() {
            Ok(b) => {
                assert!(!b.device_info().renderer.is_empty());
                assert!(
                    !b.can_render(),
                    "a Vulkan backend with no pipeline path must not claim it can render"
                );
            }
            Err(e) => assert!(!e.to_string().is_empty()),
        }
    }
}
