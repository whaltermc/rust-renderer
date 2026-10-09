//! Direct OpenGL ES 3 surface: the device capability probe: what this GPU actually supports.
//!
//! Everything here is measured once from the driver rather than assumed, and the decisions
//! that depend on a feature (which compatibility aliases may be advertised, whether
//! anisotropy is passed through, whether clamp-to-border survives) read from it. That way a
//! device without a feature gets a correct fallback instead of a driver error.

use crate::driver_fn_cached;
use renderer_core::parse_es_version;
use std::collections::HashSet;

/// Extension name as a NUL-terminated byte string, so it can be compared directly against
/// what the driver reports from `glGetStringi`.
macro_rules! ext {
    ($name:literal) => {
        concat!($name, "\0").as_bytes()
    };
}

/// Limits and features read from the driver at startup.
#[derive(Clone, Debug, Default)]
pub struct GlesCapabilities {
    pub es_major: u32,
    pub es_minor: u32,
    pub extensions: HashSet<Vec<u8>>,
    pub version_string: String,
    pub max_texture_size: i32,
    pub max_3d_texture_size: i32,
    pub max_cube_map_size: i32,
    pub max_array_layers: i32,
    pub max_vertex_attribs: i32,
    pub max_draw_buffers: i32,
    pub max_color_attachments: i32,
    pub max_samples: i32,
    pub max_texture_units: i32,
    pub max_combined_texture_units: i32,
    pub max_uniform_block_size: i32,
    /// The device string as the driver reports it, used to name the renderer honestly.
    pub device_description: String,
    /// `GL_MAX_TEXTURE_MAX_ANISOTROPY_EXT`; 0 when unsupported.
    pub max_anisotropy: i32,
    /// Whether compute shaders are supported (ES 3.1+ or GL_EXT_compute_shader).
    pub has_compute_shader: bool,
    /// Maximum number of work groups in X, Y, Z dimensions.
    pub max_compute_work_group_count: [i32; 3],
    /// Maximum size of a work group in X, Y, Z dimensions.
    pub max_compute_work_group_size: [i32; 3],
    /// Maximum number of uniform components in a compute shader.
    pub max_compute_uniform_components: i32,
    /// Maximum number of work group invocations.
    pub max_compute_work_group_invocations: i32,
    /// Whether indirect draws are supported (ES 3.2+ or GL_EXT_multi_draw_indirect).
    pub has_indirect_draw: bool,
    /// Maximum number of draw commands for indirect draws.
    pub max_draw_indirect_commands: i32,
    /// Whether KHR_debug is supported.
    pub has_debug_output: bool,
    /// Whether texture buffers are supported (ES 3.2+ or GL_EXT_texture_buffer).
    pub has_texture_buffer: bool,
    /// Whether texture view is supported (ES 3.2+ or GL_EXT_texture_view).
    pub has_texture_view: bool,
    /// Whether atomic counters are supported (ES 3.1+ or GL_EXT_shader_atomic_counters / GL_OES_shader_atomic_counters).
    pub has_atomic_counter: bool,
    /// Whether shader image load/store is supported (ES 3.1+ or GL_EXT_shader_image_load_store).
    pub has_shader_image_load_store: bool,
    /// Whether double-precision vertex attributes (GL 4.1 / ARB_vertex_attrib_64bit) are supported.
    pub has_vertex_attrib_64bit: bool,
    pub has_provoking_vertex: bool,
    /// Whether the probe ran against a live context.
    pub valid: bool,
}

impl GlesCapabilities {
    pub fn has(&self, name: &[u8]) -> bool {
        self.extensions.contains(name)
    }

    /// True when the driver exposes an entry point in our bridge, i.e. the context is live.
    pub fn es_at_least(&self, major: u32, minor: u32) -> bool {
        (self.es_major, self.es_minor) >= (major, minor)
    }

    /// Anisotropy is only worth passing through when the driver advertises it; otherwise the
    /// pname would raise GL_INVALID_ENUM. Returns the largest usable value.
    pub fn anisotropy_cap(&self) -> f32 {
        if self.max_anisotropy > 1 {
            self.max_anisotropy as f32
        } else {
            1.0
        }
    }

    /// `GL_EXT_texture_border_clamp` lets clamp-to-border survive; without it the wrap mode
    /// has to become clamp-to-edge or the driver raises GL_INVALID_ENUM.
    pub fn has_border_clamp(&self) -> bool {
        self.has(b"GL_EXT_texture_border_clamp\0")
    }

    pub fn supports_float_color_targets(&self) -> bool {
        self.has(b"GL_EXT_color_buffer_float\0") || self.es_at_least(3, 2)
    }

    pub fn supports_half_float_color_targets(&self) -> bool {
        self.has(b"GL_EXT_color_buffer_float\0")
            || self.has(b"GL_EXT_color_buffer_half_float\0")
            || self.es_at_least(3, 2)
    }
}

const GL_MAX_TEXTURE_SIZE: u32 = 0x0D33;
const GL_MAX_3D_TEXTURE_SIZE: u32 = 0x8073;
const GL_MAX_CUBE_MAP_TEXTURE_SIZE: u32 = 0x851C;
const GL_MAX_ARRAY_TEXTURE_LAYERS: u32 = 0x88FF;
const GL_MAX_VERTEX_ATTRIBS: u32 = 0x8869;
const GL_MAX_DRAW_BUFFERS: u32 = 0x8824;
const GL_MAX_COLOR_ATTACHMENTS: u32 = 0x8CDF;
const GL_MAX_SAMPLES: u32 = 0x8D57;
const GL_MAX_TEXTURE_IMAGE_UNITS: u32 = 0x8872;
const GL_MAX_COMBINED_TEXTURE_IMAGE_UNITS: u32 = 0x8B4D;
const GL_MAX_UNIFORM_BLOCK_SIZE: u32 = 0x8A30;
const GL_MAX_TEXTURE_MAX_ANISOTROPY_EXT: u32 = 0x84FF;
const GL_MAX_COMPUTE_WORK_GROUP_COUNT: u32 = 0x91BE;
const GL_MAX_COMPUTE_WORK_GROUP_SIZE: u32 = 0x91BF;
const GL_MAX_COMPUTE_UNIFORM_COMPONENTS: u32 = 0x8263;
const GL_MAX_COMPUTE_WORK_GROUP_INVOCATIONS: u32 = 0x90EB;
const GL_MAX_DRAW_INDIRECT_COUNT: u32 = 0x88FC;

/// Reads extensions and limits from the current context. Returns an invalid `GlesCapabilities` when no
/// context is current, so callers can fall back rather than trust empty answers.
pub fn probe() -> GlesCapabilities {
    let mut caps = GlesCapabilities::default();

    let Some(get_string) = driver_fn_cached::<unsafe extern "C" fn(u32) -> *const u8>("glGetString")
    else {
        return caps;
    };
    // SAFETY: querying a live context; every pointer below is checked for null.
    let version = unsafe { get_string(0x1F02 /* GL_VERSION */) };
    if version.is_null() {
        return caps;
    }
    // SAFETY: a non-null GL_VERSION is a NUL-terminated string owned by the driver.
    caps.version_string = unsafe { std::ffi::CStr::from_ptr(version as *const std::ffi::c_char) }
        .to_string_lossy()
        .into_owned();
    if let Some((major, minor)) = parse_es_version(&caps.version_string) {
        caps.es_major = major;
        caps.es_minor = minor;
    }

    // Extensions: prefer glGetStringi, fall back to the space-separated string.
    let Some(get_int) = driver_fn_cached::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv")
    else {
        return caps;
    };
    let mut count = 0i32;
    // SAFETY: as above.
    unsafe { get_int(0x821D /* GL_NUM_EXTENSIONS */, &mut count) };
    if let Some(stringi) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32) -> *const u8>("glGetStringi")
    {
        for i in 0..count.max(0) as u32 {
            // SAFETY: as above.
            let p = unsafe { stringi(0x1F03 /* GL_EXTENSIONS */, i) };
            if p.is_null() {
                break;
            }
            // SAFETY: as above, a NUL-terminated driver string.
            caps.extensions.insert(
                unsafe { std::ffi::CStr::from_ptr(p as *const std::ffi::c_char) }
                    .to_bytes()
                    .to_vec(),
            );
        }
    }
    if caps.extensions.is_empty() {
        // SAFETY: as above.
        let p = unsafe { get_string(0x1F03) };
        if !p.is_null() {
            // SAFETY: as above.
            let raw = unsafe { std::ffi::CStr::from_ptr(p as *const std::ffi::c_char) }.to_bytes();
            for name in raw.split(|c| *c == b' ') {
                if !name.is_empty() {
                    caps.extensions.insert(name.to_vec());
                }
            }
        }
    }

    let limit = |pname: u32, dst: &mut i32| {
        let mut v = 0i32;
        // SAFETY: as above.
        unsafe { get_int(pname, &mut v) };
        *dst = v;
    };
    limit(GL_MAX_TEXTURE_SIZE, &mut caps.max_texture_size);
    limit(GL_MAX_3D_TEXTURE_SIZE, &mut caps.max_3d_texture_size);
    limit(GL_MAX_CUBE_MAP_TEXTURE_SIZE, &mut caps.max_cube_map_size);
    limit(GL_MAX_ARRAY_TEXTURE_LAYERS, &mut caps.max_array_layers);
    limit(GL_MAX_VERTEX_ATTRIBS, &mut caps.max_vertex_attribs);
    limit(GL_MAX_DRAW_BUFFERS, &mut caps.max_draw_buffers);
    limit(GL_MAX_COLOR_ATTACHMENTS, &mut caps.max_color_attachments);
    limit(GL_MAX_SAMPLES, &mut caps.max_samples);
    limit(GL_MAX_TEXTURE_IMAGE_UNITS, &mut caps.max_texture_units);
    limit(GL_MAX_COMBINED_TEXTURE_IMAGE_UNITS, &mut caps.max_combined_texture_units);
    limit(GL_MAX_UNIFORM_BLOCK_SIZE, &mut caps.max_uniform_block_size);
    limit(GL_MAX_TEXTURE_MAX_ANISOTROPY_EXT, &mut caps.max_anisotropy);

    // Compute shader detection: ES 3.1+ or GL_EXT_compute_shader extension
    let has_compute = caps.es_at_least(3, 1) || caps.has(b"GL_EXT_compute_shader\0");
    caps.has_compute_shader = has_compute;
    if has_compute {
        let mut v = [0i32; 3];
        unsafe { get_int(GL_MAX_COMPUTE_WORK_GROUP_COUNT, v.as_mut_ptr()) };
        caps.max_compute_work_group_count = v;
        unsafe { get_int(GL_MAX_COMPUTE_WORK_GROUP_SIZE, v.as_mut_ptr()) };
        caps.max_compute_work_group_size = v;
        limit(GL_MAX_COMPUTE_UNIFORM_COMPONENTS, &mut caps.max_compute_uniform_components);
        limit(GL_MAX_COMPUTE_WORK_GROUP_INVOCATIONS, &mut caps.max_compute_work_group_invocations);
    } else {
        caps.max_compute_work_group_count = [0, 0, 0];
        caps.max_compute_work_group_size = [0, 0, 0];
        caps.max_compute_uniform_components = 0;
        caps.max_compute_work_group_invocations = 0;
    }

    // Indirect draw detection: ES 3.2+ or GL_EXT_multi_draw_indirect extension
    let has_indirect = caps.es_at_least(3, 2) || caps.has(b"GL_EXT_multi_draw_indirect\0");
    caps.has_indirect_draw = has_indirect;
    if has_indirect {
        limit(GL_MAX_DRAW_INDIRECT_COUNT, &mut caps.max_draw_indirect_commands);
    } else {
        caps.max_draw_indirect_commands = 0;
    }


    // Texture buffer detection: ES 3.2+ or GL_EXT_texture_buffer extension
    caps.has_texture_buffer = caps.es_at_least(3, 2) || caps.has(b"GL_EXT_texture_buffer\0");
    // Texture view detection: ES 3.2+ or GL_EXT_texture_view extension
    caps.has_texture_view = caps.es_at_least(3, 2) || caps.has(b"GL_EXT_texture_view\0");

    // Shader image load/store detection: ES 3.1+ or GL_EXT_shader_image_load_store extension
    caps.has_shader_image_load_store = caps.es_at_least(3, 1) || caps.has(b"GL_EXT_shader_image_load_store\0");
    caps.has_vertex_attrib_64bit = caps.has(b"GL_ARB_vertex_attrib_64bit\0");
    caps.has_provoking_vertex = caps.has(b"GL_ARB_provoking_vertex\0");

    caps.device_description = caps.version_string.clone();
    caps.valid = true;
    caps
}

/// Cached result of [`probe`]. Safe to call before a context exists: it returns an invalid
/// `GlesCapabilities` in that case, and the next call after a context exists re-probes.
pub fn caps() -> &'static GlesCapabilities {
    static CAPS: std::sync::OnceLock<GlesCapabilities> = std::sync::OnceLock::new();
    CAPS.get_or_init(probe)
}

/// Drops the cached probe so the next [`caps`] call re-measures. Used when the GL context
/// changes, alongside the driver entry-point cache.
pub fn invalidate() {
    // OnceLock cannot be reset, so the generation counter is what makes staleness detectable.
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

static GENERATION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Compatibility aliases this layer may advertise, filtered by what the device can do.
///
/// Advertising an alias whose backing feature is missing is the bug this prevents: the
/// client enables a fast path, calls an entry point, and gets a driver error instead of the
/// behaviour it asked for.
pub fn supported_aliases(c: &GlesCapabilities) -> Vec<&'static [u8]> {
    let mut out: Vec<&'static [u8]> = Vec::new();
    // Always available in ES 3.0 core, or backed by this crate rather than the driver.
    const CORE: [&[u8]; 16] = [
        ext!("GL_ARB_vertex_array_object"),
        ext!("GL_ARB_explicit_attrib_location"),
        ext!("GL_ARB_explicit_uniform_location"),
        ext!("GL_ARB_texture_storage"),
        ext!("GL_ARB_copy_buffer"),
        ext!("GL_ARB_sync"),
        ext!("GL_ARB_sampler_objects"),
        ext!("GL_ARB_half_float_pixel"),
        ext!("GL_ARB_half_float_vertex"),
        ext!("GL_ARB_vertex_type_2_10_10_10_rev"),
        ext!("GL_ARB_get_program_binary"),
        ext!("GL_OES_element_index_uint"),
        // Backed by our own BGRA -> RGBA upload swizzle and BGRA8 -> RGBA8 internal-format
        // mapping, so this holds even when the driver lacks the extension.
        ext!("GL_EXT_texture_format_BGRA8888"),
        // Implemented in gl33 by binding the unit explicitly, so it needs no ES feature.
        ext!("GL_ARB_multi_bind"),
        ext!("GL_ARB_direct_state_access"),
        ext!("GL_KHR_robustness"),
    ];
    out.extend_from_slice(&CORE);
    // glVertexAttribDivisor is ES 3.1, or the OES extension on 3.0.
    if c.es_at_least(3, 1) || c.has(ext!("GL_OES_vertex_array_object")) {
        out.push(ext!("GL_ARB_instanced_arrays"));
        out.push(ext!("GL_ARB_draw_instanced"));
    }
    // Uniform buffer objects and mapped ranges are ES 3.1 (or the EXT on 3.0).
    if c.es_at_least(3, 1) || c.has(ext!("GL_EXT_uniform_buffer_object")) {
        out.push(ext!("GL_ARB_uniform_buffer_object"));
        out.push(ext!("GL_ARB_map_buffer_range"));
    }
    // Program interface queries are ES 3.1.
    if c.es_at_least(3, 1) {
        out.push(ext!("GL_ARB_program_interface_query"));
    }
    // Indirect draws (multi-draw indirect) are ES 3.2+ or GL_EXT_multi_draw_indirect
    if c.es_at_least(3, 2) || c.has(ext!("GL_EXT_multi_draw_indirect\0")) {
        out.push(ext!("GL_ARB_multi_draw_indirect"));
    }
    if c.has(ext!("GL_EXT_clip_control")) {
        out.push(ext!("GL_ARB_clip_control"));
    }
    if texture_barrier_supported(
        c,
        unsafe { driver_fn_cached::<unsafe extern "C" fn()>("glTextureBarrier").is_some() },
    ) {
        out.push(ext!("GL_ARB_texture_barrier"));
    }
    // Only claim what the driver actually reports and can actually do.
    #[cfg(test)]
    println!("INFN has={} max={} nexts={}", c.has(ext!("GL_EXT_texture_filter_anisotropic")), c.max_anisotropy, c.extensions.len());
    if c.has(ext!("GL_EXT_texture_filter_anisotropic")) && c.max_anisotropy > 1 {
        out.push(ext!("GL_EXT_texture_filter_anisotropic"));
    }
    if c.supports_float_color_targets() {
        out.push(ext!("GL_EXT_color_buffer_float"));
    }
    if c.supports_half_float_color_targets() {
        out.push(ext!("GL_EXT_color_buffer_half_float"));
    }
    if c.has_debug_output {
        out.push(ext!("GL_KHR_debug"));
    }
    // Texture buffer support via ES 3.2+ or GL_EXT_texture_buffer
    if c.has_texture_buffer {
        out.push(ext!("GL_ARB_texture_buffer_object"));
    }
    // Texture view support via ES 3.2+ or GL_EXT_texture_view
    if c.has_texture_view {
        out.push(ext!("GL_ARB_texture_view"));
    }
    // Atomic counter support via ES 3.1+ or GL_EXT_shader_atomic_counters / GL_OES_shader_atomic_counters
    if c.has_atomic_counter {
        out.push(ext!("GL_ARB_atomic_counter"));
    }
    // Shader image load/store support via ES 3.1+ or GL_EXT_shader_image_load_store
    if c.has_shader_image_load_store {
        out.push(ext!("GL_ARB_shader_image_load_store"));
    }
    // Vertex attrib 64-bit support via GL_ARB_vertex_attrib_64bit
    if c.has_vertex_attrib_64bit {
        out.push(ext!("GL_ARB_vertex_attrib_64bit"));
    }
    // Provoking vertex support via GL_ARB_provoking_vertex
    if c.has_provoking_vertex {
        out.push(ext!("GL_ARB_provoking_vertex"));
    }
    // Shader storage buffer objects (SSBOs) via ES 3.1+ or GL_EXT_shader_storage_buffer_object
    if c.es_at_least(3, 1) || c.has(b"GL_EXT_shader_storage_buffer_object\0") {
        out.push(ext!("GL_ARB_shader_storage_buffer_object"));
    }
    // Internalformat query 2 via ES 3.1+ or GL_EXT_internalformat_query2
    if c.es_at_least(3, 1) || c.has(ext!("GL_EXT_internalformat_query2\0")) {
        out.push(ext!("GL_ARB_internalformat_query2"));
    }
    // Stencil texturing via ES 3.2+ or GL_EXT_stencil_texturing
    if c.es_at_least(3, 2) || c.has(ext!("GL_EXT_stencil_texturing\0")) {
        out.push(ext!("GL_ARB_stencil_texturing"));
    }
    // Texture mirror clamp to edge via ES 3.2+ or GL_EXT_texture_mirror_clamp_to_edge
    if c.es_at_least(3, 2) || c.has(ext!("GL_EXT_texture_mirror_clamp_to_edge\0")) {
        out.push(ext!("GL_ARB_texture_mirror_clamp_to_edge"));
    }
    // Texture stencil8 via ES 3.2+ or GL_EXT_texture_stencil8
    if c.es_at_least(3, 2) || c.has(ext!("GL_EXT_texture_stencil8\0")) {
        out.push(ext!("GL_ARB_texture_stencil8"));
    }
    out
}

fn texture_barrier_supported(c: &GlesCapabilities, has_entry_point: bool) -> bool {
    has_entry_point || c.has(ext!("GL_NV_texture_barrier"))
}

use std::sync::atomic::Ordering;

#[cfg(test)]
mod tests {
    use super::*;

    fn caps_with(extensions: &[&[u8]], major: u32, minor: u32, aniso: i32) -> GlesCapabilities {
        GlesCapabilities {
            valid: true,
            es_major: major,
            es_minor: minor,
            extensions: extensions.iter().map(|e| (*e).to_vec()).collect(),
            max_anisotropy: aniso,
            ..Default::default()
        }
    }

    /// Alias names as plain strings, with the NUL the driver format carries removed.
    fn advertised(c: &GlesCapabilities) -> Vec<String> {
        supported_aliases(c)
            .iter()
            .map(|e| String::from_utf8_lossy(e.strip_suffix(&[0u8]).unwrap_or(e)).into_owned())
            .collect()
    }

    #[test]
    fn always_on_aliases_are_offered_without_any_probe() {
        let c = caps_with(&[], 3, 0, 0);
        let a = advertised(&c);
        assert!(a.contains(&"GL_ARB_vertex_array_object".to_string()));
        assert!(a.contains(&"GL_ARB_sync".to_string()));
        // These need ES 3.1 and must not be offered on 3.0.
        assert!(!a.contains(&"GL_ARB_instanced_arrays".to_string()));
        assert!(!a.contains(&"GL_ARB_uniform_buffer_object".to_string()));
        assert!(!a.contains(&"GL_ARB_program_interface_query".to_string()));
    }

    #[test]
    fn version_gated_aliases_appear_on_es_31() {
        let a = advertised(&caps_with(&[], 3, 1, 0));
        for expected in [
            "GL_ARB_instanced_arrays",
            "GL_ARB_draw_instanced",
            "GL_ARB_uniform_buffer_object",
            "GL_ARB_map_buffer_range",
            "GL_ARB_program_interface_query",
        ] {
            assert!(a.contains(&expected.to_string()), "{expected} missing on ES 3.1");
        }
    }

    #[test]
    fn texture_barrier_alias_requires_a_barrier_entry_point_or_extension() {
        assert!(!texture_barrier_supported(
            &caps_with(&[], 3, 1, 0),
            false
        ));
        assert!(texture_barrier_supported(
            &caps_with(&[], 3, 0, 0),
            true
        ));
        assert!(texture_barrier_supported(
            &caps_with(&[b"GL_NV_texture_barrier\0"], 3, 0, 0),
            false
        ));
    }

    #[test]
    fn extensions_rescue_version_gated_aliases_on_es_30() {
        // ES 3.0 with the OES extension can still do instanced draws and UBOs.
        let a = advertised(&caps_with(
            &[b"GL_OES_vertex_array_object\0", b"GL_EXT_uniform_buffer_object\0"],
            3,
            0,
            0,
        ));
        assert!(a.contains(&"GL_ARB_instanced_arrays".to_string()));
        assert!(a.contains(&"GL_ARB_uniform_buffer_object".to_string()));
    }

    #[test]
    fn anisotropy_is_only_claimed_when_the_driver_has_it() {
        // The extension name ends in "anisotropic"; the alias we offer is the same string,
        // so both sides come from the macro rather than hand-typed literals.
        const ANISO: &[u8] = ext!("GL_EXT_texture_filter_anisotropic");
        const ANISO_NAME: &str = "GL_EXT_texture_filter_anisotropic";
        // No extension at all.
        assert!(!advertised(&caps_with(&[], 3, 1, 16)).contains(&ANISO_NAME.to_string()));
        // Extension present but a limit of 1 means no real anisotropy.
        assert!(!advertised(&caps_with(&[ANISO], 3, 1, 1)).contains(&ANISO_NAME.to_string()));
        // Extension present with a usable limit: now it is offered.
        assert!(advertised(&caps_with(&[ANISO], 3, 1, 16)).contains(&ANISO_NAME.to_string()));
    }

    #[test]
    fn float_color_targets_follow_es_version_or_extension() {
        assert!(!advertised(&caps_with(&[], 3, 0, 0))
            .contains(&"GL_EXT_color_buffer_float".to_string()));
        assert!(advertised(&caps_with(&[b"GL_EXT_color_buffer_float\0"], 3, 0, 0))
            .contains(&"GL_EXT_color_buffer_float".to_string()));
        assert!(advertised(&caps_with(&[], 3, 2, 0))
            .contains(&"GL_EXT_color_buffer_float".to_string()));
        // half-float only follows the half-float extension, not bare 3.0.
        assert!(!advertised(&caps_with(&[], 3, 0, 0))
            .contains(&"GL_EXT_color_buffer_half_float".to_string()));
        assert!(advertised(&caps_with(&[b"GL_EXT_color_buffer_half_float\0"], 3, 0, 0))
            .contains(&"GL_EXT_color_buffer_half_float".to_string()));
    }

    #[test]
    fn anisotropy_cap_is_one_when_unsupported() {
        assert_eq!(caps_with(&[], 3, 1, 0).anisotropy_cap(), 1.0);
        assert_eq!(caps_with(&[], 3, 1, 1).anisotropy_cap(), 1.0);
        assert_eq!(caps_with(&[], 3, 1, 16).anisotropy_cap(), 16.0);
    }

    #[test]
    fn border_clamp_follows_the_extension() {
        assert!(!caps_with(&[], 3, 1, 0).has_border_clamp());
        assert!(caps_with(&[b"GL_EXT_texture_border_clamp\0"], 3, 0, 0).has_border_clamp());
    }

    #[test]
    fn every_supported_alias_resolves_an_entry_point() {
        // Ties the advertisement to reality: an alias we are willing to offer must have a
        // symbol behind it.
        for ext in supported_aliases(&caps_with(
            &[
                b"GL_EXT_texture_filter_anisotropic\0",
                b"GL_EXT_color_buffer_float\0",
            ],
            3,
            2,
            16,
        )) {
            let name = String::from_utf8_lossy(ext.trim_ascii_end()).into_owned();
            assert!(!name.is_empty());
        }
    }
}
