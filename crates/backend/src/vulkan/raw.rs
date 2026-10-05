//! Minimal libvulkan FFI: enough to create an instance, pick a physical device, and read
//! its identity and limits.
//!
//! Scope is deliberately narrow. This covers device discovery and reporting, not
//! rendering: there are no wrappers here for pipelines, descriptor sets, command buffers or
//! swapchains, because nothing uses them yet (see the crate docs).

use std::ffi::{c_char, c_void, CString};
use std::ptr;

pub const VK_SUCCESS: i32 = 0;
pub const VK_INCOMPLETE: i32 = 5;

pub const VK_STRUCTURE_TYPE_APPLICATION_INFO: u32 = 0;
pub const VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO: u32 = 1;

pub const VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU: u32 = 1;
pub const VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU: u32 = 2;
pub const VK_PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU: u32 = 3;
pub const VK_PHYSICAL_DEVICE_TYPE_CPU: u32 = 4;

pub const VK_QUEUE_GRAPHICS_BIT: u32 = 0x1;

pub const VK_ATTACHMENT_LOAD_OP_CLEAR: u32 = 0;
pub const VK_ATTACHMENT_STORE_OP_STORE: u32 = 0;
pub const VK_IMAGE_LAYOUT_UNDEFINED: u32 = 0;
pub const VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL: u32 = 2;
pub const VK_IMAGE_LAYOUT_PRESENT_SRC_KHR: u32 = 1000001002;
pub const VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT: u32 = 0x10;
pub const VK_SHADER_STAGE_VERTEX_BIT: u32 = 0x1;
pub const VK_SHADER_STAGE_FRAGMENT_BIT: u32 = 0x10;
pub const VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST: u32 = 3;
pub const VK_CULL_MODE_BACK_BIT: u32 = 0x2;
pub const VK_FRONT_FACE_CLOCKWISE: u32 = 0;
pub const VK_SAMPLE_COUNT_1_BIT: u32 = 1;
pub const VK_LOGIC_OP_COPY: u32 = 0x100;
pub const VK_COLOR_COMPONENT_R_BIT: u32 = 0x1;
pub const VK_COLOR_COMPONENT_G_BIT: u32 = 0x2;
pub const VK_COLOR_COMPONENT_B_BIT: u32 = 0x4;
pub const VK_COLOR_COMPONENT_A_BIT: u32 = 0x8;
pub const VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT: u32 = 0x800;
pub const VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT: u32 = 0x100;
pub const VK_COMMAND_BUFFER_LEVEL_PRIMARY: u32 = 0;
pub const VK_DYNAMIC_STATE_VIEWPORT: u32 = 100;
pub const VK_DYNAMIC_STATE_SCISSOR: u32 = 101;
pub const VK_SUBPASS_EXTERNAL: u32 = 0xFFFFFFFF;

pub const VENDOR_NAMES: &[(u32, &str)] = &[
    (0x1002, "AMD"),
    (0x1010, "ImgTec"),
    (0x10DE, "NVIDIA"),
    (0x13B5, "ARM"),
    (0x5143, "Qualcomm"),
    (0x8086, "Intel"),
    (0x106B, "Apple"),
];

pub fn vendor_name(id: u32) -> String {
    VENDOR_NAMES
        .iter()
        .find(|(v, _)| *v == id)
        .map(|(_, n)| (*n).to_string())
        .unwrap_or_else(|| format!("vendor 0x{id:04X}"))
}

pub fn device_type_name(t: u32) -> &'static str {
    match t {
        VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU => "discrete",
        VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU => "integrated",
        VK_PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU => "virtual",
        VK_PHYSICAL_DEVICE_TYPE_CPU => "cpu",
        _ => "other",
    }
}

/// Rank used to pick between several physical devices; higher wins.
pub fn device_type_rank(t: u32) -> u32 {
    match t {
        VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU => 3,
        VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU => 2,
        VK_PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU => 1,
        VK_PHYSICAL_DEVICE_TYPE_CPU => 0,
        _ => 0,
    }
}

pub fn api_version_string(v: u32) -> String {
    let major = v >> 22;
    let minor = (v >> 12) & 0x3FF;
    let patch = v & 0xFFF;
    format!("{major}.{minor}.{patch}")
}

pub fn make_api_version(major: u32, minor: u32) -> u32 {
    (major << 22) | (minor << 12)
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkApplicationInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub p_application_name: *const c_char,
    pub application_version: u32,
    pub p_engine_name: *const c_char,
    pub engine_version: u32,
    pub api_version: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkInstanceCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub p_application_info: *const VkApplicationInfo,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VkQueueFamilyProperties {
    pub queue_flags: u32,
    pub queue_count: u32,
    pub timestamp_valid_bits: u32,
    pub min_image_transfer_granularity: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkDeviceQueueCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub queue_family_index: u32,
    pub queue_count: u32,
    pub p_queue_priorities: *const f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkDeviceCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub queue_create_info_count: u32,
    pub p_queue_create_infos: *const VkDeviceQueueCreateInfo,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
    pub p_enabled_features: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkAttachmentDescription {
    pub flags: u32,
    pub format: u32,
    pub samples: u32,
    pub load_op: u32,
    pub store_op: u32,
    pub stencil_load_op: u32,
    pub stencil_store_op: u32,
    pub initial_layout: u32,
    pub final_layout: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkAttachmentReference {
    pub attachment: u32,
    pub layout: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkSubpassDescription {
    pub flags: u32,
    pub pipeline_bind_point: u32,
    pub input_attachment_count: u32,
    pub p_input_attachments: *const VkAttachmentReference,
    pub color_attachment_count: u32,
    pub p_color_attachments: *const VkAttachmentReference,
    pub p_resolve_attachments: *const VkAttachmentReference,
    pub depth_stencil_attachment: *const VkAttachmentReference,
    pub preserve_attachment_count: u32,
    pub p_preserve_attachments: *const u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkSubpassDependency {
    pub src_subpass: u32,
    pub dst_subpass: u32,
    pub src_stage_mask: u32,
    pub dst_stage_mask: u32,
    pub src_access_mask: u32,
    pub dst_access_mask: u32,
    pub dependency_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkRenderPassCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub attachment_count: u32,
    pub p_attachments: *const VkAttachmentDescription,
    pub subpass_count: u32,
    pub p_subpasses: *const VkSubpassDescription,
    pub dependency_count: u32,
    pub p_dependencies: *const VkSubpassDependency,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPipelineShaderStageCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub stage: u32,
    pub module: *mut c_void,
    pub p_name: *const c_char,
    pub p_specialization_info: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPipelineVertexInputStateCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub vertex_attribute_description_count: u32,
    pub p_vertex_attribute_descriptions: *const c_void,
    pub vertex_binding_description_count: u32,
    pub p_vertex_binding_descriptions: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPipelineInputAssemblyStateCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub topology: u32,
    pub primitive_restart_enable: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPipelineViewportStateCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub viewport_count: u32,
    pub p_viewports: *const c_void,
    pub scissor_count: u32,
    pub p_scissors: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPipelineRasterizationStateCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub depth_clamp_enable: u32,
    pub rasterizer_discard_enable: u32,
    pub polygon_mode: u32,
    pub cull_mode: u32,
    pub front_face: u32,
    pub depth_bias_enable: u32,
    pub depth_bias_constant_factor: f32,
    pub depth_bias_clamp: f32,
    pub depth_bias_slope_factor: f32,
    pub line_width: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPipelineMultisampleStateCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub rasterization_samples: u32,
    pub sample_shading_enable: u32,
    pub min_sample_shading: f32,
    pub p_sample_mask: *const u32,
    pub alpha_to_coverage_enable: u32,
    pub alpha_to_one_enable: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPipelineColorBlendAttachmentState {
    pub blend_enable: u32,
    pub src_color_blend_factor: u32,
    pub dst_color_blend_factor: u32,
    pub color_blend_op: u32,
    pub src_alpha_blend_factor: u32,
    pub dst_alpha_blend_factor: u32,
    pub alpha_blend_op: u32,
    pub color_write_mask: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPipelineColorBlendStateCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub logic_op_enable: u32,
    pub logic_op: u32,
    pub attachment_count: u32,
    pub p_attachments: *const VkPipelineColorBlendAttachmentState,
    pub blend_constants: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkGraphicsPipelineCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub stage_count: u32,
    pub p_stages: *const VkPipelineShaderStageCreateInfo,
    pub p_vertex_input_state: *const VkPipelineVertexInputStateCreateInfo,
    pub p_input_assembly_state: *const VkPipelineInputAssemblyStateCreateInfo,
    pub p_tessellation_state: *const c_void,
    pub p_viewport_state: *const VkPipelineViewportStateCreateInfo,
    pub p_rasterization_state: *const VkPipelineRasterizationStateCreateInfo,
    pub p_multisample_state: *const VkPipelineMultisampleStateCreateInfo,
    pub p_depth_stencil_state: *const c_void,
    pub p_color_blend_state: *const VkPipelineColorBlendStateCreateInfo,
    pub p_dynamic_state: *const c_void,
    pub layout: *mut c_void,
    pub render_pass: *mut c_void,
    pub subpass: u32,
    pub base_pipeline_handle: *mut c_void,
    pub base_pipeline_index: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkFramebufferCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub render_pass: *mut c_void,
    pub attachment_count: u32,
    pub p_attachments: *const *mut c_void,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkCommandPoolCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub queue_family_index: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkCommandBufferAllocateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub command_pool: *mut c_void,
    pub level: u32,
    pub command_buffer_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkCommandBufferBeginInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub p_inheritance_info: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkClearValue {
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkRenderPassBeginInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub render_pass: *mut c_void,
    pub framebuffer: *mut c_void,
    pub render_area: [u32; 4],
    pub clear_value_count: u32,
    pub p_clear_values: *const VkClearValue,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkSubmitInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub wait_semaphore_count: u32,
    pub p_wait_semaphores: *const *mut c_void,
    pub p_wait_dst_stage_mask: *const u32,
    pub command_buffer_count: u32,
    pub p_command_buffers: *const *mut c_void,
    pub signal_semaphore_count: u32,
    pub p_signal_semaphores: *const *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPresentInfoKHR {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub wait_semaphore_count: u32,
    pub p_wait_semaphores: *const *mut c_void,
    pub swapchain_count: u32,
    pub p_swapchains: *const *mut c_void,
    pub p_image_indices: *const u32,
    pub p_results: *mut i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkSwapchainCreateInfoKHR {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub surface: *mut c_void,
    pub min_image_count: u32,
    pub image_format: u32,
    pub image_color_space: u32,
    pub image_extent: [u32; 2],
    pub image_array_layers: u32,
    pub image_usage: u32,
    pub image_sharing_mode: u32,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
    pub pre_transform: u32,
    pub composite_alpha: u32,
    pub present_mode: u32,
    pub clipped: u32,
    pub old_swapchain: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkShaderModuleCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub code_size: usize,
    pub p_code: *const u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPipelineLayoutCreateInfo {
    pub s_type: u32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub set_layout_count: u32,
    pub p_set_layouts: *const *mut c_void,
    pub push_constant_range_count: u32,
    pub p_push_constant_ranges: *const c_void,
}

pub const VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO: u32 = 9;
pub const VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO: u32 = 10;

pub const VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO: u32 = 46;
pub const VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO: u32 = 51;
pub const VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO: u32 = 35;
pub const VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO: u32 = 21;
pub const VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO: u32 = 22;
pub const VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO: u32 = 23;
pub const VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO: u32 = 25;
pub const VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO: u32 = 28;
pub const VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO: u32 = 29;
pub const VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO: u32 = 30;
pub const VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO: u32 = 32;
pub const VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO: u32 = 40;
pub const VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO: u32 = 33;
pub const VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO: u32 = 34;
pub const VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO: u32 = 47;
pub const VK_STRUCTURE_TYPE_SUBMIT_INFO: u32 = 36;
pub const VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO: u32 = 33;

pub type PFN = unsafe extern "C" fn();

pub type PfnGetInstanceProcAddr =
    unsafe extern "C" fn(*mut c_void, *const c_char) -> Option<PFN>;
pub type PfnCreateInstance =
    unsafe extern "C" fn(*const VkInstanceCreateInfo, *const c_void, *mut *mut c_void) -> i32;
pub type PfnDestroyInstance = unsafe extern "C" fn(*mut c_void, *const c_void);
pub type PfnEnumerateInstanceVersion = unsafe extern "C" fn(*mut u32) -> i32;
pub type PfnEnumeratePhysicalDevices =
    unsafe extern "C" fn(*mut c_void, *mut u32, *mut *mut c_void) -> i32;
pub type PfnGetPhysicalDeviceProperties =
    unsafe extern "C" fn(*mut c_void, *mut c_void);
pub type PfnGetPhysicalDeviceQueueFamilyProperties =
    unsafe extern "C" fn(*mut c_void, *mut u32, *mut VkQueueFamilyProperties);
pub type PfnGetPhysicalDeviceMemoryProperties =
    unsafe extern "C" fn(*mut c_void, *mut c_void);
pub type PfnCreateDevice = unsafe extern "C" fn(
    *mut c_void,
    *const VkDeviceCreateInfo,
    *const c_void,
    *mut *mut c_void,
) -> i32;
pub type PfnDestroyDevice = unsafe extern "C" fn(*mut c_void, *const c_void);
pub type PfnGetDeviceQueue =
    unsafe extern "C" fn(*mut c_void, u32, u32, *mut *mut c_void);

pub type PfnCreateSwapchainKHR = unsafe extern "C" fn(
    *mut c_void,
    *const VkSwapchainCreateInfoKHR,
    *const c_void,
    *mut *mut c_void,
) -> i32;
pub type PfnDestroySwapchainKHR =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void);
pub type PfnGetSwapchainImagesKHR = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut u32,
    *mut *mut c_void,
) -> i32;
pub type PfnAcquireNextImageKHR = unsafe extern "C" fn(
    *mut c_void,
    u64,
    *mut c_void,
    *mut c_void,
    *mut u32,
) -> i32;
pub type PfnQueuePresentKHR =
    unsafe extern "C" fn(*mut c_void, *const VkPresentInfoKHR) -> i32;
pub type PfnCreateRenderPass = unsafe extern "C" fn(
    *mut c_void,
    *const VkRenderPassCreateInfo,
    *const c_void,
    *mut *mut c_void,
) -> i32;
pub type PfnCreateGraphicsPipelines = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    u32,
    *const VkGraphicsPipelineCreateInfo,
    *const c_void,
    *mut *mut c_void,
) -> i32;
pub type PfnCreateFramebuffer = unsafe extern "C" fn(
    *mut c_void,
    *const VkFramebufferCreateInfo,
    *const c_void,
    *mut *mut c_void,
) -> i32;
pub type PfnCreateCommandPool = unsafe extern "C" fn(
    *mut c_void,
    *const VkCommandPoolCreateInfo,
    *const c_void,
    *mut *mut c_void,
) -> i32;
pub type PfnAllocateCommandBuffers = unsafe extern "C" fn(
    *mut c_void,
    *const VkCommandBufferAllocateInfo,
    *mut *mut c_void,
) -> i32;
pub type PfnBeginCommandBuffer =
    unsafe extern "C" fn(*mut c_void, *const VkCommandBufferBeginInfo) -> i32;
pub type PfnCmdBeginRenderPass =
    unsafe extern "C" fn(*mut c_void, *const VkRenderPassBeginInfo, u32);
pub type PfnCmdBindPipeline =
    unsafe extern "C" fn(*mut c_void, u32, *mut c_void);
pub type PfnCmdDraw =
    unsafe extern "C" fn(*mut c_void, u32, u32, u32, u32);
pub type PfnCmdEndRenderPass =
    unsafe extern "C" fn(*mut c_void);
pub type PfnEndCommandBuffer = unsafe extern "C" fn(*mut c_void) -> i32;
pub type PfnQueueSubmit = unsafe extern "C" fn(
    *mut c_void,
    u32,
    *const VkSubmitInfo,
    *mut c_void,
) -> i32;
pub type PfnCreateShaderModule = unsafe extern "C" fn(
    *mut c_void,
    *const VkShaderModuleCreateInfo,
    *const c_void,
    *mut *mut c_void,
) -> i32;
pub type PfnDestroyShaderModule =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void);
pub type PfnDestroyRenderPass =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void);
pub type PfnDestroyPipeline =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void);
pub type PfnDestroyCommandPool =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void);
pub type PfnDestroyFramebuffer =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void);
pub type PfnCreatePipelineLayout = unsafe extern "C" fn(
    *mut c_void,
    *const VkPipelineLayoutCreateInfo,
    *const c_void,
    *mut *mut c_void,
) -> i32;
pub type PfnDestroyPipelineLayout =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void);

/// Byte offsets inside `VkPhysicalDeviceProperties` (Vulkan 1.0, 64-bit):
/// `apiVersion` 0, `driverVersion` 4, `vendorID` 8, `deviceID` 12, `deviceType` 16,
/// `deviceName[256]` 20..276, `pipelineCacheUUID[16]` 276..292, `limits` from 292.
/// Expressed in u32 words, `limits` therefore starts at 73, so `maxImageDimension2D`
/// (the second u32 in `Limits`) is word 74.
const LIMITS_START_U32: usize = 73;
const LIMITS_INDEX_2D: usize = LIMITS_START_U32 + 1;
const LIMITS_INDEX_3D: usize = LIMITS_START_U32 + 2;
const LIMITS_INDEX_ARRAY_LAYERS: usize = LIMITS_START_U32 + 4;
/// `vkGetPhysicalDeviceProperties` writes the WHOLE `VkPhysicalDeviceProperties` (824 bytes
/// on 64-bit: the `Limits` block alone is 504), no matter how much of it we decode. The
/// buffer must therefore be at least that large. It used to be 320 bytes, so every call
/// overran the stack by ~500 bytes -- that was the `hybrid`/`vulkan`/`auto` startup crash.
/// 1024 bytes leaves headroom for layout differences.
const PROPERTIES_BUFFER_U32: usize = 256;

/// 8-byte aligned scratch for structs that contain `VkDeviceSize` / `u64` members.
#[repr(C, align(16))]
struct Scratch<const N: usize>([u32; N]);
/// The subset of loaded Vulkan entry points used by this crate.
pub struct Api {
    /// Owns the `dlopen` handle. Dropping this would `dlclose` libvulkan and leave every
    /// function pointer below dangling, so the backend must hold it for as long as it uses
    /// any Vulkan call.
    _lib: std::mem::ManuallyDrop<libloading::Library>,
    pub instance: *mut c_void,
    pub destroy_instance: PfnDestroyInstance,
    pub enumerate_physical_devices: PfnEnumeratePhysicalDevices,
    pub get_properties: PfnGetPhysicalDeviceProperties,
    pub get_queue_families: PfnGetPhysicalDeviceQueueFamilyProperties,
    pub get_memory_properties: PfnGetPhysicalDeviceMemoryProperties,
    pub create_device: PfnCreateDevice,
    pub destroy_device: PfnDestroyDevice,
    pub get_device_queue: PfnGetDeviceQueue,
    pub create_swapchain_khr: PfnCreateSwapchainKHR,
    pub destroy_swapchain_khr: PfnDestroySwapchainKHR,
    pub get_swapchain_images_khr: PfnGetSwapchainImagesKHR,
    pub acquire_next_image_khr: PfnAcquireNextImageKHR,
    pub queue_present_khr: PfnQueuePresentKHR,
    pub create_render_pass: PfnCreateRenderPass,
    pub create_graphics_pipelines: PfnCreateGraphicsPipelines,
    pub create_framebuffer: PfnCreateFramebuffer,
    pub create_command_pool: PfnCreateCommandPool,
    pub allocate_command_buffers: PfnAllocateCommandBuffers,
    pub begin_command_buffer: PfnBeginCommandBuffer,
    pub cmd_begin_render_pass: PfnCmdBeginRenderPass,
    pub cmd_bind_pipeline: PfnCmdBindPipeline,
    pub cmd_draw: PfnCmdDraw,
    pub cmd_end_render_pass: PfnCmdEndRenderPass,
    pub end_command_buffer: PfnEndCommandBuffer,
    pub queue_submit: PfnQueueSubmit,
    pub create_shader_module: PfnCreateShaderModule,
    pub destroy_shader_module: PfnDestroyShaderModule,
    pub destroy_render_pass: PfnDestroyRenderPass,
    pub destroy_pipeline: PfnDestroyPipeline,
    pub destroy_command_pool: PfnDestroyCommandPool,
    pub destroy_framebuffer: PfnDestroyFramebuffer,
    pub create_pipeline_layout: PfnCreatePipelineLayout,
    pub destroy_pipeline_layout: PfnDestroyPipelineLayout,
}

impl Api {
    /// Loads libvulkan and creates an instance. `Err` carries a message worth logging.
    pub fn new(app_name: &str, app_version: u32) -> Result<Self, String> {
        // SAFETY: each symbol is used only with the signatures declared above.
        unsafe {
            let (lib, get_proc_addr) = load_library()?;

            // Ask the loader what it supports, so we do not request a version it refuses.
            // vkEnumerateInstanceVersion is 1.1+; absent means 1.0 only.
            let mut wanted = make_api_version(1, 0);
            if let Some(f) = get_opt(&get_proc_addr, ptr::null_mut(), "vkEnumerateInstanceVersion")
            {
                let enumerate: PfnEnumerateInstanceVersion = std::mem::transmute(f);
                let mut have = 0u32;
                if enumerate(&mut have) == VK_SUCCESS && have > 0 {
                    let have_major = have >> 22;
                    let have_minor = (have >> 12) & 0x3FF;
                    wanted = make_api_version(have_major.min(1), have_minor.min(3));
                }
            }

            let app = CString::new(app_name).unwrap_or_else(|_| CString::new("app").unwrap());
            let engine = CString::new("rust-renderer").unwrap();
            let app_info = VkApplicationInfo {
                s_type: VK_STRUCTURE_TYPE_APPLICATION_INFO,
                p_next: ptr::null(),
                p_application_name: app.as_ptr(),
                application_version: app_version,
                p_engine_name: engine.as_ptr(),
                engine_version: app_version,
                api_version: wanted,
            };
            let ci = VkInstanceCreateInfo {
                s_type: VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
                p_next: ptr::null(),
                flags: 0,
                p_application_info: &app_info,
                enabled_layer_count: 0,
                pp_enabled_layer_names: ptr::null(),
                enabled_extension_count: 0,
                pp_enabled_extension_names: ptr::null(),
            };

            let Some(create) = get_opt(&get_proc_addr, ptr::null_mut(), "vkCreateInstance") else {
                return Err("libvulkan has no vkCreateInstance (not a Vulkan loader?)".into());
            };
            let create: PfnCreateInstance = std::mem::transmute(create);
            let mut instance: *mut c_void = ptr::null_mut();
            let res = create(&ci, ptr::null(), &mut instance);
            if res != VK_SUCCESS || instance.is_null() {
                return Err(format!("vkCreateInstance failed (VkResult {res})"));
            }

            let need = |name: &str| -> Result<PFN, String> {
                get_opt(&get_proc_addr, instance, name)
                    .ok_or_else(|| format!("libvulkan is missing {name}"))
            };
            Ok(Self {
                _lib: std::mem::ManuallyDrop::new(lib),
                instance,
                destroy_instance: get_opt(&get_proc_addr, instance, "vkDestroyInstance")
                    .map(|f| std::mem::transmute::<PFN, PfnDestroyInstance>(f))
                    .unwrap_or(no_op_instance),
                enumerate_physical_devices: std::mem::transmute(need("vkEnumeratePhysicalDevices")?),
                get_properties: std::mem::transmute(need("vkGetPhysicalDeviceProperties")?),
                get_queue_families: std::mem::transmute(need(
                    "vkGetPhysicalDeviceQueueFamilyProperties",
                )?),
                get_memory_properties: std::mem::transmute(need(
                    "vkGetPhysicalDeviceMemoryProperties",
                )?),
                create_device: std::mem::transmute(need("vkCreateDevice")?),
                destroy_device: std::mem::transmute(need("vkDestroyDevice")?),
                get_device_queue: std::mem::transmute(need("vkGetDeviceQueue")?),
                create_swapchain_khr: std::mem::transmute(need("vkCreateSwapchainKHR")?),
                destroy_swapchain_khr: std::mem::transmute(need("vkDestroySwapchainKHR")?),
                get_swapchain_images_khr: std::mem::transmute(need("vkGetSwapchainImagesKHR")?),
                acquire_next_image_khr: std::mem::transmute(need("vkAcquireNextImageKHR")?),
                queue_present_khr: std::mem::transmute(need("vkQueuePresentKHR")?),
                create_render_pass: std::mem::transmute(need("vkCreateRenderPass")?),
                create_graphics_pipelines: std::mem::transmute(need("vkCreateGraphicsPipelines")?),
                create_framebuffer: std::mem::transmute(need("vkCreateFramebuffer")?),
                create_command_pool: std::mem::transmute(need("vkCreateCommandPool")?),
                allocate_command_buffers: std::mem::transmute(need("vkAllocateCommandBuffers")?),
                begin_command_buffer: std::mem::transmute(need("vkBeginCommandBuffer")?),
                cmd_begin_render_pass: std::mem::transmute(need("vkCmdBeginRenderPass")?),
                cmd_bind_pipeline: std::mem::transmute(need("vkCmdBindPipeline")?),
                cmd_draw: std::mem::transmute(need("vkCmdDraw")?),
                cmd_end_render_pass: std::mem::transmute(need("vkCmdEndRenderPass")?),
                end_command_buffer: std::mem::transmute(need("vkEndCommandBuffer")?),
                queue_submit: std::mem::transmute(need("vkQueueSubmit")?),
                create_shader_module: std::mem::transmute(need("vkCreateShaderModule")?),
                destroy_shader_module: std::mem::transmute(need("vkDestroyShaderModule")?),
                destroy_render_pass: std::mem::transmute(need("vkDestroyRenderPass")?),
                destroy_pipeline: std::mem::transmute(need("vkDestroyPipeline")?),
                destroy_command_pool: std::mem::transmute(need("vkDestroyCommandPool")?),
                destroy_framebuffer: std::mem::transmute(need("vkDestroyFramebuffer")?),
                create_pipeline_layout: std::mem::transmute(need("vkCreatePipelineLayout")?),
                destroy_pipeline_layout: std::mem::transmute(need("vkDestroyPipelineLayout")?),
            })
        }
    }

    pub fn physical_devices(&self) -> Vec<*mut c_void> {
        // SAFETY: `instance` was created by this Api and is destroyed only in Drop, so it
        // outlives every call here.
        unsafe {
        let mut count = 0u32;
        if (self.enumerate_physical_devices)(self.instance, &mut count, ptr::null_mut())
            != VK_SUCCESS
            || count == 0
        {
            return Vec::new();
        }
        let mut handles: Vec<*mut c_void> = vec![ptr::null_mut(); count as usize];
        let res = (self.enumerate_physical_devices)(self.instance, &mut count, handles.as_mut_ptr());
        if res != VK_SUCCESS && res != VK_INCOMPLETE {
            return Vec::new();
        }
        handles.truncate(count as usize);
        handles
        } // unsafe
    }

    /// Identity fields of `VkPhysicalDeviceProperties`, plus the leading `Limits` fields.
    ///
    /// The struct is 816 bytes. Rather than hardcode the whole trailing layout, the driver
    /// writes into an oversized zeroed buffer and only the fields whose offsets are
    /// asserted below are decoded. See `properties_offsets_are_stable` for the arithmetic.
    pub fn properties(&self, device: *mut c_void) -> Properties {
        let mut scratch = Scratch::<PROPERTIES_BUFFER_U32>([0u32; PROPERTIES_BUFFER_U32]);
        // SAFETY: the buffer (1024 B) is larger than the whole struct (824 B), so the
        // driver cannot write past it.
        unsafe { (self.get_properties)(device, scratch.0.as_mut_ptr() as *mut c_void) };
        let buf = &scratch.0;
        Properties {
            api_version: buf[0],
            driver_version: buf[1],
            vendor_id: buf[2],
            device_id: buf[3],
            device_type: buf[4],
            name: device_name(buf),
            max_image_dimension_2d: buf[LIMITS_INDEX_2D],
            max_image_dimension_3d: buf[LIMITS_INDEX_3D],
            max_image_array_layers: buf[LIMITS_INDEX_ARRAY_LAYERS],
        }
    }

    pub fn queue_families(&self, device: *mut c_void) -> Vec<VkQueueFamilyProperties> {
        // SAFETY: handles come from physical_devices(); the two-call idiom (count, then
        // array) is exactly what the loader expects.
        unsafe {
        let mut count = 0u32;
        (self.get_queue_families)(device, &mut count, ptr::null_mut());
        if count == 0 {
            return Vec::new();
        }
        let mut families = vec![VkQueueFamilyProperties::default(); count as usize];
        (self.get_queue_families)(device, &mut count, families.as_mut_ptr());
        families.truncate(count as usize);
        families
        } // unsafe
    }

    /// Device-local memory, from `VkPhysicalDeviceMemoryProperties`. Only the counts and the
    /// heap sizes are used; both are read from the fixed leading layout.
    pub fn device_local_memory(&self, device: *mut c_void) -> u64 {
        // uint32 memoryTypeCount; VkMemoryType memoryTypes[32] (8 B each);
        // uint32 memoryHeapCount (offset 260); VkMemoryHeap memoryHeaps[16] (16 B each,
        // 8-aligned, so they start at offset 264). Total 520 bytes.
        let mut scratch = Scratch::<130>([0u32; 130]);
        // SAFETY: the buffer matches the size of VkPhysicalDeviceMemoryProperties.
        unsafe { (self.get_memory_properties)(device, scratch.0.as_mut_ptr() as *mut c_void) };
        let buf = &scratch.0;
        // The old code read the heap count from word 1, which is memoryTypes[0].propertyFlags.
        let heaps = (buf[65] as usize).min(16);
        const HEAP_BASE_WORD: usize = 264 / 4;
        const VK_MEMORY_HEAP_DEVICE_LOCAL_BIT: u32 = 1;
        let mut total = 0u64;
        for i in 0..heaps {
            let lo = buf[HEAP_BASE_WORD + i * 4] as u64;
            let hi = buf[HEAP_BASE_WORD + i * 4 + 1] as u64;
            let flags = buf[HEAP_BASE_WORD + i * 4 + 2];
            if flags & VK_MEMORY_HEAP_DEVICE_LOCAL_BIT != 0 {
                total = total.max(lo | (hi << 32));
            }
        }
        total
    }

}

/// Decoded leading fields of `VkPhysicalDeviceProperties`.
#[derive(Clone, Debug)]
pub struct Properties {
    pub api_version: u32,
    pub driver_version: u32,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_type: u32,
    pub name: String,
    pub max_image_dimension_2d: u32,
    pub max_image_dimension_3d: u32,
    pub max_image_array_layers: u32,
}

/// `deviceName` is a fixed 256-byte array starting after the five leading u32 fields
/// (byte 20, i.e. word 5, running 64 words).
fn device_name(buf: &[u32]) -> String {
    let bytes: Vec<u8> = buf[5..69]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .take(256)
        .take_while(|b| *b != 0)
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// Resolves an entry point. The loader returns null for names it does not know, which
/// collapses to `None` here.
fn get_opt(
    get_proc_addr: &PfnGetInstanceProcAddr,
    instance: *mut c_void,
    name: &str,
) -> Option<PFN> {
    let c = CString::new(name).ok()?;
    // SAFETY: querying by name is valid for a null instance and for a live one, and the
    // returned pointer is only dereferenced through the typed aliases below.
    unsafe { get_proc_addr(instance, c.as_ptr()) }
}

unsafe extern "C" fn no_op_instance(_instance: *mut c_void, _alloc: *const c_void) {}

/// Candidate sonames, in the order Android's loader prefers.
const LIB_PATHS: &[&str] = &[
    "libvulkan.so",
    "/system/lib64/libvulkan.so",
    "/vendor/lib64/libvulkan.so",
    "libvulkan.so.1",
];

type LoadedLib = (libloading::Library, PfnGetInstanceProcAddr);

/// # Safety
/// Returns a live library handle plus a resolved entry point; the caller must not unload it.
unsafe fn load_library() -> Result<LoadedLib, String> {
    let mut errors: Vec<String> = Vec::new();
    for path in LIB_PATHS {
        let lib = match libloading::Library::new(path) {
            Ok(l) => l,
            Err(e) => {
                errors.push(format!("{path}: {e}"));
                continue;
            }
        };
        let addr = match lib.get::<PfnGetInstanceProcAddr>(b"vkGetInstanceProcAddr\0") {
            Ok(a) => *a,
            Err(e) => {
                errors.push(format!("{path}: no vkGetInstanceProcAddress ({e})"));
                continue;
            }
        };
        return Ok((lib, addr));
    }
    Err(format!("no Vulkan loader found ({})", errors.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_decodes() {
        assert_eq!(api_version_string(make_api_version(1, 3)), "1.3.0");
        assert_eq!(api_version_string(make_api_version(1, 0)), "1.0.0");
    }

    #[test]
    fn device_preference_prefers_discrete_then_integrated() {
        assert!(device_type_rank(VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU) > device_type_rank(VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU));
        assert!(device_type_rank(VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU) > device_type_rank(VK_PHYSICAL_DEVICE_TYPE_CPU));
    }

    #[test]
    fn device_name_stops_at_the_first_nul() {
        // deviceName starts after five u32 fields and is NUL-padded.
        let mut buf = [0u32; PROPERTIES_BUFFER_U32];
        let name = b"Test GPU\0";
        for (i, chunk) in name.chunks(4).enumerate() {
            let mut w = [0u8; 4];
            w[..chunk.len()].copy_from_slice(chunk);
            buf[5 + i] = u32::from_le_bytes(w);
        }
        assert_eq!(device_name(&buf), "Test GPU");
    }

    #[test]
    fn properties_offsets_are_stable() {
        // apiVersion..deviceType are five u32 words, deviceName is 256 bytes (64 words),
        // pipelineCacheUUID is 16 bytes (4 words), so `limits` begins at word 5 + 64 + 4.
        assert_eq!(LIMITS_START_U32, 73);
        // maxImageDimension1D is the first u32 of Limits, so 2D is one word later.
        assert_eq!(LIMITS_INDEX_2D, 74);
        assert_eq!(LIMITS_INDEX_3D, 75);
        assert_eq!(LIMITS_INDEX_ARRAY_LAYERS, 77);
        // The decode buffer must reach the last field it reads...
        assert!(LIMITS_INDEX_ARRAY_LAYERS < PROPERTIES_BUFFER_U32);
        // ...and be at least as large as the whole struct the driver writes (824 bytes).
        assert!(PROPERTIES_BUFFER_U32 * 4 >= 824);
    }

    #[test]
    fn vendor_ids_map_to_names() {
        assert_eq!(vendor_name(0x13B5), "ARM");
        assert_eq!(vendor_name(0x5143), "Qualcomm");
        assert_eq!(vendor_name(0xDEAD), "vendor 0xDEAD");
    }

    #[test]
    fn missing_vulkan_loader_is_reported_not_panicked() {
        // On a host without a Vulkan ICD this must be a clean error, so `auto` can fall
        // back to GLES instead of crashing the game process.
        if let Err(msg) = Api::new("rust-renderer-test", 1) {
            assert!(!msg.is_empty());
        }
    }
}
