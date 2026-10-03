//! C-ABI OpenGL surface loaded by the launcher as the "GL library".
//!
//! Phase 1 scope: a handful of core entry points over the GLES backend with OpenGL error
//! semantics. It reports the driver's REAL (GLES) strings. It does NOT translate desktop GL,
//! so Minecraft Java will not run on this yet (spec phases 3-4).

use renderer_core::{Backend, BackendKind, Config, GlErrorState};
use std::ffi::{c_char, c_void, CStr};
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

/// Returns the driver's own string pointer (valid for the context lifetime).
#[no_mangle]
pub extern "C" fn glGetString(name: u32) -> *const u8 {
    match backend() {
        Some(be) => be.get_string(name),
        None => {
            errors().set(GL_INVALID_OPERATION);
            std::ptr::null()
        }
    }
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
