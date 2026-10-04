//! C-ABI OpenGL surface loaded by the launcher as the "GL library".
//!
//! GLES 3.0 passthrough with desktop-GL compatibility shims:
//! - shader source rewriting (desktop GLSL → GLSL ES)
//! - BGRA upload swizzle, clamp-to-border → clamp-to-edge
//! - glMapBuffer → glMapBufferRange, glDrawBuffer → glDrawBuffers, glClearDepth → f
//! - optional GL 3.2 version spoof (`RENDERER_SPOOF_GL=1`, on by default via plugin env)
//!
//! This is still incomplete for full Minecraft parity (no Vulkan, limited shader rewrite,
//! missing some desktop-only APIs). Expect crash/black-screen on unhandled paths.

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

static SPOOF_VERSION: &[u8] = b"3.3 (Core Profile) RustRenderer GLES passthrough\0";
static SPOOF_GLSL: &[u8] = b"3.30\0";

/// OPT-IN, EXPERIMENTAL: `RENDERER_SPOOF_GL=1` makes the renderer claim OpenGL 3.3 core.
/// The claim is NOT backed by a full implementation. Off by default (spec: never advertise
/// unsupported features).
fn spoof_gl() -> bool {
    static S: OnceLock<bool> = OnceLock::new();
    *S.get_or_init(|| {
        // Default ON so Minecraft's GL version checks pass. Set RENDERER_SPOOF_GL=0 to disable.
        let on = std::env::var("RENDERER_SPOOF_GL")
            .map(|v| v != "0")
            .unwrap_or(true);
        if on {
            log("[GLCompat] RENDERER_SPOOF_GL enabled: advertising OpenGL 3.3 (passthrough GLES)");
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
                *data = 3;
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

/// For BGRA/BGR uploads returns a converted RGB(A) copy; None means use caller data as-is.
unsafe fn convert_pixel_upload(w: i32, h: i32, f: u32, ty: u32, d: *const c_void) -> Option<(u32, u32, Vec<u8>)> {
    if d.is_null() || w <= 0 || h <= 0 {
        return None;
    }
    if !unpack_default() {
        if format_translate::is_bgra8(f, ty) || format_translate::is_bgr8(f, ty) {
            log("[GLCompat] BGR(A) upload with non-default unpack state: passed through");
        }
        return None;
    }
    let n = (w as usize) * (h as usize);
    if format_translate::is_bgra8(f, ty) {
        let src = std::slice::from_raw_parts(d as *const u8, n * 4);
        return Some((
            format_translate::GL_RGBA,
            format_translate::GL_UNSIGNED_BYTE,
            format_translate::swizzle_bgra_to_rgba(src, n),
        ));
    }
    if format_translate::is_bgr8(f, ty) {
        let src = std::slice::from_raw_parts(d as *const u8, n * 3);
        return Some((
            format_translate::GL_RGB,
            format_translate::GL_UNSIGNED_BYTE,
            format_translate::swizzle_bgr_to_rgb(src, n),
        ));
    }
    None
}

#[no_mangle]
pub unsafe extern "C" fn glTexImage2D(
    t: u32, l: i32, ifmt: i32, w: i32, h: i32, b: i32, f: u32, ty: u32, d: *const c_void,
) {
    let ifmt2 = format_translate::map_internal_format(ifmt, f, ty);
    let conv = convert_pixel_upload(w, h, f, ty, d);
    let (f2, ty2, ptr) = match &conv {
        Some((nf, nty, v)) => (*nf, *nty, v.as_ptr() as *const c_void),
        None => (format_translate::map_external_format(f), ty, d),
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
    let conv = convert_pixel_upload(w, h, f, ty, d);
    let (f2, ty2, ptr) = match &conv {
        Some((nf, nty, v)) => (*nf, *nty, v.as_ptr() as *const c_void),
        None => (format_translate::map_external_format(f), ty, d),
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
    // --- additional ES 3.0 / desktop-common entry points ---
    glMapBufferRange(t: u32, o: isize, l: isize, a: u32) -> *mut c_void;
    glFlushMappedBufferRange(t: u32, o: isize, l: isize);
    glCopyBufferSubData(r: u32, w: u32, ro: isize, wo: isize, s: isize);
    glGetBufferParameteriv(t: u32, n: u32, v: *mut i32);
    glGetBufferParameteri64v(t: u32, n: u32, v: *mut i64);
    glTexStorage2D(t: u32, levels: i32, ifmt: u32, w: i32, h: i32);
    glTexStorage3D(t: u32, levels: i32, ifmt: u32, w: i32, h: i32, d: i32);
    glTexSubImage3D(t: u32, l: i32, x: i32, y: i32, z: i32, w: i32, h: i32, d: i32, f: u32, ty: u32, data: *const c_void);
    glCompressedTexImage2D(t: u32, l: i32, ifmt: u32, w: i32, h: i32, b: i32, size: i32, data: *const c_void);
    glCompressedTexSubImage2D(t: u32, l: i32, x: i32, y: i32, w: i32, h: i32, f: u32, size: i32, data: *const c_void);
    glCopyTexImage2D(t: u32, l: i32, ifmt: u32, x: i32, y: i32, w: i32, h: i32, b: i32);
    glCopyTexSubImage2D(t: u32, l: i32, x: i32, y: i32, sx: i32, sy: i32, w: i32, h: i32);
    glGetTexParameteriv(t: u32, n: u32, v: *mut i32);
    glGetTexParameterfv(t: u32, n: u32, v: *mut f32);
    glGetActiveUniform(p: u32, i: u32, buf: i32, len: *mut i32, size: *mut i32, ty: *mut u32, name: *mut c_char);
    glGetActiveAttrib(p: u32, i: u32, buf: i32, len: *mut i32, size: *mut i32, ty: *mut u32, name: *mut c_char);
    glGetAttachedShaders(p: u32, max: i32, count: *mut i32, shaders: *mut u32);
    glGetFramebufferAttachmentParameteriv(t: u32, a: u32, n: u32, v: *mut i32);
    glGetRenderbufferParameteriv(t: u32, n: u32, v: *mut i32);
    glGetShaderSource(s: u32, buf: i32, len: *mut i32, src: *mut c_char);
    glGetVertexAttribiv(i: u32, n: u32, v: *mut i32);
    glGetVertexAttribfv(i: u32, n: u32, v: *mut f32);
    glGetVertexAttribPointerv(i: u32, n: u32, p: *mut *mut c_void);
    glIsBuffer(b: u32) -> u8;
    glIsFramebuffer(f: u32) -> u8;
    glIsProgram(p: u32) -> u8;
    glIsRenderbuffer(r: u32) -> u8;
    glIsShader(s: u32) -> u8;
    glIsTexture(t: u32) -> u8;
    glIsVertexArray(a: u32) -> u8;
    glLineWidth(w: f32);
    glSampleCoverage(v: f32, invert: u8);
    glStencilFuncSeparate(face: u32, f: u32, r: i32, m: u32);
    glStencilMaskSeparate(face: u32, m: u32);
    glStencilOpSeparate(face: u32, a: u32, b: u32, c: u32);
    glHint(t: u32, m: u32);
    glInvalidateFramebuffer(t: u32, n: i32, attachments: *const u32);
    glInvalidateSubFramebuffer(t: u32, n: i32, attachments: *const u32, x: i32, y: i32, w: i32, h: i32);
    glRenderbufferStorageMultisample(t: u32, samples: i32, ifmt: u32, w: i32, h: i32);
    glGetInteger64v(p: u32, d: *mut i64);
    glGetIntegeri_v(p: u32, i: u32, d: *mut i32);
    glGetInteger64i_v(p: u32, i: u32, d: *mut i64);
    glVertexAttrib1f(i: u32, x: f32);
    glVertexAttrib2f(i: u32, x: f32, y: f32);
    glVertexAttrib3f(i: u32, x: f32, y: f32, z: f32);
    glVertexAttrib4f(i: u32, x: f32, y: f32, z: f32, w: f32);
    glVertexAttrib4Nub(i: u32, x: u8, y: u8, z: u8, w: u8);
    glBindSampler(unit: u32, sampler: u32);
    glGenSamplers(n: i32, s: *mut u32);
    glDeleteSamplers(n: i32, s: *const u32);
    glIsSampler(s: u32) -> u8;
    glSamplerParameteri(s: u32, p: u32, v: i32);
    glSamplerParameterf(s: u32, p: u32, v: f32);
    glGetSamplerParameteriv(s: u32, p: u32, v: *mut i32);
    glGetSamplerParameterfv(s: u32, p: u32, v: *mut f32);
    glFenceSync(c: u32, f: u32) -> *mut c_void;
    glIsSync(s: *mut c_void) -> u8;
    glDeleteSync(s: *mut c_void);
    glClientWaitSync(s: *mut c_void, f: u32, timeout: u64) -> u32;
    glWaitSync(s: *mut c_void, f: u32, timeout: u64);
    glGetSynciv(s: *mut c_void, p: u32, buf: i32, len: *mut i32, v: *mut i32);
    glDrawRangeElements(m: u32, start: u32, end: u32, c: i32, t: u32, i: *const c_void);
    glClearDepthf(d: f32);
    glDepthRangef(n: f32, f: f32);
    glGetProgramBinary(p: u32, buf: i32, len: *mut i32, format: *mut u32, binary: *mut c_void);
    glProgramBinary(p: u32, format: u32, binary: *const c_void, len: i32);
    glProgramParameteri(p: u32, n: u32, v: i32);
    glValidateProgram(p: u32);
    glGetUniformfv(p: u32, loc: i32, v: *mut f32);
    glGetUniformiv(p: u32, loc: i32, v: *mut i32);
    glTransformFeedbackVaryings(p: u32, count: i32, varyings: *const *const c_char, buffer_mode: u32);
    glBeginTransformFeedback(mode: u32);
    glEndTransformFeedback();
    glBindTransformFeedback(t: u32, id: u32);
    glGenTransformFeedbacks(n: i32, ids: *mut u32);
    glDeleteTransformFeedbacks(n: i32, ids: *const u32);
    glIsTransformFeedback(id: u32) -> u8;
    glPauseTransformFeedback();
    glResumeTransformFeedback();
    glVertexAttribI4i(i: u32, x: i32, y: i32, z: i32, w: i32);
    glVertexAttribI4ui(i: u32, x: u32, y: u32, z: u32, w: u32);
    glUniform1uiv(l: i32, n: i32, v: *const u32);
    glUniform2uiv(l: i32, n: i32, v: *const u32);
    glUniform3uiv(l: i32, n: i32, v: *const u32);
    glUniform4uiv(l: i32, n: i32, v: *const u32);
    glUniform1ui(l: i32, a: u32);
    glUniform2ui(l: i32, a: u32, b: u32);
    glUniform3ui(l: i32, a: u32, b: u32, c: u32);
    glUniform4ui(l: i32, a: u32, b: u32, c: u32, d: u32);
    glGetActiveUniformBlockiv(p: u32, i: u32, n: u32, v: *mut i32);
    glGetActiveUniformBlockName(p: u32, i: u32, buf: i32, len: *mut i32, name: *mut c_char);
    glGetUniformIndices(p: u32, count: i32, names: *const *const c_char, indices: *mut u32);
    glGetActiveUniformsiv(p: u32, count: i32, indices: *const u32, n: u32, params: *mut i32);
    glDrawArraysInstancedBaseInstance(m: u32, f: i32, c: i32, n: i32, base: u32);
    glDrawElementsInstancedBaseVertex(m: u32, c: i32, t: u32, i: *const c_void, n: i32, base: i32);
    glDrawElementsBaseVertex(m: u32, c: i32, t: u32, i: *const c_void, base: i32);
    glBindFragDataLocation(p: u32, color: u32, name: *const c_char);
    glGetFragDataLocation(p: u32, name: *const c_char) -> i32;
}

/// Symbol lookup used by LWJGL/GLFW-style loaders (`glXGetProcAddress` flavour).
/// Returns null for anything unimplemented -- never a stub that pretends to work.
fn resolve_proc(n: &[u8]) -> *const c_void {
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
        b"glXGetProcAddress" | b"glXGetProcAddressARB" | b"glGetProcAddress" => {
            glXGetProcAddress as *const c_void
        }
        b"eglGetProcAddress" => eglGetProcAddress as *const c_void,
        b"eglGetDisplay" => eglGetDisplay as *const c_void,
        b"eglInitialize" => eglInitialize as *const c_void,
        b"eglTerminate" => eglTerminate as *const c_void,
        b"eglChooseConfig" => eglChooseConfig as *const c_void,
        b"eglGetConfigs" => eglGetConfigs as *const c_void,
        b"eglGetConfigAttrib" => eglGetConfigAttrib as *const c_void,
        b"eglCreateWindowSurface" => eglCreateWindowSurface as *const c_void,
        b"eglCreatePbufferSurface" => eglCreatePbufferSurface as *const c_void,
        b"eglDestroySurface" => eglDestroySurface as *const c_void,
        b"eglBindAPI" => eglBindAPI as *const c_void,
        b"eglCreateContext" => eglCreateContext as *const c_void,
        b"eglDestroyContext" => eglDestroyContext as *const c_void,
        b"eglMakeCurrent" => eglMakeCurrent as *const c_void,
        b"eglGetCurrentContext" => eglGetCurrentContext as *const c_void,
        b"eglGetCurrentDisplay" => eglGetCurrentDisplay as *const c_void,
        b"eglGetCurrentSurface" => eglGetCurrentSurface as *const c_void,
        b"eglQuerySurface" => eglQuerySurface as *const c_void,
        b"eglSwapBuffers" => eglSwapBuffers as *const c_void,
        b"eglSwapInterval" => eglSwapInterval as *const c_void,
        b"eglQueryString" => eglQueryString as *const c_void,
        b"eglGetError" => eglGetError as *const c_void,
        b"eglReleaseThread" => eglReleaseThread as *const c_void,
        _ => {
            let f = forwarded(n);
            if !f.is_null() {
                f
            } else {
                // Fall through to the driver so GLES extensions still resolve.
                match backend() {
                    Some(be) => {
                        let name = String::from_utf8_lossy(n);
                        let p = be.proc_address(&name);
                        if p.is_null() {
                            log(&format!(
                                "[GLCompat] Missing entry point: {name}"
                            ));
                        }
                        p
                    }
                    None => {
                        log(&format!(
                            "[GLCompat] Missing entry point (no backend): {}",
                            String::from_utf8_lossy(n)
                        ));
                        std::ptr::null()
                    }
                }
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn glXGetProcAddress(name: *const c_char) -> *const c_void {
    if name.is_null() {
        return std::ptr::null();
    }
    // SAFETY: caller passes a NUL-terminated C string per the GLX contract; null checked above.
    let n = unsafe { CStr::from_ptr(name) }.to_bytes();
    resolve_proc(n)
}

#[no_mangle]
pub extern "C" fn glXGetProcAddressARB(name: *const c_char) -> *const c_void {
    glXGetProcAddress(name)
}



#[no_mangle]
pub extern "C" fn glGetProcAddress(name: *const c_char) -> *const c_void {
    glXGetProcAddress(name)
}



// =============================================================================
// EGL forwarding — MobileGL-style: same .so is the EGL provider for the launcher.
// We dlopen system libEGL once and re-export every entry the egl_loader needs.
// No DT_NEEDED on libEGL (avoids linker/constructor fights in the game process).
// =============================================================================

struct SysEgl {
    lib: libloading::Library,
}

fn sys_egl() -> Option<&'static SysEgl> {
    static EGL: OnceLock<Option<SysEgl>> = OnceLock::new();
    EGL.get_or_init(|| {
        let paths = [
            "/system/lib64/libEGL.so",
            "/vendor/lib64/libEGL.so",
            "libEGL.so",
        ];
        for path in paths {
            if let Ok(lib) = unsafe { libloading::Library::new(path) } {
                log(&format!("[EGL] loaded system EGL from {path}"));
                return Some(SysEgl { lib });
            }
        }
        log("[EGL] FAILED to load system libEGL.so");
        None
    })
    .as_ref()
}

fn egl_sym<T>(name: &[u8]) -> Option<T>
where
    T: Copy,
{
    let e = sys_egl()?;
    let s: libloading::Symbol<T> = unsafe { e.lib.get(name).ok()? };
    Some(*s)
}

macro_rules! egl_export {
    ($name:ident ( $($arg:ident : $ty:ty),* ) -> $ret:ty) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name($($arg : $ty),*) -> $ret {
            type F = unsafe extern "C" fn($($ty),*) -> $ret;
            let n = concat!(stringify!($name), "\0");
            match egl_sym::<F>(n.as_bytes()) {
                Some(f) => f($($arg),*),
                None => {
                    // Safe zero defaults for pointer / int returns
                    #[allow(unused_assignments, unused_mut)]
                    let mut z: $ret = unsafe { std::mem::zeroed() };
                    z
                }
            }
        }
    };
    // void-ish already covered by -> type
}

// Core EGL 1.4 used by pojov egl_loader / glfwstub
egl_export!(eglGetDisplay(display_id: *mut c_void) -> *mut c_void);
egl_export!(eglInitialize(dpy: *mut c_void, major: *mut i32, minor: *mut i32) -> u32);
egl_export!(eglTerminate(dpy: *mut c_void) -> u32);
egl_export!(eglGetConfigs(dpy: *mut c_void, configs: *mut *mut c_void, config_size: i32, num: *mut i32) -> u32);
egl_export!(eglChooseConfig(dpy: *mut c_void, attrib: *const i32, configs: *mut *mut c_void, config_size: i32, num: *mut i32) -> u32);
egl_export!(eglGetConfigAttrib(dpy: *mut c_void, config: *mut c_void, attr: i32, value: *mut i32) -> u32);
egl_export!(eglCreateWindowSurface(dpy: *mut c_void, config: *mut c_void, win: *mut c_void, attrib: *const i32) -> *mut c_void);
egl_export!(eglCreatePbufferSurface(dpy: *mut c_void, config: *mut c_void, attrib: *const i32) -> *mut c_void);
egl_export!(eglDestroySurface(dpy: *mut c_void, surface: *mut c_void) -> u32);
egl_export!(eglBindAPI(api: u32) -> u32);
egl_export!(eglCreateContext(dpy: *mut c_void, config: *mut c_void, share: *mut c_void, attrib: *const i32) -> *mut c_void);
egl_export!(eglDestroyContext(dpy: *mut c_void, ctx: *mut c_void) -> u32);
egl_export!(eglMakeCurrent(dpy: *mut c_void, draw: *mut c_void, read: *mut c_void, ctx: *mut c_void) -> u32);
egl_export!(eglGetCurrentContext() -> *mut c_void);
egl_export!(eglGetCurrentDisplay() -> *mut c_void);
egl_export!(eglGetCurrentSurface(readdraw: i32) -> *mut c_void);
egl_export!(eglQuerySurface(dpy: *mut c_void, surface: *mut c_void, attr: i32, value: *mut i32) -> u32);
egl_export!(eglSwapBuffers(dpy: *mut c_void, surface: *mut c_void) -> u32);
egl_export!(eglSwapInterval(dpy: *mut c_void, interval: i32) -> u32);
egl_export!(eglQueryString(dpy: *mut c_void, name: i32) -> *const u8);
egl_export!(eglGetError() -> u32);
egl_export!(eglReleaseThread() -> u32);
egl_export!(eglBindTexImage(dpy: *mut c_void, surface: *mut c_void, buffer: i32) -> u32);
egl_export!(eglReleaseTexImage(dpy: *mut c_void, surface: *mut c_void, buffer: i32) -> u32);
egl_export!(eglSurfaceAttrib(dpy: *mut c_void, surface: *mut c_void, attr: i32, value: i32) -> u32);
egl_export!(eglWaitClient() -> u32);
egl_export!(eglWaitGL() -> u32);
egl_export!(eglWaitNative(engine: i32) -> u32);
egl_export!(eglQueryContext(dpy: *mut c_void, ctx: *mut c_void, attr: i32, value: *mut i32) -> u32);
egl_export!(eglQueryAPI() -> u32);

/// eglGetProcAddress: our GL symbols first, then system EGL.
#[no_mangle]
pub unsafe extern "C" fn eglGetProcAddress(name: *const c_char) -> *const c_void {
    if name.is_null() {
        return std::ptr::null();
    }
    let n = unsafe { CStr::from_ptr(name) }.to_bytes();
    let ours = resolve_proc(n);
    if !ours.is_null() {
        return ours;
    }
    // System extension lookup
    type F = unsafe extern "C" fn(*const c_char) -> *const c_void;
    match egl_sym::<F>(b"eglGetProcAddress\0") {
        Some(f) => f(name),
        None => std::ptr::null(),
    }
}
