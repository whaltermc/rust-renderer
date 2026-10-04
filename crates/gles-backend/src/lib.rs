//! OpenGL ES 3.0+ backend implementing the full resource-level `Backend` trait.
//! It does NOT create an EGL context; the launcher/GLFW layer owns the context, and this
//! backend must be created on a thread where one is current.

use libloading::Library;
use renderer_core::{
    gl_error_name, parse_es_version, Backend, BackendError, BackendKind, BufferId, Capabilities,
    DeviceInfo, FramebufferId, ProgramId, ShaderId, TextureId, VertexArrayId,
};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::Mutex;

const GL_EXTENSIONS: u32 = 0x1F03;
const GL_VENDOR: u32 = 0x1F00;
const GL_RENDERER: u32 = 0x1F01;
const GL_VERSION: u32 = 0x1F02;
const GL_SHADING_LANGUAGE_VERSION: u32 = 0x8B8C;
const GL_NUM_EXTENSIONS: u32 = 0x821D;
const GL_MAX_TEXTURE_SIZE: u32 = 0x0D33;
const GL_MAX_VERTEX_ATTRIBS: u32 = 0x8869;
const GL_MAX_DRAW_BUFFERS: u32 = 0x8824;
const GL_MAX_COLOR_ATTACHMENTS: u32 = 0x8CDF;
const GL_MAX_COMBINED_TEXTURE_IMAGE_UNITS: u32 = 0x8B4D;
const GL_MAX_UNIFORM_BLOCK_SIZE: u32 = 0x8A30;
const GL_MAX_SAMPLES: u32 = 0x8D57;
const GL_COMPILE_STATUS: u32 = 0x8B81;
const GL_LINK_STATUS: u32 = 0x8B82;
const GL_INFO_LOG_LENGTH: u32 = 0x8B84;

macro_rules! gl_fns {
    ($( $field:ident : $name:literal => fn($($t:ty),*) $(-> $r:ty)? ),* $(,)?) => {
        struct Fns { $( $field: unsafe extern "C" fn($($t),*) $(-> $r)? ),* }
        impl Fns {
            fn load(lib: &Library) -> Result<Self, BackendError> {
                Ok(Self { $( $field: {
                    // SAFETY: name and signature match the GLES 3.0 C ABI. The fn pointer is
                    // copied out; it stays valid because the Library is stored next to Fns
                    // in GlesBackend and never unloaded.
                    let s = unsafe { lib.get::<unsafe extern "C" fn($($t),*) $(-> $r)?>(concat!($name, "\0").as_bytes()) }
                        .map_err(|e| BackendError::InitFailed(format!("missing {}: {e}", $name)))?;
                    *s
                } ),* })
            }
        }
    };
}

gl_fns! {
    get_string: "glGetString" => fn(u32) -> *const u8,
    get_stringi: "glGetStringi" => fn(u32, u32) -> *const u8,
    get_integerv: "glGetIntegerv" => fn(u32, *mut i32),
    get_error: "glGetError" => fn() -> u32,
    clear_color: "glClearColor" => fn(f32, f32, f32, f32),
    clear: "glClear" => fn(u32),
    viewport: "glViewport" => fn(i32, i32, i32, i32),
    scissor: "glScissor" => fn(i32, i32, i32, i32),
    enable: "glEnable" => fn(u32),
    disable: "glDisable" => fn(u32),
    blend_func: "glBlendFunc" => fn(u32, u32),
    depth_func: "glDepthFunc" => fn(u32),
    depth_mask: "glDepthMask" => fn(u8),
    cull_face: "glCullFace" => fn(u32),
    gen_buffers: "glGenBuffers" => fn(i32, *mut u32),
    delete_buffers: "glDeleteBuffers" => fn(i32, *const u32),
    bind_buffer: "glBindBuffer" => fn(u32, u32),
    buffer_data: "glBufferData" => fn(u32, isize, *const c_void, u32),
    buffer_sub_data: "glBufferSubData" => fn(u32, isize, isize, *const c_void),
    gen_textures: "glGenTextures" => fn(i32, *mut u32),
    delete_textures: "glDeleteTextures" => fn(i32, *const u32),
    active_texture: "glActiveTexture" => fn(u32),
    bind_texture: "glBindTexture" => fn(u32, u32),
    tex_image_2d: "glTexImage2D" => fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    tex_parameter_i: "glTexParameteri" => fn(u32, u32, i32),
    create_shader: "glCreateShader" => fn(u32) -> u32,
    shader_source: "glShaderSource" => fn(u32, i32, *const *const c_char, *const i32),
    compile_shader: "glCompileShader" => fn(u32),
    get_shaderiv: "glGetShaderiv" => fn(u32, u32, *mut i32),
    get_shader_info_log: "glGetShaderInfoLog" => fn(u32, i32, *mut i32, *mut c_char),
    delete_shader: "glDeleteShader" => fn(u32),
    create_program: "glCreateProgram" => fn() -> u32,
    attach_shader: "glAttachShader" => fn(u32, u32),
    link_program: "glLinkProgram" => fn(u32),
    get_programiv: "glGetProgramiv" => fn(u32, u32, *mut i32),
    get_program_info_log: "glGetProgramInfoLog" => fn(u32, i32, *mut i32, *mut c_char),
    delete_program: "glDeleteProgram" => fn(u32),
    use_program: "glUseProgram" => fn(u32),
    get_uniform_location: "glGetUniformLocation" => fn(u32, *const c_char) -> i32,
    uniform_1i: "glUniform1i" => fn(i32, i32),
    uniform_1f: "glUniform1f" => fn(i32, f32),
    uniform_4f: "glUniform4f" => fn(i32, f32, f32, f32, f32),
    uniform_matrix_4fv: "glUniformMatrix4fv" => fn(i32, i32, u8, *const f32),
    gen_vertex_arrays: "glGenVertexArrays" => fn(i32, *mut u32),
    delete_vertex_arrays: "glDeleteVertexArrays" => fn(i32, *const u32),
    bind_vertex_array: "glBindVertexArray" => fn(u32),
    vertex_attrib_pointer: "glVertexAttribPointer" => fn(u32, i32, u32, u8, i32, *const c_void),
    enable_vertex_attrib_array: "glEnableVertexAttribArray" => fn(u32),
    disable_vertex_attrib_array: "glDisableVertexAttribArray" => fn(u32),
    gen_framebuffers: "glGenFramebuffers" => fn(i32, *mut u32),
    delete_framebuffers: "glDeleteFramebuffers" => fn(i32, *const u32),
    bind_framebuffer: "glBindFramebuffer" => fn(u32, u32),
    framebuffer_texture_2d: "glFramebufferTexture2D" => fn(u32, u32, u32, u32, i32),
    check_framebuffer_status: "glCheckFramebufferStatus" => fn(u32) -> u32,
    draw_arrays: "glDrawArrays" => fn(u32, i32, i32),
    draw_elements: "glDrawElements" => fn(u32, i32, u32, *const c_void),
}

pub struct GlesBackend {
    // Keeps the driver mapped for as long as `fns` pointers exist.
    lib: Library,
    fns: Fns,
    info: DeviceInfo,
    caps: Capabilities,
    proc_cache: Mutex<HashMap<String, usize>>,
}

impl GlesBackend {
    /// Loads the driver, verifies OpenGL ES 3.0+, and measures capabilities.
    ///
    /// Requires a current GLES context on the calling thread; otherwise `glGetString`
    /// returns null and this fails with `InitFailed` instead of pretending it worked.
    pub fn new() -> Result<Self, BackendError> {
        // SAFETY: loading a system library runs its initializers. libGLESv3/v2 are platform
        // libraries with no unsound constructors; failure is handled.
        let lib = unsafe { Library::new("libGLESv3.so").or_else(|_| Library::new("libGLESv2.so")) }
            .map_err(|e| BackendError::InitFailed(format!("cannot load GLES driver: {e}")))?;
        let fns = Fns::load(&lib)?;

        let read = |name: u32| -> Result<String, BackendError> {
            // SAFETY: valid fn pointer; a current context is verified via the null check.
            let p = unsafe { (fns.get_string)(name) };
            if p.is_null() {
                return Err(BackendError::InitFailed(
                    "glGetString returned null (no current GLES context?)".into(),
                ));
            }
            // SAFETY: GL returns a NUL-terminated string that outlives this call.
            Ok(unsafe { CStr::from_ptr(p as *const c_char) }.to_string_lossy().into_owned())
        };
        let info = DeviceInfo {
            vendor: read(GL_VENDOR)?,
            renderer: read(GL_RENDERER)?,
            api_version: read(GL_VERSION)?,
            glsl_version: read(GL_SHADING_LANGUAGE_VERSION)?,
        };

        let (es_major, es_minor) = parse_es_version(&info.api_version).ok_or_else(|| {
            BackendError::InitFailed(format!("unrecognized GL version string: {}", info.api_version))
        })?;
        if es_major < 3 {
            return Err(BackendError::InitFailed(format!(
                "OpenGL ES 3.0+ required, driver reports {es_major}.{es_minor}"
            )));
        }

        let geti = |p: u32| -> i32 {
            let mut v = 0i32;
            // SAFETY: valid fn pointer; `v` outlives the call.
            unsafe { (fns.get_integerv)(p, &mut v) };
            v
        };
        let n_ext = geti(GL_NUM_EXTENSIONS).max(0) as u32;
        let mut extensions = Vec::with_capacity(n_ext as usize);
        for i in 0..n_ext {
            // SAFETY: valid fn pointer, index < GL_NUM_EXTENSIONS; null-checked below.
            let p = unsafe { (fns.get_stringi)(GL_EXTENSIONS, i) };
            if !p.is_null() {
                // SAFETY: NUL-terminated driver string.
                extensions.push(unsafe { CStr::from_ptr(p as *const c_char) }.to_string_lossy().into_owned());
            }
        }
        let caps = Capabilities {
            es_major,
            es_minor,
            extensions,
            max_texture_size: geti(GL_MAX_TEXTURE_SIZE),
            max_vertex_attribs: geti(GL_MAX_VERTEX_ATTRIBS),
            max_draw_buffers: geti(GL_MAX_DRAW_BUFFERS),
            max_color_attachments: geti(GL_MAX_COLOR_ATTACHMENTS),
            max_texture_units: geti(GL_MAX_COMBINED_TEXTURE_IMAGE_UNITS),
            max_uniform_block_size: geti(GL_MAX_UNIFORM_BLOCK_SIZE),
            max_samples: geti(GL_MAX_SAMPLES),
        };

        Ok(Self { lib, fns, info, caps, proc_cache: Mutex::new(HashMap::new()) })
    }

    fn check(&self, op: &str) -> Result<(), BackendError> {
        // SAFETY: valid fn pointer.
        let e = unsafe { (self.fns.get_error)() };
        if e == 0 {
            Ok(())
        } else {
            Err(BackendError::Gl(format!("{op}: {}", gl_error_name(e))))
        }
    }

    fn shader_log(&self, id: u32) -> String {
        let mut len = 0i32;
        // SAFETY: valid fn pointers; the buffer is sized from the driver-reported length.
        unsafe {
            (self.fns.get_shaderiv)(id, GL_INFO_LOG_LENGTH, &mut len);
            if len <= 1 {
                return String::new();
            }
            let mut buf = vec![0u8; len as usize];
            let mut written = 0i32;
            (self.fns.get_shader_info_log)(id, len, &mut written, buf.as_mut_ptr() as *mut c_char);
            buf.truncate(written.max(0) as usize);
            String::from_utf8_lossy(&buf).into_owned()
        }
    }

    fn program_log(&self, id: u32) -> String {
        let mut len = 0i32;
        // SAFETY: as in `shader_log`.
        unsafe {
            (self.fns.get_programiv)(id, GL_INFO_LOG_LENGTH, &mut len);
            if len <= 1 {
                return String::new();
            }
            let mut buf = vec![0u8; len as usize];
            let mut written = 0i32;
            (self.fns.get_program_info_log)(id, len, &mut written, buf.as_mut_ptr() as *mut c_char);
            buf.truncate(written.max(0) as usize);
            String::from_utf8_lossy(&buf).into_owned()
        }
    }

    /// Generates one name with a `glGen*` style function.
    fn gen_one(&self, f: unsafe extern "C" fn(i32, *mut u32), what: &str) -> Result<u32, BackendError> {
        let mut id = 0u32;
        // SAFETY: valid fn pointer; `id` outlives the call.
        unsafe { f(1, &mut id) };
        self.check(what)?;
        if id == 0 {
            return Err(BackendError::Gl(format!("{what} returned 0")));
        }
        Ok(id)
    }
}

// SAFETY: only fn pointers, an immutable library handle, plain data and a Mutex are held.
// GL calls must still be made on the context's thread (caller's responsibility, spec 24).
unsafe impl Send for GlesBackend {}
unsafe impl Sync for GlesBackend {}

impl Backend for GlesBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Gles
    }
    fn device_info(&self) -> &DeviceInfo {
        &self.info
    }
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn clear_color(&self, r: f32, g: f32, b: f32, a: f32) {
        // SAFETY (all state calls below): valid fn pointers, plain value arguments.
        unsafe { (self.fns.clear_color)(r, g, b, a) }
    }
    fn clear(&self, mask: u32) {
        unsafe { (self.fns.clear)(mask) }
    }
    fn viewport(&self, x: i32, y: i32, w: i32, h: i32) {
        unsafe { (self.fns.viewport)(x, y, w, h) }
    }
    fn scissor(&self, x: i32, y: i32, w: i32, h: i32) {
        unsafe { (self.fns.scissor)(x, y, w, h) }
    }
    fn enable(&self, cap: u32) {
        unsafe { (self.fns.enable)(cap) }
    }
    fn disable(&self, cap: u32) {
        unsafe { (self.fns.disable)(cap) }
    }
    fn blend_func(&self, src: u32, dst: u32) {
        unsafe { (self.fns.blend_func)(src, dst) }
    }
    fn depth_func(&self, func: u32) {
        unsafe { (self.fns.depth_func)(func) }
    }
    fn depth_mask(&self, enabled: bool) {
        unsafe { (self.fns.depth_mask)(enabled as u8) }
    }
    fn cull_face(&self, mode: u32) {
        unsafe { (self.fns.cull_face)(mode) }
    }

    fn create_buffer(&self) -> Result<BufferId, BackendError> {
        self.gen_one(self.fns.gen_buffers, "glGenBuffers").map(BufferId)
    }
    fn delete_buffer(&self, id: BufferId) {
        // SAFETY: valid fn pointer; pointer to one live u32.
        unsafe { (self.fns.delete_buffers)(1, &id.0) }
    }
    fn bind_buffer(&self, target: u32, id: Option<BufferId>) {
        unsafe { (self.fns.bind_buffer)(target, id.map_or(0, |i| i.0)) }
    }
    fn buffer_data(&self, target: u32, data: &[u8], usage: u32) -> Result<(), BackendError> {
        // SAFETY: pointer/length come from a live slice.
        unsafe { (self.fns.buffer_data)(target, data.len() as isize, data.as_ptr() as *const c_void, usage) };
        self.check("glBufferData")
    }
    fn buffer_sub_data(&self, target: u32, offset: usize, data: &[u8]) -> Result<(), BackendError> {
        // SAFETY: pointer/length come from a live slice.
        unsafe {
            (self.fns.buffer_sub_data)(target, offset as isize, data.len() as isize, data.as_ptr() as *const c_void)
        };
        self.check("glBufferSubData")
    }

    fn create_texture(&self) -> Result<TextureId, BackendError> {
        self.gen_one(self.fns.gen_textures, "glGenTextures").map(TextureId)
    }
    fn delete_texture(&self, id: TextureId) {
        // SAFETY: valid fn pointer; pointer to one live u32.
        unsafe { (self.fns.delete_textures)(1, &id.0) }
    }
    fn active_texture(&self, unit: u32) {
        unsafe { (self.fns.active_texture)(unit) }
    }
    fn bind_texture(&self, target: u32, id: Option<TextureId>) {
        unsafe { (self.fns.bind_texture)(target, id.map_or(0, |i| i.0)) }
    }
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
    ) -> Result<(), BackendError> {
        if width < 0 || height < 0 || width > self.caps.max_texture_size || height > self.caps.max_texture_size {
            return Err(BackendError::Gl(format!(
                "glTexImage2D: size {width}x{height} outside 0..={}",
                self.caps.max_texture_size
            )));
        }
        let ptr = data.map_or(std::ptr::null(), |d| d.as_ptr() as *const c_void);
        // SAFETY: ptr is null (allocate only) or points into a live slice. The caller is
        // responsible for the slice being large enough for width*height*format (GL contract).
        unsafe { (self.fns.tex_image_2d)(target, level, internal_format, width, height, 0, format, ty, ptr) };
        self.check("glTexImage2D")
    }
    fn tex_parameter_i(&self, target: u32, pname: u32, value: i32) {
        unsafe { (self.fns.tex_parameter_i)(target, pname, value) }
    }

    fn compile_shader(&self, kind: u32, source: &str) -> Result<ShaderId, BackendError> {
        let c = CString::new(source)
            .map_err(|_| BackendError::Unsupported("shader source contains a NUL byte".into()))?;
        // SAFETY: valid fn pointers; `c` outlives the calls; one source string, no length array.
        unsafe {
            let id = (self.fns.create_shader)(kind);
            if id == 0 {
                self.check("glCreateShader")?;
                return Err(BackendError::Gl("glCreateShader returned 0".into()));
            }
            let p = c.as_ptr();
            (self.fns.shader_source)(id, 1, &p, std::ptr::null());
            (self.fns.compile_shader)(id);
            let mut ok = 0i32;
            (self.fns.get_shaderiv)(id, GL_COMPILE_STATUS, &mut ok);
            if ok == 0 {
                let log = self.shader_log(id);
                (self.fns.delete_shader)(id);
                return Err(BackendError::Gl(format!("shader compile failed: {log}")));
            }
            Ok(ShaderId(id))
        }
    }
    fn delete_shader(&self, id: ShaderId) {
        unsafe { (self.fns.delete_shader)(id.0) }
    }
    fn link_program(&self, shaders: &[ShaderId]) -> Result<ProgramId, BackendError> {
        // SAFETY: valid fn pointers, plain integer arguments.
        unsafe {
            let prog = (self.fns.create_program)();
            if prog == 0 {
                self.check("glCreateProgram")?;
                return Err(BackendError::Gl("glCreateProgram returned 0".into()));
            }
            for s in shaders {
                (self.fns.attach_shader)(prog, s.0);
            }
            (self.fns.link_program)(prog);
            let mut ok = 0i32;
            (self.fns.get_programiv)(prog, GL_LINK_STATUS, &mut ok);
            if ok == 0 {
                let log = self.program_log(prog);
                (self.fns.delete_program)(prog);
                return Err(BackendError::Gl(format!("program link failed: {log}")));
            }
            Ok(ProgramId(prog))
        }
    }
    fn delete_program(&self, id: ProgramId) {
        unsafe { (self.fns.delete_program)(id.0) }
    }
    fn use_program(&self, id: Option<ProgramId>) {
        unsafe { (self.fns.use_program)(id.map_or(0, |i| i.0)) }
    }
    fn uniform_location(&self, program: ProgramId, name: &str) -> Option<i32> {
        let c = CString::new(name).ok()?;
        // SAFETY: valid fn pointer; `c` is NUL-terminated and outlives the call.
        let loc = unsafe { (self.fns.get_uniform_location)(program.0, c.as_ptr()) };
        if loc < 0 {
            None
        } else {
            Some(loc)
        }
    }
    fn uniform_1i(&self, location: i32, v: i32) {
        unsafe { (self.fns.uniform_1i)(location, v) }
    }
    fn uniform_1f(&self, location: i32, v: f32) {
        unsafe { (self.fns.uniform_1f)(location, v) }
    }
    fn uniform_4f(&self, location: i32, x: f32, y: f32, z: f32, w: f32) {
        unsafe { (self.fns.uniform_4f)(location, x, y, z, w) }
    }
    fn uniform_matrix_4(&self, location: i32, m: &[f32; 16], transpose: bool) {
        // SAFETY: `m` is exactly 16 floats = one 4x4 matrix.
        unsafe { (self.fns.uniform_matrix_4fv)(location, 1, transpose as u8, m.as_ptr()) }
    }

    fn create_vertex_array(&self) -> Result<VertexArrayId, BackendError> {
        self.gen_one(self.fns.gen_vertex_arrays, "glGenVertexArrays").map(VertexArrayId)
    }
    fn delete_vertex_array(&self, id: VertexArrayId) {
        // SAFETY: valid fn pointer; pointer to one live u32.
        unsafe { (self.fns.delete_vertex_arrays)(1, &id.0) }
    }
    fn bind_vertex_array(&self, id: Option<VertexArrayId>) {
        unsafe { (self.fns.bind_vertex_array)(id.map_or(0, |i| i.0)) }
    }
    fn vertex_attrib_pointer(&self, index: u32, size: i32, ty: u32, normalized: bool, stride: i32, offset: usize) {
        // SAFETY: with a buffer bound to GL_ARRAY_BUFFER the "pointer" is a byte offset, which
        // is how GLES 3 core uses it; no memory is dereferenced here.
        unsafe { (self.fns.vertex_attrib_pointer)(index, size, ty, normalized as u8, stride, offset as *const c_void) }
    }
    fn set_vertex_attrib_enabled(&self, index: u32, enabled: bool) {
        unsafe {
            if enabled {
                (self.fns.enable_vertex_attrib_array)(index)
            } else {
                (self.fns.disable_vertex_attrib_array)(index)
            }
        }
    }

    fn create_framebuffer(&self) -> Result<FramebufferId, BackendError> {
        self.gen_one(self.fns.gen_framebuffers, "glGenFramebuffers").map(FramebufferId)
    }
    fn delete_framebuffer(&self, id: FramebufferId) {
        // SAFETY: valid fn pointer; pointer to one live u32.
        unsafe { (self.fns.delete_framebuffers)(1, &id.0) }
    }
    fn bind_framebuffer(&self, target: u32, id: Option<FramebufferId>) {
        unsafe { (self.fns.bind_framebuffer)(target, id.map_or(0, |i| i.0)) }
    }
    fn framebuffer_texture_2d(&self, target: u32, attachment: u32, tex_target: u32, tex: TextureId, level: i32) {
        unsafe { (self.fns.framebuffer_texture_2d)(target, attachment, tex_target, tex.0, level) }
    }
    fn check_framebuffer_status(&self, target: u32) -> u32 {
        unsafe { (self.fns.check_framebuffer_status)(target) }
    }

    fn draw_arrays(&self, mode: u32, first: i32, count: i32) {
        unsafe { (self.fns.draw_arrays)(mode, first, count) }
    }
    fn draw_elements(&self, mode: u32, count: i32, ty: u32, offset: usize) {
        // SAFETY: with an element buffer bound the "pointer" is a byte offset; not dereferenced here.
        unsafe { (self.fns.draw_elements)(mode, count, ty, offset as *const c_void) }
    }

    fn get_error(&self) -> u32 {
        unsafe { (self.fns.get_error)() }
    }
    fn get_string(&self, name: u32) -> *const u8 {
        unsafe { (self.fns.get_string)(name) }
    }
    fn proc_address(&self, name: &str) -> *const c_void {
        let mut cache = match self.proc_cache.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(&addr) = cache.get(name) {
            return addr as *const c_void;
        }
        let mut n = name.as_bytes().to_vec();
        n.push(0);
        // SAFETY: the symbol is only read as an opaque address and never called through this
        // type; the library stays mapped for the lifetime of `self`.
        let addr = match unsafe { self.lib.get::<unsafe extern "C" fn()>(&n) } {
            Ok(sym) => *sym as usize,
            Err(_) => 0,
        };
        cache.insert(name.to_string(), addr);
        addr as *const c_void
    }
}
