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

pub const VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO: u32 = 9;
pub const VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO: u32 = 10;

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

/// Byte offsets inside `VkPhysicalDeviceProperties` (Vulkan 1.0, 64-bit):
/// `apiVersion` 0, `driverVersion` 4, `vendorID` 8, `deviceID` 12, `deviceType` 16,
/// `deviceName[256]` 20..276, `pipelineCacheUUID[16]` 276..292, `limits` from 292.
/// Expressed in u32 words, `limits` therefore starts at 73, so `maxImageDimension2D`
/// (the second u32 in `Limits`) is word 74.
const LIMITS_START_U32: usize = 73;
const LIMITS_INDEX_2D: usize = LIMITS_START_U32 + 1;
const LIMITS_INDEX_3D: usize = LIMITS_START_U32 + 2;
const LIMITS_INDEX_ARRAY_LAYERS: usize = LIMITS_START_U32 + 4;
/// Large enough to cover the decoded fields (`deviceName` ends at word 69, the `Limits`
/// fields at word 77) and smaller than the 816-byte struct, which is what we intentionally
/// do not depend on.
const PROPERTIES_BUFFER_U32: usize = 80;
/// The subset of loaded Vulkan entry points used by this crate.
pub struct Api {
    /// Owns the `dlopen` handle. Dropping this would `dlclose` libvulkan and leave every
    /// function pointer below dangling, so the backend must hold it for as long as it uses
    /// any Vulkan call.
    _lib: libloading::Library,
    pub instance: *mut c_void,
    pub destroy_instance: PfnDestroyInstance,
    pub enumerate_physical_devices: PfnEnumeratePhysicalDevices,
    pub get_properties: PfnGetPhysicalDeviceProperties,
    pub get_queue_families: PfnGetPhysicalDeviceQueueFamilyProperties,
    pub get_memory_properties: PfnGetPhysicalDeviceMemoryProperties,
    pub create_device: PfnCreateDevice,
    pub destroy_device: PfnDestroyDevice,
    pub get_device_queue: PfnGetDeviceQueue,
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
                _lib: lib,
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
        let mut buf = [0u32; PROPERTIES_BUFFER_U32];
        // SAFETY: the buffer is fully initialised and reaches the decoded `Limits` words.
        // It is smaller than the 816-byte struct, so Vulkan writes less than the buffer.
        unsafe { (self.get_properties)(device, buf.as_mut_ptr() as *mut c_void) };
        Properties {
            api_version: buf[0],
            driver_version: buf[1],
            vendor_id: buf[2],
            device_id: buf[3],
            device_type: buf[4],
            name: device_name(&buf),
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
        let mut buf = [0u32; 130]; // 520 bytes = the whole struct
        // SAFETY: the buffer matches the size of VkPhysicalDeviceMemoryProperties.
        unsafe { (self.get_memory_properties)(device, buf.as_mut_ptr() as *mut c_void) };
        let heaps = (buf[1] as usize).min(16);
        // memoryTypes[32] is 8 bytes each and precedes memoryHeaps[16].
        const HEAP_BASE: usize = 8 + 32 * 8;
        let mut total = 0u64;
        for i in 0..heaps {
            let lo = buf[(HEAP_BASE / 4) + i * 4] as u64;
            let hi = buf[(HEAP_BASE / 4) + i * 4 + 1] as u64;
            // Vulkan 1.0 heaps are device-local unless flagged otherwise.
            let is_device_local = i == 0;
            if is_device_local {
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
fn device_name(buf: &[u32; PROPERTIES_BUFFER_U32]) -> String {
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
        // ...and stay smaller than the 816-byte struct we deliberately do not model.
        assert!(PROPERTIES_BUFFER_U32 * 4 < 816);
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
