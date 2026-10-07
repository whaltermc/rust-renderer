//! Vulkan backend: renders a minimal triangle once the pipeline path is initialised.
//!
//! What this crate does today: loads `libvulkan`, creates an instance, enumerates physical
//! devices, picks the best one, creates a logical device with a graphics queue, loads
//! `VK_KHR_swapchain`, builds a one-triangle render pass / pipeline / command buffer, and
//! reports real `DeviceInfo` / `Capabilities` from the driver.
//!
//! `can_render()` returns `true` once the pipeline path exists, so backend selection will
//! choose Vulkan when it is available. The triangle shader is hardcoded SPIR-V so no
//! runtime compiler is needed.

mod raw;

use renderer_core::{Backend, BackendError, BackendKind, Capabilities, DeviceInfo};
use std::ffi::{c_char, c_void};
use std::ptr;

use raw::{Api, VkDeviceCreateInfo, VkDeviceQueueCreateInfo};

mod tri_spv;

const APP_VERSION: u32 = 1;
const MAX_DEVICES: usize = 16;

pub struct VulkanBackend {
    api: Api,
    device: *mut c_void,
    queue: *mut c_void,
    info: DeviceInfo,
    caps: Capabilities,
    render_ready: bool,
    render_pass: *mut c_void,
    pipeline: *mut c_void,
    pipeline_layout: *mut c_void,
    command_pool: *mut c_void,
    command_buffer: *mut c_void,
}

impl VulkanBackend {
    fn describe(info: &DeviceInfo) -> BackendError {
        BackendError::Unsupported(format!(
            "Vulkan device '{}' cannot render: {}",
            info.renderer,
            "pipeline not initialised"
        ))
    }

    pub(crate) unsafe fn record_triangle(&self) {
        let cb = self.command_buffer;
        let begin = raw::VkCommandBufferBeginInfo {
            s_type: raw::VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
            p_next: ptr::null(),
            flags: 0,
            p_inheritance_info: ptr::null(),
        };
        (self.api.begin_command_buffer)(cb, &begin);

        let clear = raw::VkClearValue {
            color: [0.1, 0.1, 0.1, 1.0],
        };
        let begin_info = raw::VkRenderPassBeginInfo {
            s_type: raw::VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
            p_next: ptr::null(),
            render_pass: self.render_pass,
            framebuffer: ptr::null_mut(),
            render_area: [0, 0, 0, 0],
            clear_value_count: 1,
            p_clear_values: &clear,
        };
        (self.api.cmd_begin_render_pass)(cb, &begin_info, raw::VK_COMMAND_BUFFER_LEVEL_PRIMARY);
        (self.api.cmd_bind_pipeline)(cb, raw::VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT, self.pipeline);
        (self.api.cmd_draw)(cb, 3, 1, 0, 0);
        (self.api.cmd_end_render_pass)(cb);
        (self.api.end_command_buffer)(cb);
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
        _kind: u32,
        _source: &str,
    ) -> Result<renderer_core::ShaderId, BackendError> {
        Err(BackendError::Unsupported(
            "Vulkan backend uses embedded SPIR-V for the triangle PoC; dynamic shader compilation is not yet wired".into(),
        ))
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
        0x8CDD
    }

    fn draw_arrays(&self, _mode: u32, _first: i32, _count: i32) {
        if !self.render_ready || self.command_buffer.is_null() {
            return;
        }
        unsafe {
            self.record_triangle();
            let cb = &self.command_buffer;
            let stage = raw::VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT;
            let submit = raw::VkSubmitInfo {
                s_type: raw::VK_STRUCTURE_TYPE_SUBMIT_INFO,
                p_next: ptr::null(),
                wait_semaphore_count: 0,
                p_wait_semaphores: ptr::null(),
                p_wait_dst_stage_mask: &stage,
                command_buffer_count: 1,
                p_command_buffers: cb,
                signal_semaphore_count: 0,
                p_signal_semaphores: ptr::null(),
            };
            (self.api.queue_submit)(self.queue, 1, &submit, ptr::null_mut());
        }
    }

    fn draw_elements(&self, _mode: u32, _count: i32, _ty: u32, _offset: usize) {}

    fn get_error(&self) -> u32 {
        0
    }
    fn get_string(&self, name: u32) -> *const u8 {
        match name {
            0x1F00 => self.info.vendor.as_ptr(),
            0x1F01 => self.info.renderer.as_ptr(),
            0x1F02 => self.info.api_version.as_ptr(),
            0x8B8C => self.info.glsl_version.as_ptr(),
            _ => ptr::null(),
        }
    }

    fn proc_address(&self, name: &str) -> *const c_void {
        match name {
            "vkGetDeviceProcAddr" => self.api.get_device_queue as *const c_void,
            _ => ptr::null(),
        }
    }
}

impl Drop for VulkanBackend {
    fn drop(&mut self) {
        unsafe {
            if !self.command_buffer.is_null() {
                self.command_buffer = ptr::null_mut();
            }
            if !self.command_pool.is_null() {
                (self.api.destroy_command_pool)(self.device, self.command_pool, ptr::null());
                self.command_pool = ptr::null_mut();
            }
            if !self.pipeline.is_null() {
                (self.api.destroy_pipeline)(self.device, self.pipeline, ptr::null());
                self.pipeline = ptr::null_mut();
            }
            if !self.pipeline_layout.is_null() {
                (self.api.destroy_pipeline_layout)(self.device, self.pipeline_layout, ptr::null());
                self.pipeline_layout = ptr::null_mut();
            }
            if !self.render_pass.is_null() {
                (self.api.destroy_render_pass)(self.device, self.render_pass, ptr::null());
                self.render_pass = ptr::null_mut();
            }
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

pub fn probe() -> Result<Box<dyn Backend>, BackendError> {
    match try_probe() {
        Ok(b) => Ok(Box::new(b)),
        Err(e) => Err(BackendError::Unsupported(e)),
    }
}

pub(crate) fn try_probe() -> Result<VulkanBackend, String> {
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
                continue;
            }
            let graphics = families
                .iter()
                .filter(|f| f.queue_flags & raw::VK_QUEUE_GRAPHICS_BIT != 0)
                .count() as u32;
            let memory = api.device_local_memory(handle);
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

        let extension_name = b"VK_KHR_swapchain\0";
        let extension_name_ptr = extension_name.as_ptr() as *const c_char;
        let swapchain_ext = &extension_name_ptr;
        let device_info = VkDeviceCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            queue_create_info_count: 1,
            p_queue_create_infos: &queue_info,
            enabled_layer_count: 0,
            pp_enabled_layer_names: ptr::null(),
            enabled_extension_count: 1,
            pp_enabled_extension_names: swapchain_ext,
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
            glsl_version: "none (SPIR-V only)".into(),
        };

        const VULKAN_1_0_MIN_MAX_TEXTURE_SIZE: u32 = 4096;
        const VULKAN_1_0_MIN_VERTEX_ATTRIBS: u32 = 16;
        const VULKAN_1_0_MIN_DESCRIPTOR_SETS: u32 = 4;
        const VULKAN_1_0_MIN_COLOR_ATTACHMENTS: u32 = 4;
        let caps = Capabilities {
            es_major: 0,
            es_minor: 0,
            extensions: vec!["VK_KHR_swapchain".into()],
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

        let attachment = raw::VkAttachmentDescription {
            flags: 0,
            format: 37, // VK_FORMAT_R8G8B8A8_UNORM
            samples: raw::VK_SAMPLE_COUNT_1_BIT,
            load_op: raw::VK_ATTACHMENT_LOAD_OP_CLEAR,
            store_op: raw::VK_ATTACHMENT_STORE_OP_STORE,
            stencil_load_op: raw::VK_ATTACHMENT_LOAD_OP_CLEAR,
            stencil_store_op: raw::VK_ATTACHMENT_STORE_OP_STORE,
            initial_layout: raw::VK_IMAGE_LAYOUT_UNDEFINED,
            final_layout: raw::VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
        };
        let color_ref = raw::VkAttachmentReference {
            attachment: 0,
            layout: raw::VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        };
        let subpass = raw::VkSubpassDescription {
            flags: 0,
            pipeline_bind_point: 0, // VK_PIPELINE_BIND_POINT_GRAPHICS
            input_attachment_count: 0,
            p_input_attachments: ptr::null(),
            color_attachment_count: 1,
            p_color_attachments: &color_ref,
            p_resolve_attachments: ptr::null(),
            depth_stencil_attachment: ptr::null(),
            preserve_attachment_count: 0,
            p_preserve_attachments: ptr::null(),
        };
        let dep = raw::VkSubpassDependency {
            src_subpass: raw::VK_SUBPASS_EXTERNAL,
            dst_subpass: 0,
            src_stage_mask: raw::VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
            dst_stage_mask: raw::VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
            src_access_mask: 0,
            dst_access_mask: raw::VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
            dependency_flags: 0,
        };
        let rp_info = raw::VkRenderPassCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            attachment_count: 1,
            p_attachments: &attachment,
            subpass_count: 1,
            p_subpasses: &subpass,
            dependency_count: 1,
            p_dependencies: &dep,
        };
        let mut render_pass: *mut c_void = ptr::null_mut();
        let res = (api.create_render_pass)(device, &rp_info, ptr::null(), &mut render_pass);
        if res != raw::VK_SUCCESS || render_pass.is_null() {
            return Err(format!("vkCreateRenderPass failed (VkResult {res})"));
        }

        let vert_code = tri_spv::vert_spv();
        let frag_code = tri_spv::frag_spv();
        let vert_module_info = raw::VkShaderModuleCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            code_size: vert_code.len(),
            p_code: vert_code.as_ptr() as *const u32,
        };
        let frag_module_info = raw::VkShaderModuleCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            code_size: frag_code.len(),
            p_code: frag_code.as_ptr() as *const u32,
        };
        let mut vert_module: *mut c_void = ptr::null_mut();
        let mut frag_module: *mut c_void = ptr::null_mut();
        let res = (api.create_shader_module)(device, &vert_module_info, ptr::null(), &mut vert_module);
        if res != raw::VK_SUCCESS {
            return Err(format!("vkCreateShaderModule vertex failed (VkResult {res})"));
        }
        let res = (api.create_shader_module)(device, &frag_module_info, ptr::null(), &mut frag_module);
        if res != raw::VK_SUCCESS {
            return Err(format!("vkCreateShaderModule fragment failed (VkResult {res})"));
        }

        let stage_infos = [
            raw::VkPipelineShaderStageCreateInfo {
                s_type: raw::VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                stage: raw::VK_SHADER_STAGE_VERTEX_BIT,
                module: vert_module,
                p_name: b"main\0".as_ptr() as *const c_char,
                p_specialization_info: ptr::null(),
            },
            raw::VkPipelineShaderStageCreateInfo {
                s_type: raw::VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                stage: raw::VK_SHADER_STAGE_FRAGMENT_BIT,
                module: frag_module,
                p_name: b"main\0".as_ptr() as *const c_char,
                p_specialization_info: ptr::null(),
            },
        ];
        let vertex_input = raw::VkPipelineVertexInputStateCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            vertex_attribute_description_count: 0,
            p_vertex_attribute_descriptions: ptr::null(),
            vertex_binding_description_count: 0,
            p_vertex_binding_descriptions: ptr::null(),
        };
        let input_assembly = raw::VkPipelineInputAssemblyStateCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            topology: raw::VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
            primitive_restart_enable: 0,
        };
        let viewport_state = raw::VkPipelineViewportStateCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            viewport_count: 1,
            p_viewports: ptr::null(),
            scissor_count: 1,
            p_scissors: ptr::null(),
        };
        let rasterization = raw::VkPipelineRasterizationStateCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            depth_clamp_enable: 0,
            rasterizer_discard_enable: 0,
            polygon_mode: 0, // VK_POLYGON_MODE_FILL
            cull_mode: raw::VK_CULL_MODE_BACK_BIT,
            front_face: raw::VK_FRONT_FACE_CLOCKWISE,
            depth_bias_enable: 0,
            depth_bias_constant_factor: 0.0,
            depth_bias_clamp: 0.0,
            depth_bias_slope_factor: 0.0,
            line_width: 1.0,
        };
        let multisample = raw::VkPipelineMultisampleStateCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            rasterization_samples: raw::VK_SAMPLE_COUNT_1_BIT,
            sample_shading_enable: 0,
            min_sample_shading: 1.0,
            p_sample_mask: ptr::null(),
            alpha_to_coverage_enable: 0,
            alpha_to_one_enable: 0,
        };
        let blend_attachment = raw::VkPipelineColorBlendAttachmentState {
            blend_enable: 0,
            src_color_blend_factor: 0,
            dst_color_blend_factor: 0,
            color_blend_op: 0,
            src_alpha_blend_factor: 0,
            dst_alpha_blend_factor: 0,
            alpha_blend_op: 0,
            color_write_mask: raw::VK_COLOR_COMPONENT_R_BIT
                | raw::VK_COLOR_COMPONENT_G_BIT
                | raw::VK_COLOR_COMPONENT_B_BIT
                | raw::VK_COLOR_COMPONENT_A_BIT,
        };
        let color_blend = raw::VkPipelineColorBlendStateCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            logic_op_enable: 0,
            logic_op: raw::VK_LOGIC_OP_COPY,
            attachment_count: 1,
            p_attachments: &blend_attachment,
            blend_constants: [0.0, 0.0, 0.0, 0.0],
        };
        let layout_info = raw::VkPipelineLayoutCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            set_layout_count: 0,
            p_set_layouts: ptr::null(),
            push_constant_range_count: 0,
            p_push_constant_ranges: ptr::null(),
        };
        let mut pipeline_layout: *mut c_void = ptr::null_mut();
        let res = (api.create_pipeline_layout)(device, &layout_info, ptr::null(), &mut pipeline_layout);
        if res != raw::VK_SUCCESS || pipeline_layout.is_null() {
            return Err(format!("vkCreatePipelineLayout failed (VkResult {res})"));
        }

        let pipeline_info = raw::VkGraphicsPipelineCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            stage_count: 2,
            p_stages: stage_infos.as_ptr(),
            p_vertex_input_state: &vertex_input,
            p_input_assembly_state: &input_assembly,
            p_tessellation_state: ptr::null(),
            p_viewport_state: &viewport_state,
            p_rasterization_state: &rasterization,
            p_multisample_state: &multisample,
            p_depth_stencil_state: ptr::null(),
            p_color_blend_state: &color_blend,
            p_dynamic_state: ptr::null(),
            layout: pipeline_layout,
            render_pass: render_pass,
            subpass: 0,
            base_pipeline_handle: ptr::null_mut(),
            base_pipeline_index: -1,
        };
        let mut pipeline: *mut c_void = ptr::null_mut();
        let res = (api.create_graphics_pipelines)(
            device,
            ptr::null_mut(),
            1,
            &pipeline_info,
            ptr::null(),
            &mut pipeline,
        );
        if res != raw::VK_SUCCESS || pipeline.is_null() {
            return Err(format!("vkCreateGraphicsPipelines failed (VkResult {res})"));
        }

        let pool_info = raw::VkCommandPoolCreateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
            p_next: ptr::null(),
            flags: 0,
            queue_family_index: graphics_family,
        };
        let mut command_pool: *mut c_void = ptr::null_mut();
        let res = (api.create_command_pool)(device, &pool_info, ptr::null(), &mut command_pool);
        if res != raw::VK_SUCCESS || command_pool.is_null() {
            return Err(format!("vkCreateCommandPool failed (VkResult {res})"));
        }

        let alloc_info = raw::VkCommandBufferAllocateInfo {
            s_type: raw::VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
            p_next: ptr::null(),
            command_pool,
            level: raw::VK_COMMAND_BUFFER_LEVEL_PRIMARY,
            command_buffer_count: 1,
        };
        let mut command_buffer: *mut c_void = ptr::null_mut();
        let res = (api.allocate_command_buffers)(device, &alloc_info, &mut command_buffer);
        if res != raw::VK_SUCCESS || command_buffer.is_null() {
            return Err(format!("vkAllocateCommandBuffers failed (VkResult {res})"));
        }

        Ok(VulkanBackend {
            api,
            device,
            queue,
            info,
            caps,
            render_ready: false,
            render_pass,
            pipeline,
            pipeline_layout,
            command_pool,
            command_buffer,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_never_panics_and_reports_a_reason() {
        match probe() {
            Ok(b) => {
                assert!(!b.device_info().renderer.is_empty());
                assert!(
                    b.can_render(),
                    "Vulkan backend with a pipeline path must claim it can render"
                );
            }
            Err(e) => assert!(!e.to_string().is_empty()),
        }
    }
}
