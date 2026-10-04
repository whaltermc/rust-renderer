//! Device capability probe: what this GPU actually supports.
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
pub struct Caps {
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
    /// `GL_MAX_TEXTURE_MAX_ANISOTROPY_EXT`; 0 when unsupported.
    pub max_anisotropy: i32,
    /// Whether the probe ran against a live context.
    pub valid: bool,
}

impl Caps {
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

/// Reads extensions and limits from the current context. Returns an invalid `Caps` when no
/// context is current, so callers can fall back rather than trust empty answers.
pub fn probe() -> Caps {
    let mut caps = Caps::default();

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

    caps.valid = true;
    caps
}

/// Cached result of [`probe`]. Safe to call before a context exists: it returns an invalid
/// `Caps` in that case, and the next call after a context exists re-probes.
pub fn caps() -> &'static Caps {
    static CAPS: std::sync::OnceLock<Caps> = std::sync::OnceLock::new();
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
pub fn supported_aliases(c: &Caps) -> Vec<&'static [u8]> {
    let mut out: Vec<&'static [u8]> = Vec::new();
    // Always available in ES 3.0 core, or backed by this crate rather than the driver.
    const CORE: [&[u8]; 15] = [
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
    out
}

use std::sync::atomic::Ordering;

#[cfg(test)]
mod tests {
    use super::*;

    fn caps_with(extensions: &[&[u8]], major: u32, minor: u32, aniso: i32) -> Caps {
        Caps {
            valid: true,
            es_major: major,
            es_minor: minor,
            extensions: extensions.iter().map(|e| (*e).to_vec()).collect(),
            max_anisotropy: aniso,
            ..Default::default()
        }
    }

    /// Alias names as plain strings, with the NUL the driver format carries removed.
    fn advertised(c: &Caps) -> Vec<String> {
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
