//! DirectES — Direct OpenGL ES 3.x backend.
//!
//! Bypasses the GL compatibility layer for maximum performance on devices
//! that natively support OpenGL ES 3.x. DirectES targets:
//! - Mali (ARM)
//! - Adreno (Qualcomm)
//! - PowerVR (Imagination)
//! - Vivante
//! - NVIDIA Tegra
//!
//! It implements the full `Backend` trait directly against the ES driver,
//! with no desktop-GL translation or shader rewriting.

use renderer_core::{
    gl_error_name, parse_es_version, Backend, BackendError, BackendKind, BufferId, Capabilities,
    DeviceInfo, FramebufferId, ProgramId, ShaderId, TextureId, VertexArrayId,
};
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::Mutex;
use libloading::Library;

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
            fn load(lib: &libloading::Library) -> Result<Self, BackendError> {
                Ok(Self { $( $field: {
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
    gen_queries: "glGenQueries" => fn(i32, *mut u32),
    begin_query: "glBeginQuery" => fn(u32, u32),
    end_query: "glEndQuery" => fn(u32),
    get_query_object_iv: "glGetQueryObjectiv" => fn(u32, u32, *mut i32),
    delete_queries: "glDeleteQueries" => fn(i32, *const u32),
    fence_sync: "glFenceSync" => fn(u32, u32) -> *mut c_void,
    client_wait_sync: "glClientWaitSync" => fn(*mut c_void, u32, u64) -> u32,
    delete_sync: "glDeleteSync" => fn(*mut c_void),
    draw_buffers: "glDrawBuffers" => fn(i32, *const u32),
    read_buffer: "glReadBuffer" => fn(u32),
    blit_framebuffer: "glBlitFramebuffer" => fn(i32, i32, i32, i32, i32, i32, i32, i32, u32, u32),
    renderbuffer_storage_multisample: "glRenderbufferStorageMultisample" => fn(i32, u32, u32, u32, i32, i32),
    gen_renderbuffers: "glGenRenderbuffers" => fn(i32, *mut u32),
    bind_renderbuffer: "glBindRenderbuffer" => fn(u32, u32),
    framebuffer_renderbuffer: "glFramebufferRenderbuffer" => fn(u32, u32, u32, u32),
    pixel_storei: "glPixelStorei" => fn(u32, i32),
    read_pixels: "glReadPixels" => fn(i32, i32, i32, i32, u32, u32, *mut c_void),
}

pub struct DirectEsBackend {
    lib: libloading::Library,
    fns: Fns,
    info: DeviceInfo,
    caps: Capabilities,
    proc_cache: Mutex<HashMap<String, usize>>,
}

impl DirectEsBackend {
    pub fn new() -> Result<Self, BackendError> {
        let lib = unsafe { libloading::Library::new("libGLESv3.so") }
            .or_else(|_| unsafe { libloading::Library::new("libGLESv2.so") })
            .map_err(|e| BackendError::InitFailed(format!("cannot load GLES driver: {e}")))?;
        let fns = Fns::load(&lib)?;

        let read = |name: u32| -> Result<String, BackendError> {
            let p = unsafe { (fns.get_string)(name) };
            if p.is_null() {
                return Err(BackendError::InitFailed(
                    "glGetString returned null (no current GLES context?)".into(),
                ));
            }
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
            unsafe { (fns.get_integerv)(p, &mut v) };
            v
        };
        let n_ext = geti(GL_NUM_EXTENSIONS).max(0) as u32;
        let mut extensions = Vec::with_capacity(n_ext as usize);
        for i in 0..n_ext {
            let p = unsafe { (fns.get_stringi)(GL_EXTENSIONS, i) };
            if !p.is_null() {
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

        Ok(Self {
            lib,
            fns,
            info,
            caps,
            proc_cache: Mutex::new(HashMap::new()),
        })
    }

    fn check(&self, op: &str) -> Result<(), BackendError> {
        let e = unsafe { (self.fns.get_error)() };
        if e == 0 {
            Ok(())
        } else {
            Err(BackendError::Gl(format!("{op}: {}", gl_error_name(e))))
        }
    }
}

// SAFETY: only fn pointers, an immutable library handle, plain data and a Mutex are held.
unsafe impl Send for DirectEsBackend {}
unsafe impl Sync for DirectEsBackend {}

impl Backend for DirectEsBackend {
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
        let mut id = 0u32;
        unsafe { (self.fns.gen_buffers)(1, &mut id) };
        self.check("glGenBuffers")?;
        if id == 0 {
            return Err(BackendError::Gl("glGenBuffers returned 0".into()));
        }
        Ok(BufferId(id))
    }
    fn delete_buffer(&self, id: BufferId) {
        unsafe { (self.fns.delete_buffers)(1, &id.0) }
    }
    fn bind_buffer(&self, target: u32, id: Option<BufferId>) {
        unsafe { (self.fns.bind_buffer)(target, id.map_or(0, |i| i.0)) }
    }
    fn buffer_data(&self, target: u32, data: &[u8], usage: u32) -> Result<(), BackendError> {
        unsafe { (self.fns.buffer_data)(target, data.len() as isize, data.as_ptr() as *const c_void, usage) };
        self.check("glBufferData")
    }
    fn buffer_sub_data(&self, target: u32, offset: usize, data: &[u8]) -> Result<(), BackendError> {
        unsafe { (self.fns.buffer_sub_data)(target, offset as isize, data.len() as isize, data.as_ptr() as *const c_void) };
        self.check("glBufferSubData")
    }

    fn create_texture(&self) -> Result<TextureId, BackendError> {
        let mut id = 0u32;
        unsafe { (self.fns.gen_textures)(1, &mut id) };
        self.check("glGenTextures")?;
        if id == 0 {
            return Err(BackendError::Gl("glGenTextures returned 0".into()));
        }
        Ok(TextureId(id))
    }
    fn delete_texture(&self, id: TextureId) {
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
        let ptr = data.map_or(std::ptr::null(), |d| d.as_ptr() as *const c_void);
        unsafe { (self.fns.tex_image_2d)(target, level, internal_format, width, height, 0, format, ty, ptr) };
        self.check("glTexImage2D")
    }
    fn tex_parameter_i(&self, target: u32, pname: u32, value: i32) {
        unsafe { (self.fns.tex_parameter_i)(target, pname, value) }
    }

    fn compile_shader(&self, kind: u32, source: &str) -> Result<ShaderId, BackendError> {
        let c = CString::new(source)
            .map_err(|_| BackendError::Unsupported("shader source contains a NUL byte".into()))?;
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
        let loc = unsafe { (self.fns.get_uniform_location)(program.0, c.as_ptr()) };
        if loc < 0 { None } else { Some(loc) }
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
        unsafe { (self.fns.uniform_matrix_4fv)(location, 1, transpose as u8, m.as_ptr()) }
    }

    fn create_vertex_array(&self) -> Result<VertexArrayId, BackendError> {
        let mut id = 0u32;
        unsafe { (self.fns.gen_vertex_arrays)(1, &mut id) };
        self.check("glGenVertexArrays")?;
        if id == 0 {
            return Err(BackendError::Gl("glGenVertexArrays returned 0".into()));
        }
        Ok(VertexArrayId(id))
    }
    fn delete_vertex_array(&self, id: VertexArrayId) {
        unsafe { (self.fns.delete_vertex_arrays)(1, &id.0) }
    }
    fn bind_vertex_array(&self, id: Option<VertexArrayId>) {
        unsafe { (self.fns.bind_vertex_array)(id.map_or(0, |i| i.0)) }
    }
    fn vertex_attrib_pointer(&self, index: u32, size: i32, ty: u32, normalized: bool, stride: i32, offset: usize) {
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
        let mut id = 0u32;
        unsafe { (self.fns.gen_framebuffers)(1, &mut id) };
        self.check("glGenFramebuffers")?;
        if id == 0 {
            return Err(BackendError::Gl("glGenFramebuffers returned 0".into()));
        }
        Ok(FramebufferId(id))
    }
    fn delete_framebuffer(&self, id: FramebufferId) {
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
        let addr = match unsafe { self.lib.get::<unsafe extern "C" fn()>(&n) } {
            Ok(sym) => *sym as usize,
            Err(_) => 0,
        };
        cache.insert(name.to_string(), addr);
        addr as *const c_void
    }

    fn read_pixels(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: u32,
        ty: u32,
        pixels: &mut [u8],
    ) -> Result<(), BackendError> {
        // SAFETY: valid fn pointer; `pixels` outlives the call.
        unsafe {
            (self.fns.pixel_storei)(0x0CF5 /* GL_PACK_ALIGNMENT */, 1);
            (self.fns.read_pixels)(x, y, width, height, format, ty, pixels.as_mut_ptr() as *mut c_void);
        }
        self.check("glReadPixels")
    }
}

impl DirectEsBackend {
    fn shader_log(&self, id: u32) -> String {
        let mut len = 0i32;
        unsafe {
            (self.fns.get_shaderiv)(id, GL_INFO_LOG_LENGTH, &mut len);
            if len <= 1 { return String::new(); }
            let mut buf = vec![0u8; len as usize];
            let mut written = 0i32;
            (self.fns.get_shader_info_log)(id, len, &mut written, buf.as_mut_ptr() as *mut c_char);
            buf.truncate(written.max(0) as usize);
            String::from_utf8_lossy(&buf).into_owned()
        }
    }
    fn program_log(&self, id: u32) -> String {
        let mut len = 0i32;
        unsafe {
            (self.fns.get_programiv)(id, GL_INFO_LOG_LENGTH, &mut len);
            if len <= 1 { return String::new(); }
            let mut buf = vec![0u8; len as usize];
            let mut written = 0i32;
            (self.fns.get_program_info_log)(id, len, &mut written, buf.as_mut_ptr() as *mut c_char);
            buf.truncate(written.max(0) as usize);
            String::from_utf8_lossy(&buf).into_owned()
        }
    }
}

/// Probe for DirectES.
pub fn probe() -> Result<Box<dyn Backend>, BackendError> {
    let backend = DirectEsBackend::new()?;
    Ok(Box::new(backend))
}
