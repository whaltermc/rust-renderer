//! C-ABI OpenGL surface loaded by the launcher as the "GL library".
//!
//! Phase 1 scope: a handful of core entry points over the GLES backend with OpenGL error
//! semantics. It reports the driver's REAL (GLES) strings. It does NOT translate desktop GL,
//! so Minecraft Java will not run on this yet (spec phases 3-4).

use renderer_core::{Backend, BackendKind, Config, GlErrorState};
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::{Mutex, OnceLock};

const GL_INVALID_VALUE: u32 = 0x0501;
const GL_INVALID_OPERATION: u32 = 0x0502;

static BACKEND: OnceLock<Box<dyn Backend>> = OnceLock::new();
static ERRORS: OnceLock<GlErrorState> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());

fn errors() -> &'static GlErrorState {
    ERRORS.get_or_init(GlErrorState::default)
}

fn log(msg: &str) {
    #[cfg(target_os = "android")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            android_logger::init_once(
                android_logger::Config::default()
                    .with_tag("RustRenderer")
                    .with_max_level(log::LevelFilter::Info),
            )
        });
        log::info!("{msg}");
    }
    #[cfg(not(target_os = "android"))]
    eprintln!("[RustRenderer] {msg}");
}

/// Lazily selects/initializes a backend on the first GL call (a context must be current by
/// then). Never selects a backend that fails to initialize.
fn backend() -> Option<&'static dyn Backend> {
    if let Some(b) = BACKEND.get() {
        return Some(b.as_ref());
    }
    let _g = INIT_LOCK.lock().ok()?;
    if let Some(b) = BACKEND.get() {
        return Some(b.as_ref());
    }
    let cfg = Config::from_env();
    log(&format!("[Renderer] Initializing, requested backend: {:?}", cfg.backend));

    let mut chosen: Option<Box<dyn Backend>> = None;
    if matches!(cfg.backend, BackendKind::Auto | BackendKind::Vulkan) {
        match vulkan_backend::probe() {
            Ok(b) => chosen = Some(b),
            Err(e) => log(&format!("[Vulkan] {e}")),
        }
    }
    if chosen.is_none() && matches!(cfg.backend, BackendKind::Auto | BackendKind::Gles) {
        match gles_backend::GlesBackend::new() {
            Ok(b) => chosen = Some(Box::new(b)),
            Err(e) => log(&format!("[GLES] {e}")),
        }
    }
    match chosen {
        Some(b) => {
            let i = b.device_info();
            log(&format!("[Renderer] Backend: {:?}", b.kind()));
            log(&format!("[Renderer] GPU: {} ({})", i.renderer, i.vendor));
            log(&format!("[Renderer] API: {}", i.api_version));
            let c = b.capabilities();
            log(&format!(
                "[Renderer] Caps: ES {}.{}, {} extensions, max texture {}, {} draw buffers",
                c.es_major, c.es_minor, c.extensions.len(), c.max_texture_size, c.max_draw_buffers
            ));
            let _ = BACKEND.set(b);
            BACKEND.get().map(|b| b.as_ref())
        }
        None => {
            log("[Renderer] No backend could be initialized");
            None
        }
    }
}

#[no_mangle]
pub extern "C" fn glGetError() -> u32 {
    let ours = errors().take();
    if ours != 0 {
        return ours;
    }
    backend().map(|b| b.get_error()).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn glClearColor(r: f32, g: f32, b: f32, a: f32) {
    match backend() {
        Some(be) => be.clear_color(r, g, b, a),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub extern "C" fn glClear(mask: u32) {
    match backend() {
        Some(be) => be.clear(mask),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub extern "C" fn glViewport(x: i32, y: i32, w: i32, h: i32) {
    if w < 0 || h < 0 {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    match backend() {
        Some(be) => be.viewport(x, y, w, h),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub extern "C" fn glEnable(cap: u32) {
    match backend() {
        Some(be) => be.enable(cap),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub extern "C" fn glDisable(cap: u32) {
    match backend() {
        Some(be) => be.disable(cap),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

const GL_VERSION: u32 = 0x1F02;
const GL_SHADING_LANGUAGE_VERSION: u32 = 0x8B8C;
const GL_MAJOR_VERSION: u32 = 0x821B;
const GL_MINOR_VERSION: u32 = 0x821C;

static SPOOF_VERSION: &[u8] = b"3.2 (Core Profile) RustRenderer GLES passthrough\0";
static SPOOF_GLSL: &[u8] = b"1.50\0";

/// OPT-IN, EXPERIMENTAL: `RENDERER_SPOOF_GL=1` makes the renderer claim OpenGL 3.2 core.
/// The claim is NOT backed by a full implementation. Off by default (spec: never advertise
/// unsupported features).
fn spoof_gl() -> bool {
    static S: OnceLock<bool> = OnceLock::new();
    *S.get_or_init(|| {
        let on = std::env::var("RENDERER_SPOOF_GL").map(|v| v == "1").unwrap_or(false);
        if on {
            log("[GLCompat] WARNING: RENDERER_SPOOF_GL=1, advertising GL 3.2 without full support");
        }
        on
    })
}

/// Resolves a driver function pointer of type `T` (must be a fn pointer type).
unsafe fn driver_fn<T: Copy>(name: &str) -> Option<T> {
    assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*const c_void>());
    let be = backend()?;
    let p = be.proc_address(name);
    if p.is_null() {
        None
    } else {
        // SAFETY: caller guarantees T is the fn-pointer type matching `name`'s C prototype.
        Some(std::mem::transmute_copy::<*const c_void, T>(&p))
    }
}

#[no_mangle]
pub extern "C" fn glGetString(name: u32) -> *const u8 {
    if spoof_gl() {
        match name {
            GL_VERSION => return SPOOF_VERSION.as_ptr(),
            GL_SHADING_LANGUAGE_VERSION => return SPOOF_GLSL.as_ptr(),
            _ => {}
        }
    }
    match backend() {
        Some(be) => be.get_string(name),
        None => {
            errors().set(GL_INVALID_OPERATION);
            std::ptr::null()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetIntegerv(pname: u32, data: *mut i32) {
    if spoof_gl() && !data.is_null() {
        match pname {
            GL_MAJOR_VERSION => {
                *data = 3;
                return;
            }
            GL_MINOR_VERSION => {
                *data = 2;
                return;
            }
            _ => {}
        }
    }
    match driver_fn::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv") {
        Some(f) => f(pname, data),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetStringi(name: u32, index: u32) -> *const u8 {
    match driver_fn::<unsafe extern "C" fn(u32, u32) -> *const u8>("glGetStringi") {
        Some(f) => f(name, index),
        None => {
            errors().set(GL_INVALID_OPERATION);
            std::ptr::null()
        }
    }
}

/// Desktop `glClearDepth(double)` -> ES `glClearDepthf(float)`.
#[no_mangle]
pub unsafe extern "C" fn glClearDepth(depth: f64) {
    match driver_fn::<unsafe extern "C" fn(f32)>("glClearDepthf") {
        Some(f) => f(depth as f32),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

/// Joins the source strings, rewrites desktop GLSL to GLSL ES, and hands one string to the driver.
#[no_mangle]
pub unsafe extern "C" fn glShaderSource(
    shader: u32,
    count: i32,
    string: *const *const c_char,
    length: *const i32,
) {
    if count < 0 || (count > 0 && string.is_null()) {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    let mut src = String::new();
    for i in 0..count as usize {
        let sp = *string.add(i);
        if sp.is_null() {
            continue;
        }
        let len = if length.is_null() { -1 } else { *length.add(i) };
        let bytes: &[u8] = if len < 0 {
            CStr::from_ptr(sp).to_bytes()
        } else {
            std::slice::from_raw_parts(sp as *const u8, len as usize)
        };
        src.push_str(&String::from_utf8_lossy(bytes));
    }
    let translated = match shader_translate::translate(&src) {
        Ok(t) => t,
        Err(e) => {
            log(&format!("[ShaderTranslate] shader {shader} rejected: {e}"));
            errors().set(GL_INVALID_OPERATION);
            return;
        }
    };
    let c = match CString::new(translated) {
        Ok(c) => c,
        Err(_) => {
            errors().set(GL_INVALID_VALUE);
            return;
        }
    };
    match driver_fn::<unsafe extern "C" fn(u32, i32, *const *const c_char, *const i32)>("glShaderSource") {
        Some(f) => {
            let ptr = c.as_ptr();
            f(shader, 1, &ptr, std::ptr::null());
        }
        None => errors().set(GL_INVALID_OPERATION),
    }
}


// ---- translated entry points (desktop semantics -> GLES) -------------------------------

/// True when pixel-unpack state is default (no row length / skip), so a tight copy is valid.
unsafe fn unpack_default() -> bool {
    let get = match driver_fn::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv") {
        Some(f) => f,
        None => return false,
    };
    let mut v = [0i32; 3];
    get(0x0CF2, &mut v[0]); // GL_UNPACK_ROW_LENGTH
    get(0x0CF3, &mut v[1]); // GL_UNPACK_SKIP_ROWS
    get(0x0CF4, &mut v[2]); // GL_UNPACK_SKIP_PIXELS
    v == [0, 0, 0]
}

/// For 8-bit BGRA uploads returns an RGBA copy; None means "use the caller's data as is".
unsafe fn bgra_to_rgba_upload(w: i32, h: i32, f: u32, ty: u32, d: *const c_void) -> Option<Vec<u8>> {
    if d.is_null() || w <= 0 || h <= 0 || !format_translate::is_bgra8(f, ty) {
        return None;
    }
    if !unpack_default() {
        log("[GLCompat] BGRA upload with non-default unpack state: passed through unconverted");
        return None;
    }
    let n = (w as usize) * (h as usize);
    let src = std::slice::from_raw_parts(d as *const u8, n * 4);
    Some(format_translate::swizzle_bgra_to_rgba(src, n))
}

#[no_mangle]
pub unsafe extern "C" fn glTexImage2D(
    t: u32, l: i32, ifmt: i32, w: i32, h: i32, b: i32, f: u32, ty: u32, d: *const c_void,
) {
    let ifmt2 = format_translate::map_internal_format(ifmt, f, ty);
    let swz = bgra_to_rgba_upload(w, h, f, ty, d);
    let (f2, ty2, ptr) = match &swz {
        Some(v) => (format_translate::GL_RGBA, format_translate::GL_UNSIGNED_BYTE, v.as_ptr() as *const c_void),
        None => (f, ty, d),
    };
    type F = unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void);
    match driver_fn::<F>("glTexImage2D") {
        Some(g) => g(t, l, ifmt2, w, h, b, f2, ty2, ptr),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTexSubImage2D(
    t: u32, l: i32, x: i32, y: i32, w: i32, h: i32, f: u32, ty: u32, d: *const c_void,
) {
    let swz = bgra_to_rgba_upload(w, h, f, ty, d);
    let (f2, ty2, ptr) = match &swz {
        Some(v) => (format_translate::GL_RGBA, format_translate::GL_UNSIGNED_BYTE, v.as_ptr() as *const c_void),
        None => (f, ty, d),
    };
    type F = unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void);
    match driver_fn::<F>("glTexSubImage2D") {
        Some(g) => g(t, l, x, y, w, h, f2, ty2, ptr),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTexParameteri(t: u32, p: u32, v: i32) {
    let is_wrap = matches!(p, 0x2802 | 0x2803 | 0x8072);
    if is_wrap && v == format_translate::GL_CLAMP_TO_BORDER {
        log("[GLCompat] GL_CLAMP_TO_BORDER unsupported in ES 3.0, using clamp-to-edge");
    }
    let v2 = if is_wrap { format_translate::map_wrap(v) } else { v };
    match driver_fn::<unsafe extern "C" fn(u32, u32, i32)>("glTexParameteri") {
        Some(g) => g(t, p, v2),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

/// Desktop `glDrawBuffer(mode)` -> ES `glDrawBuffers(1, &mode)`.
#[no_mangle]
pub unsafe extern "C" fn glDrawBuffer(mode: u32) {
    match driver_fn::<unsafe extern "C" fn(i32, *const u32)>("glDrawBuffers") {
        Some(g) => g(1, &mode),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

/// Desktop `glMapBuffer` -> ES `glMapBufferRange` over the whole buffer.
#[no_mangle]
pub unsafe extern "C" fn glMapBuffer(target: u32, access: u32) -> *mut c_void {
    let bits = match format_translate::map_access(access) {
        Some(b) => b,
        None => {
            errors().set(0x0500); // GL_INVALID_ENUM
            return std::ptr::null_mut();
        }
    };
    let get = driver_fn::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetBufferParameteriv");
    let map = driver_fn::<unsafe extern "C" fn(u32, isize, isize, u32) -> *mut c_void>("glMapBufferRange");
    match (get, map) {
        (Some(g), Some(m)) => {
            let mut size = 0i32;
            g(target, 0x8764, &mut size); // GL_BUFFER_SIZE
            if size <= 0 {
                errors().set(GL_INVALID_OPERATION);
                return std::ptr::null_mut();
            }
            m(target, 0, size as isize, bits)
        }
        _ => {
            errors().set(GL_INVALID_OPERATION);
            std::ptr::null_mut()
        }
    }
}

/// ES has no polygon mode. FILL is the ES behavior (no-op); anything else is reported.
#[no_mangle]
pub extern "C" fn glPolygonMode(_face: u32, mode: u32) {
    if mode != 0x1B02 {
        log("[GLCompat] Unsupported operation: glPolygonMode(LINE/POINT)\n  Reason: not available in GLES\n  Fallback: none");
        errors().set(GL_INVALID_OPERATION);
    }
}

/// Functions whose GLES 3.0 signature and behavior match desktop GL closely enough to forward.
/// Format/enum differences (e.g. BGRA, legacy internal formats) are NOT translated yet;
/// the driver's own error is surfaced via glGetError.
macro_rules! forward_all {
    ($( $name:ident ( $($a:ident : $t:ty),* ) $(-> $r:ty)? ; )*) => {
        $(
            #[no_mangle]
            pub unsafe extern "C" fn $name($($a: $t),*) $(-> $r)? {
                type F = unsafe extern "C" fn($($t),*) $(-> $r)?;
                match driver_fn::<F>(stringify!($name)) {
                    Some(f) => f($($a),*),
                    None => {
                        errors().set(GL_INVALID_OPERATION);
                        Default::default()
                    }
                }
            }
        )*
        fn forwarded(name: &[u8]) -> *const c_void {
            $( if name == stringify!($name).as_bytes() { return $name as *const c_void; } )*
            std::ptr::null()
        }
    };
}

forward_all! {
    glActiveTexture(t: u32);
    glAttachShader(p: u32, s: u32);
    glBindAttribLocation(p: u32, i: u32, n: *const c_char);
    glBindBuffer(t: u32, b: u32);
    glBindBufferBase(t: u32, i: u32, b: u32);
    glBindBufferRange(t: u32, i: u32, b: u32, o: isize, s: isize);
    glBindFramebuffer(t: u32, f: u32);
    glBindRenderbuffer(t: u32, r: u32);
    glBindTexture(t: u32, x: u32);
    glBindVertexArray(a: u32);
    glBlendColor(r: f32, g: f32, b: f32, a: f32);
    glBlendEquation(m: u32);
    glBlendEquationSeparate(a: u32, b: u32);
    glBlendFunc(s: u32, d: u32);
    glBlendFuncSeparate(a: u32, b: u32, c: u32, d: u32);
    glBlitFramebuffer(x0: i32, y0: i32, x1: i32, y1: i32, dx0: i32, dy0: i32, dx1: i32, dy1: i32, mask: u32, filter: u32);
    glBufferData(t: u32, size: isize, data: *const c_void, usage: u32);
    glBufferSubData(t: u32, o: isize, size: isize, data: *const c_void);
    glCheckFramebufferStatus(t: u32) -> u32;
    glClearStencil(s: i32);
    glClearBufferfv(b: u32, d: i32, v: *const f32);
    glClearBufferiv(b: u32, d: i32, v: *const i32);
    glClearBufferuiv(b: u32, d: i32, v: *const u32);
    glClearBufferfi(b: u32, d: i32, dep: f32, st: i32);
    glColorMask(r: u8, g: u8, b: u8, a: u8);
    glCompileShader(s: u32);
    glCreateProgram() -> u32;
    glCreateShader(t: u32) -> u32;
    glCullFace(m: u32);
    glDeleteBuffers(n: i32, b: *const u32);
    glDeleteFramebuffers(n: i32, f: *const u32);
    glDeleteProgram(p: u32);
    glDeleteRenderbuffers(n: i32, r: *const u32);
    glDeleteShader(s: u32);
    glDeleteTextures(n: i32, t: *const u32);
    glDeleteVertexArrays(n: i32, a: *const u32);
    glDepthFunc(f: u32);
    glDepthMask(m: u8);
    glDetachShader(p: u32, s: u32);
    glDisableVertexAttribArray(i: u32);
    glDrawArrays(m: u32, f: i32, c: i32);
    glDrawArraysInstanced(m: u32, f: i32, c: i32, n: i32);
    glDrawBuffers(n: i32, b: *const u32);
    glDrawElements(m: u32, c: i32, t: u32, i: *const c_void);
    glDrawElementsInstanced(m: u32, c: i32, t: u32, i: *const c_void, n: i32);
    glEnableVertexAttribArray(i: u32);
    glFinish();
    glFlush();
    glFramebufferRenderbuffer(t: u32, a: u32, rt: u32, r: u32);
    glFramebufferTexture2D(t: u32, a: u32, tt: u32, tex: u32, l: i32);
    glFramebufferTextureLayer(t: u32, a: u32, tex: u32, l: i32, layer: i32);
    glFrontFace(m: u32);
    glGenBuffers(n: i32, b: *mut u32);
    glGenFramebuffers(n: i32, f: *mut u32);
    glGenRenderbuffers(n: i32, r: *mut u32);
    glGenTextures(n: i32, t: *mut u32);
    glGenVertexArrays(n: i32, a: *mut u32);
    glGenerateMipmap(t: u32);
    glGetAttribLocation(p: u32, n: *const c_char) -> i32;
    glGetBooleanv(p: u32, d: *mut u8);
    glGetFloatv(p: u32, d: *mut f32);
    glGetProgramInfoLog(p: u32, b: i32, l: *mut i32, log: *mut c_char);
    glGetProgramiv(p: u32, n: u32, v: *mut i32);
    glGetShaderInfoLog(s: u32, b: i32, l: *mut i32, log: *mut c_char);
    glGetShaderiv(s: u32, n: u32, v: *mut i32);
    glGetUniformLocation(p: u32, n: *const c_char) -> i32;
    glGetUniformBlockIndex(p: u32, n: *const c_char) -> u32;
    glUniformBlockBinding(p: u32, i: u32, b: u32);
    glIsEnabled(c: u32) -> u8;
    glLinkProgram(p: u32);
    glPixelStorei(n: u32, v: i32);
    glPolygonOffset(f: f32, u: f32);
    glReadBuffer(m: u32);
    glReadPixels(x: i32, y: i32, w: i32, h: i32, f: u32, t: u32, d: *mut c_void);
    glRenderbufferStorage(t: u32, f: u32, w: i32, h: i32);
    glScissor(x: i32, y: i32, w: i32, h: i32);
    glStencilFunc(f: u32, r: i32, m: u32);
    glStencilMask(m: u32);
    glStencilOp(a: u32, b: u32, c: u32);
    glTexImage3D(t: u32, l: i32, ifmt: i32, w: i32, h: i32, dp: i32, b: i32, f: u32, ty: u32, d: *const c_void);
    glTexParameterf(t: u32, p: u32, v: f32);
    glUniform1f(l: i32, a: f32);
    glUniform2f(l: i32, a: f32, b: f32);
    glUniform3f(l: i32, a: f32, b: f32, c: f32);
    glUniform4f(l: i32, a: f32, b: f32, c: f32, d: f32);
    glUniform1i(l: i32, a: i32);
    glUniform2i(l: i32, a: i32, b: i32);
    glUniform3i(l: i32, a: i32, b: i32, c: i32);
    glUniform4i(l: i32, a: i32, b: i32, c: i32, d: i32);
    glUniform1fv(l: i32, n: i32, v: *const f32);
    glUniform2fv(l: i32, n: i32, v: *const f32);
    glUniform3fv(l: i32, n: i32, v: *const f32);
    glUniform4fv(l: i32, n: i32, v: *const f32);
    glUniform1iv(l: i32, n: i32, v: *const i32);
    glUniformMatrix2fv(l: i32, n: i32, t: u8, v: *const f32);
    glUniformMatrix3fv(l: i32, n: i32, t: u8, v: *const f32);
    glUniformMatrix4fv(l: i32, n: i32, t: u8, v: *const f32);
    glUseProgram(p: u32);
    glVertexAttribPointer(i: u32, s: i32, t: u32, n: u8, st: i32, p: *const c_void);
    glVertexAttribIPointer(i: u32, s: i32, t: u32, st: i32, p: *const c_void);
    glVertexAttribDivisor(i: u32, d: u32);
    glUnmapBuffer(t: u32) -> u8;
}

/// Symbol lookup used by LWJGL/GLFW-style loaders (`glXGetProcAddress` flavour).
/// Returns null for anything unimplemented -- never a stub that pretends to work.
#[no_mangle]
pub extern "C" fn glXGetProcAddress(name: *const c_char) -> *const c_void {
    if name.is_null() {
        return std::ptr::null();
    }
    // SAFETY: caller passes a NUL-terminated C string per the GLX contract; null checked above.
    let n = unsafe { CStr::from_ptr(name) }.to_bytes();
    match n {
        b"glGetError" => glGetError as *const c_void,
        b"glClearColor" => glClearColor as *const c_void,
        b"glClear" => glClear as *const c_void,
        b"glViewport" => glViewport as *const c_void,
        b"glEnable" => glEnable as *const c_void,
        b"glDisable" => glDisable as *const c_void,
        b"glGetString" => glGetString as *const c_void,
        b"glGetIntegerv" => glGetIntegerv as *const c_void,
        b"glGetStringi" => glGetStringi as *const c_void,
        b"glClearDepth" => glClearDepth as *const c_void,
        b"glShaderSource" => glShaderSource as *const c_void,
        b"glTexImage2D" => glTexImage2D as *const c_void,
        b"glTexSubImage2D" => glTexSubImage2D as *const c_void,
        b"glTexParameteri" => glTexParameteri as *const c_void,
        b"glDrawBuffer" => glDrawBuffer as *const c_void,
        b"glMapBuffer" => glMapBuffer as *const c_void,
        b"glPolygonMode" => glPolygonMode as *const c_void,
        _ if !forwarded(n).is_null() => forwarded(n),
        _ => {
            log(&format!(
                "[GLCompat] Unsupported operation: {}\n  Reason: not implemented in this phase\n  Fallback: none",
                String::from_utf8_lossy(n)
            ));
            std::ptr::null()
        }
    }
}

#[no_mangle]
pub extern "C" fn glXGetProcAddressARB(name: *const c_char) -> *const c_void {
    glXGetProcAddress(name)
}
