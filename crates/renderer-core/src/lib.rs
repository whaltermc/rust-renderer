//! Backend-independent renderer concepts. No GLES or Vulkan dependency here.

use std::ffi::c_void;
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// Try each backend in order and use the first that initializes.
    Auto,
    /// GLES 3.x passthrough. This is what serves the game's desktop-GL entry points.
    Gles,
    /// Vulkan only.
    Vulkan,
    /// GLES for the GL API surface the game draws through, Vulkan for the work this
    /// renderer owns itself. Falls back per-stage, never per-frame, so no frame is ever
    /// split across two APIs.
    Hybrid,
}

impl BackendKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "gles" | "es" | "opengles" | "opengles3" => Some(Self::Gles),
            "vulkan" | "vk" | "vulkan1" => Some(Self::Vulkan),
            "hybrid" | "vk_es" | "vulkan+gles" | "gles+vulkan" => Some(Self::Hybrid),
            _ => None,
        }
    }

    /// Stable lowercase name, as used by the launcher's renderer options.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Gles => "gles",
            Self::Vulkan => "vulkan",
            Self::Hybrid => "hybrid",
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
    /// Reads the backend selection and debug flag from the environment.
    pub fn from_env() -> Self {
        let select = std::env::var("RENDERER_BACKEND").ok();
        let base = std::env::var("RENDERER_BACKEND_SELECT").ok();
        let debug = std::env::var("RENDERER_DEBUG").map(|v| v == "1").unwrap_or(false);
        Self {
            backend: select_backend(select.as_deref(), base.as_deref()),
            debug,
        }
    }
}

/// Chooses the backend from the environment.
///
/// The plugin exposes **one** backend option, `RENDERER_BACKEND`, so the launcher has a
/// single source of truth for it in its renderer settings. `RENDERER_BACKEND_SELECT` is
/// still accepted as a fallback because an already-installed launcher build may carry the
/// older key, but shipping both described the same setting twice.
///
/// An unparseable value is ignored rather than fatal, so a stale or hand-edited value cannot
/// stop the game from starting.
pub fn select_backend(primary: Option<&str>, legacy_select: Option<&str>) -> BackendKind {
    primary
        .and_then(BackendKind::parse)
        .or_else(|| legacy_select.and_then(BackendKind::parse))
        .unwrap_or(BackendKind::Auto)
}

#[derive(Clone, Debug, Default)]
pub struct DeviceInfo {
    pub vendor: String,
    pub renderer: String,
    pub api_version: String,
    pub glsl_version: String,
}

/// What the device actually supports, measured at init (never assumed).
#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub es_major: u32,
    pub es_minor: u32,
    pub extensions: Vec<String>,
    pub max_texture_size: i32,
    pub max_vertex_attribs: i32,
    pub max_draw_buffers: i32,
    pub max_color_attachments: i32,
    pub max_texture_units: i32,
    pub max_uniform_block_size: i32,
    pub max_samples: i32,
}

impl Capabilities {
    pub fn has_extension(&self, name: &str) -> bool {
        self.extensions.iter().any(|e| e == name)
    }
    pub fn at_least(&self, major: u32, minor: u32) -> bool {
        (self.es_major, self.es_minor) >= (major, minor)
    }
}

/// Parses "OpenGL ES 3.2 V@..." into (3, 2).
pub fn parse_es_version(s: &str) -> Option<(u32, u32)> {
    let rest = s.strip_prefix("OpenGL ES ")?;
    let v: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let mut it = v.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    Some((major, minor))
}

pub fn gl_error_name(code: u32) -> &'static str {
    match code {
        0 => "GL_NO_ERROR",
        0x0500 => "GL_INVALID_ENUM",
        0x0501 => "GL_INVALID_VALUE",
        0x0502 => "GL_INVALID_OPERATION",
        0x0505 => "GL_OUT_OF_MEMORY",
        0x0506 => "GL_INVALID_FRAMEBUFFER_OPERATION",
        _ => "GL_UNKNOWN_ERROR",
    }
}

#[derive(Debug)]
pub enum BackendError {
    /// Not implemented / not available. Never silently succeeds.
    Unsupported(String),
    InitFailed(String),
    /// A GL/Vulkan call failed; the string carries the operation and driver message.
    Gl(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(s) => write!(f, "unsupported: {s}"),
            Self::InitFailed(s) => write!(f, "init failed: {s}"),
            Self::Gl(s) => write!(f, "graphics error: {s}"),
        }
    }
}
impl std::error::Error for BackendError {}

macro_rules! handles {
    ($($n:ident),* $(,)?) => {
        $( #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub struct $n(pub u32); )*
    };
}
handles!(BufferId, TextureId, ShaderId, ProgramId, VertexArrayId, FramebufferId);

/// Resource-level backend interface (spec phase 2). Enum arguments are raw GL values for
/// now; a Vulkan backend will translate them.
pub trait Backend: Send + Sync {
    fn kind(&self) -> BackendKind;
    fn device_info(&self) -> &DeviceInfo;
    fn capabilities(&self) -> &Capabilities;

    /// Whether this backend can actually render frames.
    ///
    /// A backend may initialize (Vulkan present, device enumerated) without being able to
    /// draw: Vulkan detection alone is not a renderer. Selection uses this to fall back to
    /// a backend that can draw instead of reporting success for one that cannot.
    fn can_render(&self) -> bool {
        true
    }

    // --- frame / state ---
    fn clear_color(&self, r: f32, g: f32, b: f32, a: f32);
    fn clear(&self, mask: u32);
    fn viewport(&self, x: i32, y: i32, w: i32, h: i32);
    fn scissor(&self, x: i32, y: i32, w: i32, h: i32);
    fn enable(&self, cap: u32);
    fn disable(&self, cap: u32);
    fn blend_func(&self, src: u32, dst: u32);
    fn depth_func(&self, func: u32);
    fn depth_mask(&self, enabled: bool);
    fn cull_face(&self, mode: u32);

    // --- buffers ---
    fn create_buffer(&self) -> Result<BufferId, BackendError>;
    fn delete_buffer(&self, id: BufferId);
    fn bind_buffer(&self, target: u32, id: Option<BufferId>);
    fn buffer_data(&self, target: u32, data: &[u8], usage: u32) -> Result<(), BackendError>;
    fn buffer_sub_data(&self, target: u32, offset: usize, data: &[u8]) -> Result<(), BackendError>;

    // --- textures ---
    fn create_texture(&self) -> Result<TextureId, BackendError>;
    fn delete_texture(&self, id: TextureId);
    fn active_texture(&self, unit: u32);
    fn bind_texture(&self, target: u32, id: Option<TextureId>);
    #[allow(clippy::too_many_arguments)]
    fn tex_image_2d(
        &self,
        target: u32,
        level: i32,
        internal_format: i32,
        width: i32,
        height: i32,
        format: u32,
        ty: u32,
        data: Option<&[u8]>,
    ) -> Result<(), BackendError>;
    fn tex_parameter_i(&self, target: u32, pname: u32, value: i32);

    // --- shaders / programs ---
    fn compile_shader(&self, kind: u32, source: &str) -> Result<ShaderId, BackendError>;
    fn delete_shader(&self, id: ShaderId);
    fn link_program(&self, shaders: &[ShaderId]) -> Result<ProgramId, BackendError>;
    fn delete_program(&self, id: ProgramId);
    fn use_program(&self, id: Option<ProgramId>);
    fn uniform_location(&self, program: ProgramId, name: &str) -> Option<i32>;
    fn uniform_1i(&self, location: i32, v: i32);
    fn uniform_1f(&self, location: i32, v: f32);
    fn uniform_4f(&self, location: i32, x: f32, y: f32, z: f32, w: f32);
    fn uniform_matrix_4(&self, location: i32, m: &[f32; 16], transpose: bool);

    // --- vertex arrays ---
    fn create_vertex_array(&self) -> Result<VertexArrayId, BackendError>;
    fn delete_vertex_array(&self, id: VertexArrayId);
    fn bind_vertex_array(&self, id: Option<VertexArrayId>);
    fn vertex_attrib_pointer(&self, index: u32, size: i32, ty: u32, normalized: bool, stride: i32, offset: usize);
    fn set_vertex_attrib_enabled(&self, index: u32, enabled: bool);

    // --- framebuffers ---
    fn create_framebuffer(&self) -> Result<FramebufferId, BackendError>;
    fn delete_framebuffer(&self, id: FramebufferId);
    fn bind_framebuffer(&self, target: u32, id: Option<FramebufferId>);
    fn framebuffer_texture_2d(&self, target: u32, attachment: u32, tex_target: u32, tex: TextureId, level: i32);
    fn check_framebuffer_status(&self, target: u32) -> u32;

    // --- draws ---
    fn draw_arrays(&self, mode: u32, first: i32, count: i32);
    fn draw_elements(&self, mode: u32, count: i32, ty: u32, offset: usize);

    // --- diagnostics / interop ---
    /// Raw backend error (0 if none).
    fn get_error(&self) -> u32;
    /// NUL-terminated string owned by the backend/driver; null if unavailable.
    fn get_string(&self, name: u32) -> *const u8;
    /// Looks up a driver entry point by name (cached); null if the driver lacks it.
    fn proc_address(&self, name: &str) -> *const c_void;
}

/// OpenGL-style sticky error state (spec section 7). First error wins until read.
///
/// Relaxed ordering is enough and matters here: the game calls `glGetError` constantly, and
/// this is read-modify-written on every call. The only requirement is that the flag is not
/// lost between threads, which Release/Acquire provides; `SeqCst` bought nothing.
#[derive(Default)]
pub struct GlErrorState(AtomicU32);

impl GlErrorState {
    pub fn set(&self, e: u32) {
        let _ = self
            .0
            .compare_exchange(0, e, Ordering::Release, Ordering::Relaxed);
    }
    pub fn take(&self) -> u32 {
        self.0.swap(0, Ordering::Acquire)
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

    #[test]
    fn parses_backend_aliases_used_by_the_launcher() {
        // The launcher sends the exact value listed in the renderer options; the aliases
        // keep hand-edited env files working.
        assert_eq!(BackendKind::parse("gles"), Some(BackendKind::Gles));
        assert_eq!(BackendKind::parse("es"), Some(BackendKind::Gles));
        assert_eq!(BackendKind::parse("vk"), Some(BackendKind::Vulkan));
        assert_eq!(BackendKind::parse("hybrid"), Some(BackendKind::Hybrid));
        assert_eq!(BackendKind::parse("HYBRID"), Some(BackendKind::Hybrid));
        assert_eq!(BackendKind::parse("vk_es"), Some(BackendKind::Hybrid));
        assert_eq!(BackendKind::parse("zink"), None);
    }

    #[test]
    fn backend_names_round_trip() {
        for kind in [
            BackendKind::Auto,
            BackendKind::Gles,
            BackendKind::Vulkan,
            BackendKind::Hybrid,
        ] {
            assert_eq!(BackendKind::parse(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn the_single_backend_option_is_honoured() {
        for (value, want) in [
            ("gles", BackendKind::Gles),
            ("vulkan", BackendKind::Vulkan),
            ("hybrid", BackendKind::Hybrid),
            ("auto", BackendKind::Auto),
        ] {
            assert_eq!(select_backend(Some(value), None), want, "for {value}");
        }
    }

    #[test]
    fn an_older_launcher_key_still_works_as_a_fallback() {
        // An installed launcher build may still send RENDERER_BACKEND_SELECT.
        assert_eq!(select_backend(None, Some("hybrid")), BackendKind::Hybrid);
        // The current key wins when both are present.
        assert_eq!(select_backend(Some("vulkan"), Some("hybrid")), BackendKind::Vulkan);
    }

    #[test]
    fn unusable_values_fall_through_instead_of_failing() {
        assert_eq!(select_backend(Some(""), Some("vulkan")), BackendKind::Vulkan);
        assert_eq!(select_backend(Some("nonsense"), Some("gles")), BackendKind::Gles);
        assert_eq!(select_backend(None, None), BackendKind::Auto);
    }

    #[test]
    fn legacy_launcher_picker_beats_the_shipped_default() {
        // The plugin always ships RENDERER_BACKEND=gles; picking "hybrid" in the launcher
        // has to win, otherwise the option does nothing.
        assert_eq!(
            select_backend(Some("hybrid"), Some("gles")),
            BackendKind::Hybrid
        );
        assert_eq!(
            select_backend(Some("vulkan"), Some("gles")),
            BackendKind::Vulkan
        );
        // Absent picker (older launcher, or the value was not applied): use the default.
        assert_eq!(select_backend(None, Some("gles")), BackendKind::Gles);
        // A junk value must not stop the game from starting.
        assert_eq!(select_backend(Some("nonsense"), Some("gles")), BackendKind::Gles);
        // Nothing set at all.
        assert_eq!(select_backend(None, None), BackendKind::Auto);
    }

    #[test]
    fn an_empty_picker_falls_through_to_the_default() {
        assert_eq!(select_backend(Some(""), Some("vulkan")), BackendKind::Vulkan);
    }

    #[test]
    fn parses_es_versions() {
        assert_eq!(parse_es_version("OpenGL ES 3.2 V@0502.0 (GIT@abc)"), Some((3, 2)));
        assert_eq!(parse_es_version("OpenGL ES 3.0"), Some((3, 0)));
        assert_eq!(parse_es_version("4.6.0 NVIDIA"), None);
        assert_eq!(parse_es_version("OpenGL ES x"), None);
    }

    #[test]
    fn capability_queries() {
        let c = Capabilities {
            es_major: 3,
            es_minor: 1,
            extensions: vec!["GL_EXT_texture_format_BGRA8888".into()],
            ..Default::default()
        };
        assert!(c.at_least(3, 0));
        assert!(c.at_least(3, 1));
        assert!(!c.at_least(3, 2));
        assert!(c.has_extension("GL_EXT_texture_format_BGRA8888"));
        assert!(!c.has_extension("GL_FOO"));
    }

    #[test]
    fn error_names() {
        assert_eq!(gl_error_name(0x0502), "GL_INVALID_OPERATION");
        assert_eq!(gl_error_name(0x9999), "GL_UNKNOWN_ERROR");
    }
}
