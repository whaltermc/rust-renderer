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

#[macro_use]
// Entry points are grouped by API family -- the desktop-GL compatibility surface, the ES
// surface underneath it, and extension spellings. See `gl/mod.rs` for the full map.
mod gl;
mod gles3;
mod khr;

// The GL compatibility surface, still flat: vertex/attribute association and MSAA
// substitution, the GL 4.5 named-object entry points, and the fixed-function path.
mod vertex_state;
mod named_objects;
mod fixed_func;
mod fixed_draw;
mod immediate;

use renderer_core::{Backend, BackendKind, Config, GlErrorState};
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};


const GL_INVALID_VALUE: u32 = 0x0501;
const GL_INVALID_OPERATION: u32 = 0x0502;


static BACKEND: OnceLock<Box<dyn Backend>> = OnceLock::new();
static ERRORS: OnceLock<GlErrorState> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());

fn errors() -> &'static GlErrorState {
    ERRORS.get_or_init(GlErrorState::default)
}

/// Current GL_ARRAY_BUFFER binding (client-array pointers capture it, per GL semantics).
/// Shadow of the `GL_ARRAY_BUFFER` binding.
///
/// `GL_ARRAY_BUFFER` is global context state (it is *not* part of vertex-array-object
/// state, unlike `GL_ELEMENT_ARRAY_BUFFER`), so mirroring it here is safe. This removes a
/// `glGetIntegerv` round-trip from every client-array pointer call on the fixed-function
/// path.
static ARRAY_BUFFER_BINDING: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn set_array_buffer_binding(v: u32) {
    ARRAY_BUFFER_BINDING.store(v, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn current_array_buffer() -> u32 {
    ARRAY_BUFFER_BINDING.load(std::sync::atomic::Ordering::Relaxed)
}


/// Ring buffer of the most recent forwarded GL entry points.
///
/// The 1.21.11 failure was `IllegalStateException: OpenGL error 1282` with no "GL error site"
/// line, which proved the error came from the *driver*, not from this layer -- and left no way
/// to tell which call provoked it. Minecraft only ever reports the numeric code, so the bridge
/// keeps the last few calls and dumps them when `glGetError` returns something.
///
/// Enabled with `RENDERER_TRACE_GL=1` so the common path stays a single relaxed atomic load.
const TRACE_LEN: usize = 16;
static TRACE: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static TRACE_ON: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(u8::MAX);

fn trace_enabled() -> bool {
    let cached = TRACE_ON.load(Ordering::Relaxed);
    if cached != u8::MAX {
        return cached != 0;
    }
    let on = std::env::var("RENDERER_TRACE_GL").map(|v| v == "1").unwrap_or(false);
    TRACE_ON.store(u8::from(on), Ordering::Relaxed);
    on
}

/// Records a forwarded GL call. Called from `forward_all!`, so it covers every pass-through.
#[inline]
pub(crate) fn trace_call(name: &'static str) {
    if !trace_enabled() {
        return;
    }
    let mut t = TRACE.lock().unwrap_or_else(|e| e.into_inner());
    if t.len() == TRACE_LEN {
        t.remove(0);
    }
    t.push(name);
}

/// Dumps the recent call history, newest last. Called when the game sees a GL error.
fn dump_trace(reason: &str) {
    if !trace_enabled() {
        return;
    }
    let t = TRACE.lock().unwrap_or_else(|e| e.into_inner());
    if t.is_empty() {
        return;
    }
    log(&format!("[GLTrace] {reason}; last calls: {}", t.join(" -> ")));
}

/// Runs `f` only the first time it is called. Used for one-time diagnostics that would
/// otherwise repeat per frame.
pub(crate) fn log_once(flag: &std::sync::atomic::AtomicBool, msg: &str) {
    if !flag.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log(msg);
    }
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
        // Also to stderr: the launcher captures it into the game log (no adb needed).
        eprintln!("[RustRenderer] {msg}");
    }
    #[cfg(not(target_os = "android"))]
    eprintln!("[RustRenderer] {msg}");
}

/// Lazily selects/initializes a backend on the first GL call (a context must be current by
/// then). Never selects a backend that fails to initialize.
static BACKEND_FAILED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn backend() -> Option<&'static dyn Backend> {
    if let Some(b) = BACKEND.get() {
        return Some(b.as_ref());
    }
    // A failed initialisation is remembered too. Without this the whole probe reran on every
    // GL call, which during a trace replay was 51972 attempts and buried every other message.
    if BACKEND_FAILED.load(Ordering::Relaxed) {
        return None;
    }
    let _g = INIT_LOCK.lock().ok()?;
    if let Some(b) = BACKEND.get() {
        return Some(b.as_ref());
    }
    let cfg = Config::from_env();
    log(&format!(
        "[Renderer] Initializing, requested backend: {}",
        cfg.backend.as_str()
    ));

    let gles = || match gles_backend::GlesBackend::new() {
        Ok(b) => Some(Box::new(b) as Box<dyn Backend>),
        Err(e) => {
            log(&format!("[GLES] {e}"));
            None
        }
    };

    // Vulkan is only *attempted* for the modes that ask for it. Note the game reaches us
    // through desktop GL entry points, which the GLES driver serves; a Vulkan backend is
    // usable here only for renderer-owned work (fixed-function emulation), and only when it
    // reports it can actually render.
    let wants_vulkan = matches!(
        cfg.backend,
        BackendKind::Auto | BackendKind::Vulkan | BackendKind::Hybrid
    );
    let probed = if wants_vulkan {
        match vulkan_backend::probe() {
            Ok(b) => {
                let i = b.device_info();
                log(&format!("[Vulkan] found {} — {}", i.renderer, i.api_version));
                if !b.can_render() {
                    log("[Vulkan] device present but it cannot render yet (no SPIR-V/pipeline/present path)");
                }
                Some(b)
            }
            Err(e) => {
                log(&format!("[Vulkan] {e}"));
                None
            }
        }
    } else {
        None
    };
    // Description for logging, taken before the backend is moved into a match arm.
    let detected_vulkan = probed.as_ref().map(|b| b.device_info().renderer.clone());
    let vulkan_can_render = probed.as_ref().is_some_and(|b| b.can_render());

    let chosen: Option<Box<dyn Backend>> = match cfg.backend {
        // Explicit Vulkan: never claim to be rendering on a backend that cannot.
        BackendKind::Vulkan => match probed {
            Some(b) if vulkan_can_render => Some(b),
            _ => {
                log("[Renderer] RENDERER_BACKEND=vulkan requested but no Vulkan device can render; using GLES so the game still starts");
                gles()
            }
        },
        // Hybrid: GLES serves the GL entry points that every frame is drawn through; Vulkan
        // is kept for renderer-owned work once it can render. Today it cannot, so GLES draws
        // the frame and this says so explicitly rather than looking like gles was ignored.
        BackendKind::Hybrid => {
            match (&detected_vulkan, vulkan_can_render) {
                (Some(dev), true) => log(&format!(
                    "[Renderer] hybrid: Vulkan device '{dev}' will take renderer-owned work; \
                     GLES still serves the GL entry points"
                )),
                (Some(dev), false) => log(&format!(
                    "[Renderer] hybrid: Vulkan device '{dev}' detected but it cannot render yet, \
                     so GLES serves the whole frame. Vulkan is used for device reporting only."
                )),
                (None, _) => log(
                    "[Renderer] hybrid: no Vulkan device available, so GLES serves the whole frame",
                ),
            }
            gles()
        }
        BackendKind::Gles => gles(),
        BackendKind::Auto => {
            if vulkan_can_render {
                probed
            } else {
                gles()
            }
        }
    };

    match chosen {
        Some(b) => {
            let i = b.device_info();
            log(&format!("[Renderer] Backend: {}", b.kind().as_str()));
            log(&format!("[Renderer] GPU: {} ({})", i.renderer, i.vendor));
            log(&format!("[Renderer] API: {}", i.api_version));
            let c = b.capabilities();
            log(&format!(
                "[Renderer] GlesCapabilities: ES {}.{}, {} extensions, max texture {}, {} draw buffers",
                c.es_major, c.es_minor, c.extensions.len(), c.max_texture_size, c.max_draw_buffers
            ));
            let _ = BACKEND.set(b);
            BACKEND.get().map(|b| b.as_ref())
        }
        None => {
            BACKEND_FAILED.store(true, Ordering::Relaxed);
            log("[Renderer] No backend could be initialized; the bridge will run in passthrough-only mode");
            None
        }
    }
}

#[no_mangle]
pub extern "C" fn glGetError() -> u32 {
    let ours = errors().take();
    if ours != 0 {
        dump_trace(&format!("glGetError from this layer: 0x{ours:04X}"));
        return ours;
    }
    if let Some(b) = backend() {
        let e = b.get_error();
        if e != 0 {
            dump_trace(&format!("glGetError from the driver: 0x{e:04X}"));
        }
        return e;
    }
    unsafe {
        type F = unsafe extern "C" fn() -> u32;
        if let Some(f) = driver_fn_cached::<F>("glGetError") {
            let e = f();
            if e != 0 {
                dump_trace(&format!("glGetError from the driver: 0x{e:04X}"));
            }
            return e;
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn glClearColor(r: f32, g: f32, b: f32, a: f32) {
    if let Some(be) = backend() {
        be.clear_color(r, g, b, a);
        return;
    }
    unsafe {
        type F = unsafe extern "C" fn(f32, f32, f32, f32);
        if let Some(f) = driver_fn_cached::<F>("glClearColor") {
            f(r, g, b, a);
            return;
        }
    }
    errors().set(GL_INVALID_OPERATION);
}

#[no_mangle]
pub extern "C" fn glClear(mask: u32) {
    if let Some(be) = backend() {
        be.clear(mask);
        return;
    }
    unsafe {
        type F = unsafe extern "C" fn(u32);
        if let Some(f) = driver_fn_cached::<F>("glClear") {
            f(mask);
            return;
        }
    }
    errors().set(GL_INVALID_OPERATION);
}

#[no_mangle]
pub extern "C" fn glViewport(x: i32, y: i32, w: i32, h: i32) {
    if w < 0 || h < 0 {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    if let Some(be) = backend() {
        be.viewport(x, y, w, h);
        return;
    }
    unsafe {
        type F = unsafe extern "C" fn(i32, i32, i32, i32);
        if let Some(f) = driver_fn_cached::<F>("glViewport") {
            f(x, y, w, h);
            return;
        }
    }
    errors().set(GL_INVALID_OPERATION);
}

#[no_mangle]
pub extern "C" fn glEnable(cap: u32) {
    if fixed_func::handle_cap(cap, true) {
        return;
    }
    if let Some(be) = backend() {
        be.enable(cap);
        return;
    }
    unsafe {
        type F = unsafe extern "C" fn(u32);
        if let Some(f) = driver_fn_cached::<F>("glEnable") {
            f(cap);
            return;
        }
    }
    errors().set(GL_INVALID_OPERATION);
}

#[no_mangle]
pub extern "C" fn glDisable(cap: u32) {
    if fixed_func::handle_cap(cap, false) {
        return;
    }
    if let Some(be) = backend() {
        be.disable(cap);
        return;
    }
    unsafe {
        type F = unsafe extern "C" fn(u32);
        if let Some(f) = driver_fn_cached::<F>("glDisable") {
            f(cap);
            return;
        }
    }
    errors().set(GL_INVALID_OPERATION);
}

const GL_VERSION: u32 = 0x1F02;
const GL_SHADING_LANGUAGE_VERSION: u32 = 0x8B8C;
const GL_MAJOR_VERSION: u32 = 0x821B;
const GL_MINOR_VERSION: u32 = 0x821C;

static SPOOF_VERSION: &[u8] = b"3.3 (Core Profile) RustRenderer GLES translation\0";
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
/// Direct GLESv3/v2 symbol bridge used by every forwarded entry point.
/// Symbol lookup does NOT require a current context (only the first real GL *call*
/// that queries state does). This is what LWJGL needs: dlsym our exports, then
/// each export jumps into the system GLES driver.
struct GlesDriver {
    lib: libloading::Library,
}

fn gles_driver() -> Option<&'static GlesDriver> {
    static DRV: OnceLock<Option<GlesDriver>> = OnceLock::new();
    DRV.get_or_init(|| {
        // Prefer GLESv3; fall back to GLESv2 (still exports most ES3 entry points
        // on modern Android drivers via eglGetProcAddress, but many core symbols
        // are in the GLESv3 soname).
        let candidates = [
            "libGLESv3.so",
            "/system/lib64/libGLESv3.so",
            "/vendor/lib64/libGLESv3.so",
            "libGLESv2.so",
            "/system/lib64/libGLESv2.so",
        ];
        for path in candidates {
            match unsafe { libloading::Library::new(path) } {
                Ok(lib) => {
                    log(&format!("[GLBridge] loaded GLES driver: {path}"));
                    return Some(GlesDriver { lib });
                }
                Err(e) => log(&format!("[GLBridge] {path}: {e}")),
            }
        }
        log("[GLBridge] FAILED to load any GLESv3/v2 library");
        None
    })
    .as_ref()
}

/// Resolved driver entry points, keyed by the address of the NUL-terminated name literal.
///
/// Resolving costs a `dlsym` (plus a `CString` allocation in the `eglGetProcAddress`
/// fallback) and the previous code paid that on *every* forwarded GL call — a `dlsym` per
/// draw call. Every name here is a `'static` literal, so its address identifies it. The
/// whole table is dropped when the EGL context changes, because `eglGetProcAddress` may
/// hand back context-specific pointers.
static DRIVER_CACHE: Mutex<Vec<(usize, usize, usize)>> = Mutex::new(Vec::new());

/// Drops every memoized driver entry point. Called when the GL context changes.
fn clear_driver_cache() {
    DRIVER_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// Memoized [`driver_fn`]. Callers pass the same `'static` name every time.
pub(crate) fn driver_fn_cached<T: Copy>(name: &'static str) -> Option<T> {
    trace_call(name);
    let key = (name.as_ptr() as usize, name.len());
    {
        let cache = DRIVER_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, _, addr)) = cache
            .iter()
            .find(|(p, l, _)| *p == key.0 && *l == key.1)
        {
            if *addr == 0 {
                return None;
            }
            // SAFETY: the cached value came from `driver_fn`, which stores only pointers
            // obtained from dlsym / eglGetProcAddress for this exact symbol name.
            let addr = *addr;
            return Some(unsafe { std::mem::transmute_copy::<usize, T>(&addr) });
        }
    }
    // Resolve outside the lock: dlsym must not run while the cache is borrowed.
    let resolved = unsafe { driver_fn::<T>(name) };
    let Some(f) = resolved else {
        // Do NOT cache a miss. `eglGetProcAddress` can legitimately return null before a
        // context exists, and caching that would leave the symbol permanently unresolvable
        // for the rest of the process — which shows up much later as a version-specific
        // failure, since modern Minecraft resolves far more extension entry points than the
        // fixed-function path does.
        return None;
    };
    // SAFETY: `driver_fn` already checked that T has pointer size, so this copies the
    // function pointer's bits verbatim into a pointer-sized integer.
    let ptr: usize = unsafe { std::mem::transmute_copy::<T, usize>(&f) };
    let mut cache = DRIVER_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if !cache.iter().any(|(p, l, _)| *p == key.0 && *l == key.1) {
        cache.push((key.0, key.1, ptr));
    }
    Some(f)
}

/// Resolve a GLES driver symbol by name. Safe to call before a context exists.
pub(crate) unsafe fn driver_fn<T: Copy>(name: &str) -> Option<T> {
    if std::mem::size_of::<T>() != std::mem::size_of::<*const c_void>() {
        log(&format!("[GLBridge] bad fn size for {name}"));
        return None;
    }
    // 1) dlsym from libGLESv3/v2
    if let Some(drv) = gles_driver() {
        let mut buf = [0u8; 128];
        if name.len() < buf.len() {
            buf[..name.len()].copy_from_slice(name.as_bytes());
            buf[name.len()] = 0;
            if let Ok(s) = unsafe { drv.lib.get::<T>(&buf[..=name.len()]) } {
                return Some(*s);
            }
        }
    }
    // 2) eglGetProcAddress from system libEGL (extensions + some core)
    if let Some(ptr) = sys_egl_get_proc(name) {
        if !ptr.is_null() {
            return Some(std::mem::transmute_copy(&ptr));
        }
    }
    None
}

/// System eglGetProcAddress (not our export) — used for GLES extension lookup.
fn sys_egl_get_proc(name: &str) -> Option<*const c_void> {
    static GPA: OnceLock<usize> = OnceLock::new();
    let addr = *GPA.get_or_init(|| {
        let paths = [
            "/system/lib64/libEGL.so",
            "/vendor/lib64/libEGL.so",
            "libEGL.so",
        ];
        for path in paths {
            if let Ok(lib) = unsafe { libloading::Library::new(path) } {
                let ptr = unsafe {
                    lib.get::<unsafe extern "C" fn()>(b"eglGetProcAddress\0")
                        .map(|s| *s as usize)
                        .unwrap_or(0)
                };
                std::mem::forget(lib);
                if ptr != 0 {
                    return ptr;
                }
            }
        }
        0
    });
    if addr == 0 {
        return None;
    }
    type F = unsafe extern "C" fn(*const c_char) -> *const c_void;
    let f: F = unsafe { std::mem::transmute(addr) };
    let c = std::ffi::CString::new(name).ok()?;
    Some(unsafe { f(c.as_ptr()) })
}

static SPOOF_VENDOR: &[u8] = b"RustRenderer\0";
static SPOOF_RENDERER: &[u8] = b"RustRenderer GLES passthrough\0";
static SPOOF_EXTENSIONS: &[u8] = b"\0"; // empty; use glGetStringi when needed

#[no_mangle]
pub extern "C" fn glGetString(name: u32) -> *const u8 {
    // Never return null for the strings LWJGL/Minecraft always query — null here = instant crash.
    const GL_VENDOR: u32 = 0x1F00;
    const GL_RENDERER: u32 = 0x1F01;
    const GL_EXTENSIONS: u32 = 0x1F03;

    if spoof_gl() {
        match name {
            GL_VERSION => return SPOOF_VERSION.as_ptr(),
            GL_SHADING_LANGUAGE_VERSION => return SPOOF_GLSL.as_ptr(),
            GL_VENDOR => return SPOOF_VENDOR.as_ptr(),
            GL_RENDERER => return SPOOF_RENDERER.as_ptr(),
            GL_EXTENSIONS => return merged_extension_string().as_ptr(),
            _ => {}
        }
    }

    // Prefer live driver if a context is up
    if let Some(be) = backend() {
        let p = be.get_string(name);
        if !p.is_null() {
            return p;
        }
    }

    // Direct driver call (works once a GLES context is current)
    unsafe {
        type F = unsafe extern "C" fn(u32) -> *const u8;
        if let Some(f) = driver_fn_cached::<F>("glGetString") {
            let p = f(name);
            if !p.is_null() {
                return p;
            }
        }
    }

    // Last-resort non-null fallbacks
    match name {
        GL_VENDOR => SPOOF_VENDOR.as_ptr(),
        GL_RENDERER => SPOOF_RENDERER.as_ptr(),
        GL_VERSION => SPOOF_VERSION.as_ptr(),
        GL_SHADING_LANGUAGE_VERSION => SPOOF_GLSL.as_ptr(),
        GL_EXTENSIONS => SPOOF_EXTENSIONS.as_ptr(),
        _ => {
            errors().set(GL_INVALID_VALUE);
            SPOOF_EXTENSIONS.as_ptr()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetIntegerv(pname: u32, data: *mut i32) {
    // Fog queries are answered from recorded state: ES 3.x has no fog, so forwarding them
    // raises GL_INVALID_ENUM and desynchronises the game's cached fog state.
    if let Some(v) = fixed_func::fog_query_i(pname) {
        if !data.is_null() {
            *data = v;
        }
        return;
    }
    if data.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    if spoof_gl() {
        match pname {
            GL_MAJOR_VERSION => { *data = 3; return; }
            GL_MINOR_VERSION => { *data = 3; return; }
            0x9126 => { *data = 0x0001; return; } // GL_CONTEXT_PROFILE_MASK = CORE_PROFILE_BIT
            GL_NUM_EXTENSIONS => {
                *data = merged_extensions().len() as i32;
                return;
            }
            _ => {}
        }
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv") {
        f(pname, data);
        return;
    }
    // Safe zeros rather than leaving uninitialized memory for the caller
    *data = 0;
    errors().set(GL_INVALID_OPERATION);
}

/// Extensions this layer adds on top of the driver, filtered by what the device supports.
///
/// The list is computed from [`gles3::probe`] rather than fixed, so a device without a
/// feature is not told it has one. Only advertise an alias whose entry points we actually
/// export, or whose semantics are backed by GLES 3.x or by an implementation in
/// gl-compat.
fn compat_extensions() -> Vec<&'static [u8]> {
    gles3::supported_aliases(gles3::caps())
}

const GL_NUM_EXTENSIONS: u32 = 0x821D;
const GL_EXTENSIONS: u32 = 0x1F03;

type GetIntFn = unsafe extern "C" fn(u32, *mut i32);
type GetStringiFn = unsafe extern "C" fn(u32, u32) -> *const u8;
type GetStringFn = unsafe extern "C" fn(u32) -> *const u8;

/// Appends `name` as a NUL-terminated entry unless it is empty or already present.
fn push_extension(out: &mut Vec<Vec<u8>>, name: &[u8]) {
    if name.is_empty() || name.contains(&0) {
        return;
    }
    let entry = [name, &[0u8]].concat();
    if !out.iter().any(|e| e == &entry) {
        out.push(entry);
    }
}

/// Appends the driver's own extension names to `out` and returns how many were found.
///
/// # Safety
/// Calls into the GL driver, so a context must be current. Reached only from GL entry
/// points (`glGetString`, `glGetStringi`, `glGetIntegerv`), which guarantees that.
unsafe fn collect_driver_extensions(out: &mut Vec<Vec<u8>>) -> usize {
    let mut count = 0i32;
    if let Some(get) = driver_fn_cached::<GetIntFn>("glGetIntegerv") {
        get(GL_NUM_EXTENSIONS, &mut count);
    }
    let before = out.len();
    if let Some(stringi) = driver_fn_cached::<GetStringiFn>("glGetStringi") {
        for i in 0..count.max(0) as u32 {
            let p = stringi(GL_EXTENSIONS, i);
            if p.is_null() {
                break;
            }
            push_extension(out, CStr::from_ptr(p as *const c_char).to_bytes());
        }
    } else if let Some(get_string) = driver_fn_cached::<GetStringFn>("glGetString") {
        let p = get_string(GL_EXTENSIONS);
        if !p.is_null() {
            for name in CStr::from_ptr(p as *const c_char).to_bytes().split(|c| *c == b' ') {
                push_extension(out, name);
            }
        }
    }
    out.len() - before
}

/// Holds the most recently built list so pointers handed to callers stay valid for as long
/// as GL promises (until the next `glGetStringi`).
static MERGED: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
/// Set once the driver has reported at least one extension. Until then the query may have
/// happened before a context existed, so the list is rebuilt instead of cached.
static DRIVER_LISTED: AtomicBool = AtomicBool::new(false);

/// Every extension name this layer reports, NUL-terminated: the driver's own list first,
/// then our aliases, without duplicates. `glGetStringi` and `GL_NUM_EXTENSIONS` must agree
/// on this list — serving the two from different sources makes an iterator stop early.
fn merged_extensions() -> Vec<Vec<u8>> {
    {
        let cached = MERGED.lock().unwrap_or_else(|e| e.into_inner());
        if DRIVER_LISTED.load(Ordering::Relaxed) && !cached.is_empty() {
            return cached.clone();
        }
    }

    let mut out: Vec<Vec<u8>> = Vec::new();
    // SAFETY: only reachable from a GL entry point, so a context is current.
    let driver_count = unsafe { collect_driver_extensions(&mut out) };
    if driver_count > 0 {
        DRIVER_LISTED.store(true, Ordering::Relaxed);
    }
    for name in compat_extensions() {
        // The table stores each name NUL-terminated; push_extension adds its own.
        let name = name.strip_suffix(&[0u8]).unwrap_or(name);
        push_extension(&mut out, name);
    }
    if driver_count > 0 {
        static LOGGED: AtomicBool = AtomicBool::new(false);
        if !LOGGED.swap(true, Ordering::Relaxed) {
            log(&format!(
                "[GLCompat] advertising {driver_count} driver + {} compatibility extensions",
                out.len() - driver_count
            ));
        }
    }
    if !gles3::caps().valid {
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::Relaxed) {
            log("[GLCompat] capability probe failed (no context?): only ES 3.0 core aliases advertised");
        }
    }

    *MERGED.lock().unwrap_or_else(|e| e.into_inner()) = out.clone();
    out
}

/// `GL_EXTENSIONS` as one space-separated string.
///
/// GLES 3.0 defines this as an empty string — on GLES, extensions are enumerated through
/// `glGetStringi`. So this reports the compatibility aliases only; `glGetStringi` plus
/// `GL_NUM_EXTENSIONS` remain the authoritative, driver-inclusive list.
fn merged_extension_string() -> &'static [u8] {
    static STR: OnceLock<Vec<u8>> = OnceLock::new();
    STR.get_or_init(|| {
        let mut s = Vec::new();
        for e in compat_extensions() {
            if !s.is_empty() {
                s.push(b' ');
            }
            s.extend_from_slice(e.strip_suffix(&[0u8]).unwrap_or(e));
        }
        s.push(0);
        s
    })
}

#[no_mangle]
pub unsafe extern "C" fn glGetStringi(name: u32, index: u32) -> *const u8 {
    const GL_EXTENSIONS: u32 = 0x1F03;
    if name == GL_EXTENSIONS {
        // Indexed access into the same merged list GL_NUM_EXTENSIONS reports, so a caller
        // that iterates `count` entries never hits an early null.
        let exts = merged_extensions();
        if (index as usize) < exts.len() {
            return exts[index as usize].as_ptr();
        }
        errors().set(GL_INVALID_VALUE);
        return std::ptr::null();
    }
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32) -> *const u8>("glGetStringi") {
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
    match driver_fn_cached::<unsafe extern "C" fn(f32)>("glClearDepthf") {
        Some(f) => f(depth as f32),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

/// Desktop `glDepthRange(double, double)` -> ES `glDepthRangef(float, float)`.
/// The fixed-function path (1.12-1.16) sets its depth range through this call.
#[no_mangle]
pub unsafe extern "C" fn glDepthRange(near: f64, far: f64) {
    match driver_fn_cached::<unsafe extern "C" fn(f32, f32)>("glDepthRangef") {
        Some(f) => f(near as f32, far as f32),
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
            // Do not abort with empty source — that freezes loading. Pass a tiny valid
            // shader so glCompileShader fails cleanly; the game can fall back.
            log(&format!("[ShaderTranslate] shader {shader} rejected: {e}"));
            if src.contains("gl_Position") || src.contains("gl_Vertex") {
                "#version 300 es
void main(){ gl_Position = vec4(0.0); }
".into()
            } else {
                "#version 300 es
precision highp float;
layout(location=0) out vec4 c;
void main(){ c = vec4(1.0); }
".into()
            }
        }
    };
    let c = match CString::new(translated) {
        Ok(c) => c,
        Err(_) => {
            errors().set(GL_INVALID_VALUE);
            return;
        }
    };
    match driver_fn_cached::<unsafe extern "C" fn(u32, i32, *const *const c_char, *const i32)>("glShaderSource") {
        Some(f) => {
            let ptr = c.as_ptr();
            f(shader, 1, &ptr, std::ptr::null());
        }
        None => errors().set(GL_INVALID_OPERATION),
    }
}


// ---- translated entry points (desktop semantics -> GLES) -------------------------------

/// Pixel-unpack state that the upload path depends on.
///
/// Tracked locally instead of queried: the previous implementation issued three
/// `glGetIntegerv` driver round-trips *per texture upload*, which for a texture-atlas-heavy
/// frame is thousands of needless calls. Every mutation of these enums arrives through
/// `glPixelStorei`, which updates this state, so the shadow cannot drift from the driver.
static UNPACK_ROW_LENGTH: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static UNPACK_SKIP_ROWS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static UNPACK_SKIP_PIXELS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static UNPACK_ALIGNMENT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(4);

const GL_UNPACK_ALIGNMENT: u32 = 0x0CF5;

use std::sync::atomic::Ordering as AtomicOrdering;

fn set_unpack_row_length(v: i32) {
    UNPACK_ROW_LENGTH.store(v, AtomicOrdering::Relaxed);
}
fn set_unpack_skip_rows(v: i32) {
    UNPACK_SKIP_ROWS.store(v, AtomicOrdering::Relaxed);
}
fn set_unpack_skip_pixels(v: i32) {
    UNPACK_SKIP_PIXELS.store(v, AtomicOrdering::Relaxed);
}
fn set_unpack_alignment(v: i32) {
    UNPACK_ALIGNMENT.store(v, AtomicOrdering::Relaxed);
}

/// True when pixel-unpack state is default (no row length / skip), so a tight copy is valid.
fn unpack_is_tight() -> bool {
    UNPACK_ROW_LENGTH.load(AtomicOrdering::Relaxed) == 0
        && UNPACK_SKIP_ROWS.load(AtomicOrdering::Relaxed) == 0
        && UNPACK_SKIP_PIXELS.load(AtomicOrdering::Relaxed) == 0
}

/// Records unpack state changes that matter to the BGRA/BGR conversion path.
fn track_pixel_store(pname: u32, value: i32) {
    match pname {
        0x0CF2 => set_unpack_row_length(value),  // GL_UNPACK_ROW_LENGTH
        0x0CF3 => set_unpack_skip_rows(value),   // GL_UNPACK_SKIP_ROWS
        0x0CF4 => set_unpack_skip_pixels(value), // GL_UNPACK_SKIP_PIXELS
        GL_UNPACK_ALIGNMENT => set_unpack_alignment(value),
        _ => {}
    }
}

/// For BGRA/BGR uploads returns a converted RGB(A) copy; None means use caller data as-is.
pub(crate) unsafe fn convert_pixel_upload(w: i32, h: i32, f: u32, ty: u32, d: *const c_void) -> Option<(u32, u32, Vec<u8>)> {
    if d.is_null() || w <= 0 || h <= 0 {
        return None;
    }
    if !unpack_is_tight() {
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

/// Renderbuffer storage has no format/type pair, so only the BGRA/BGR internal-format
/// aliases need translating before the call reaches GLES.
/// Forwards to the driver and keeps the unpack shadow the BGRA/BGR conversion path reads,
/// so uploads no longer query the driver for these enums.
#[no_mangle]
pub unsafe extern "C" fn glPixelStorei(n: u32, v: i32) {
    track_pixel_store(n, v);
    match driver_fn_cached::<unsafe extern "C" fn(u32, i32)>("glPixelStorei") {
        Some(f) => f(n, v),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

/// Keeps the `GL_ARRAY_BUFFER` shadow in step with the driver.
#[no_mangle]
pub unsafe extern "C" fn glBindBuffer(t: u32, b: u32) {
    if t == 0x8892 /* GL_ARRAY_BUFFER */ {
        set_array_buffer_binding(b);
    } else if t == 0x8893 /* GL_ELEMENT_ARRAY_BUFFER */ {
        // GLES buffer names are per-target, so a buffer created via glCreateBuffers only has
        // storage on the target it was first filled on. Materialise it on this target too.
        vertex_state::note_buffer_target(b, t);
    }
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        Some(f) => f(t, b),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

/// Attaches a texture, redirecting to the renderbuffer substitute when a multisample depth
/// texture could not be represented in ES. See `dsa_named` for why that substitution exists.
#[no_mangle]
pub unsafe extern "C" fn glFramebufferTexture2D(t: u32, a: u32, tt: u32, tex: u32, l: i32) {
    const GL_TEXTURE_2D_MULTISAMPLE: u32 = 0x9100;
    const GL_RENDERBUFFER: u32 = 0x8D41;
    const GL_FRAMEBUFFER: u32 = 0x8D40;
    if tt == GL_TEXTURE_2D_MULTISAMPLE && matches!(a, 0x8D00 | 0x8D20 | 0x821A) {
        if let Some(rbo) = named_objects::msaa_substitute_for(tex) {
            if let Some(f) =
                driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, u32)>(
                    "glFramebufferRenderbuffer",
                )
            {
                f(GL_FRAMEBUFFER, a, GL_RENDERBUFFER, rbo);
                return;
            }
        }
    }
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, u32, i32)>(
        "glFramebufferTexture2D",
    ) {
        Some(f) => f(t, a, tt, tex, l),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glRenderbufferStorage(t: u32, f: u32, w: i32, h: i32) {
    let f2 = format_translate::map_renderbuffer_internal_format(f);
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32, i32, i32)>("glRenderbufferStorage") {
        Some(g) => g(t, f2, w, h),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTexImage2D(
    t: u32, l: i32, ifmt: i32, w: i32, h: i32, b: i32, f: u32, ty: u32, d: *const c_void,
) {
    // Desktop proxy texture probe (Minecraft max texture size detection)
    let max_tex = backend().map(|b| b.capabilities().max_texture_size).unwrap_or(2048);
    if fixed_func::handle_proxy_tex_image(t, w, h, max_tex) {
        return;
    }
    // The whole format triple has to be mapped, not just the internal format: ES requires a
    // sized depth internal format to be paired with a matching type.
    let (ifmt2, f2, ty2) = format_translate::map_upload_format(ifmt, f, ty);
    let f = f2;
    let ty = ty2;
    let conv = convert_pixel_upload(w, h, f, ty, d);
    let (f2, ty2, ptr) = match &conv {
        Some((nf, nty, v)) => (*nf, *nty, v.as_ptr() as *const c_void),
        None => (format_translate::map_external_format(f), ty, d),
    };
    type F = unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void);
    match driver_fn_cached::<F>("glTexImage2D") {
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
    match driver_fn_cached::<F>("glTexSubImage2D") {
        Some(g) => g(t, l, x, y, w, h, f2, ty2, ptr),
        None => errors().set(GL_INVALID_OPERATION),
    }
}


/// `GL_TEXTURE_MAX_ANISOTROPY_EXT` only exists where the driver advertises it, and the
/// usable value is capped by `GL_MAX_TEXTURE_MAX_ANISOTROPY_EXT`. Deciding once keeps the
/// four `glTexParameter*` wrappers from each re-deriving it.
fn anisotropy_supported() -> bool {
    gles3::caps().has(b"GL_EXT_texture_filter_anisotropic\0") && gles3::caps().max_anisotropy > 1
}

/// Translates a texture pname using the probed capabilities.
fn map_tex_parameter(pname: u32) -> Option<u32> {
    fixed_func::map_tex_parameter_with(pname, anisotropy_supported())
}

#[no_mangle]
pub unsafe extern "C" fn glTexParameteri(t: u32, p: u32, v: i32) {
    let Some(p2) = map_tex_parameter(p) else {
        return; // desktop-only pname — drop silently
    };
    let is_wrap = matches!(p2, 0x2802 | 0x2803 | 0x8072);
    if is_wrap && v == format_translate::GL_CLAMP_TO_BORDER && !gles3::caps().has_border_clamp() {
        log("[GLCompat] GL_CLAMP_TO_BORDER → clamp-to-edge (driver lacks EXT_texture_border_clamp)");
    }
    // Without the border-clamp extension, CLAMP_TO_BORDER is an invalid enum in ES.
    let v2 = if is_wrap && !gles3::caps().has_border_clamp() {
        format_translate::map_wrap(v)
    } else if is_wrap {
        v
    } else {
        v
    };
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32, i32)>("glTexParameteri") {
        Some(g) => g(t, p2, v2),
        None => errors().set(GL_INVALID_OPERATION),
    }
}
#[no_mangle]
pub unsafe extern "C" fn glTexParameterf(t: u32, p: u32, v: f32) {
    let Some(p2) = map_tex_parameter(p) else { return; };
    let is_wrap = matches!(p2, 0x2802 | 0x2803 | 0x8072);
    let v2 = if is_wrap { format_translate::map_wrap(v as i32) as f32 } else { v };
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32, f32)>("glTexParameterf") {
        Some(g) => g(t, p2, v2),
        None => {
            // fall back to integer path
            match driver_fn_cached::<unsafe extern "C" fn(u32, u32, i32)>("glTexParameteri") {
                Some(g) => g(t, p2, v2 as i32),
                None => errors().set(GL_INVALID_OPERATION),
            }
        }
    }
}

/// Vector forms take the same state as the scalar ones. Wrap modes live in `params[0]`,
/// and for every other pname GL requires the array length to match, so forwarding the
/// caller's pointer unchanged is correct once pname has been translated.
#[no_mangle]
pub unsafe extern "C" fn glTexParameteriv(t: u32, p: u32, params: *const i32) {
    if params.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    let Some(p2) = map_tex_parameter(p) else {
        return; // desktop-only pname — drop silently
    };
    if !matches!(p2, 0x2802 | 0x2803 | 0x8072) {
        match driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const i32)>("glTexParameteriv") {
            Some(g) => g(t, p2, params),
            None => errors().set(GL_INVALID_OPERATION),
        }
        return;
    }
    // CLAMP/CLAMP_TO_BORDER are not in ES 3.x: swap in the translated value. The caller's
    // buffer is const, so translate through a one-element scratch array.
    let wrapped = [format_translate::map_wrap(*params)];
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const i32)>("glTexParameteriv") {
        Some(g) => g(t, p2, wrapped.as_ptr()),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTexParameterfv(t: u32, p: u32, params: *const f32) {
    if params.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    let Some(p2) = map_tex_parameter(p) else {
        return; // desktop-only pname — drop silently
    };
    if !matches!(p2, 0x2802 | 0x2803 | 0x8072) {
        match driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const f32)>("glTexParameterfv") {
            Some(g) => g(t, p2, params),
            None => errors().set(GL_INVALID_OPERATION),
        }
        return;
    }
    let wrapped = [format_translate::map_wrap(*params as i32) as f32];
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const f32)>("glTexParameterfv") {
        Some(g) => g(t, p2, wrapped.as_ptr()),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetTexLevelParameteriv(target: u32, level: i32, pname: u32, params: *mut i32) {
    if fixed_func::handle_get_tex_level_parameter(target, level, pname, params) {
        return;
    }
    match driver_fn_cached::<unsafe extern "C" fn(u32, i32, u32, *mut i32)>("glGetTexLevelParameteriv") {
        Some(g) => g(target, level, pname, params),
        None => {
            // ES often lacks this — soft-fail
            if !params.is_null() {
                *params = 0;
            }
            errors().set(GL_INVALID_OPERATION);
        }
    }
}


/// Desktop `glDrawBuffer(mode)` -> ES `glDrawBuffers(1, &mode)`.
/// MRT draw-buffer selection, clamped to what the device actually supports.
///
/// Shader packs ask for as many layers as they declare (Complementary uses up to eight).
/// ES 3.0 only guarantees four colour attachments, and passing the pack's full count to a
/// driver that cannot honour it raises GL_INVALID_OPERATION — which shows up as a black or
/// half-rendered world rather than a diagnosable error. Clamping keeps the frame drawing.
#[no_mangle]
pub unsafe extern "C" fn glDrawBuffers(n: i32, b: *const u32) {
    if n < 0 || (n > 0 && b.is_null()) {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    let max = gles3::caps().max_draw_buffers;
    let mut n = n;
    if max > 0 && n > max {
        static ONCE: AtomicBool = AtomicBool::new(false);
        if !ONCE.swap(true, Ordering::Relaxed) {
            log(&format!(
                "[GLCompat] glDrawBuffers: device supports {max} draw buffers, \
                 clamping the requested {n} (shader pack declares more layers than the GPU has)"
            ));
        }
        n = max;
    }
    match driver_fn_cached::<unsafe extern "C" fn(i32, *const u32)>("glDrawBuffers") {
        Some(g) => g(n, b),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glDrawBuffer(mode: u32) {
    match driver_fn_cached::<unsafe extern "C" fn(i32, *const u32)>("glDrawBuffers") {
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
    let get = driver_fn_cached::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetBufferParameteriv");
    let map = driver_fn_cached::<unsafe extern "C" fn(u32, isize, isize, u32) -> *mut c_void>("glMapBufferRange");
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
                trace_call(stringify!($name));
                match driver_fn_cached::<F>(stringify!($name)) {
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
    glAttachShader(p: u32, s: u32);
    glBindAttribLocation(p: u32, i: u32, n: *const c_char);
    glBindFramebuffer(t: u32, f: u32);
    glBindRenderbuffer(t: u32, r: u32);
    glBindTexture(t: u32, x: u32);
    glBindVertexArray(a: u32);
    glBlendColor(r: f32, g: f32, b: f32, a: f32);
    glBlendEquation(m: u32);
    glBlendEquationSeparate(a: u32, b: u32);
    glBlendFunc(s: u32, d: u32);
    glBlendFuncSeparate(a: u32, b: u32, c: u32, d: u32);
    glCheckFramebufferStatus(t: u32) -> u32;
    glClearStencil(s: i32);
    glColorMask(r: u8, g: u8, b: u8, a: u8);
    glCompileShader(s: u32);
    glCreateProgram() -> u32;
    glCreateShader(t: u32) -> u32;
    glCullFace(m: u32);
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
    glDrawElements(m: u32, c: i32, t: u32, i: *const c_void);
    glEnableVertexAttribArray(i: u32);
    glFinish();
    glFlush();
    glFramebufferRenderbuffer(t: u32, a: u32, rt: u32, r: u32);
    glFrontFace(m: u32);
    glGenBuffers(n: i32, b: *mut u32);
    glGenFramebuffers(n: i32, f: *mut u32);
    glGenRenderbuffers(n: i32, r: *mut u32);
    glGenTextures(n: i32, t: *mut u32);
    glGenVertexArrays(n: i32, a: *mut u32);
    glGenerateMipmap(t: u32);
    glGetAttribLocation(p: u32, n: *const c_char) -> i32;
    glGetBooleanv(p: u32, d: *mut u8);

    glGetProgramInfoLog(p: u32, b: i32, l: *mut i32, log: *mut c_char);
    glGetProgramiv(p: u32, n: u32, v: *mut i32);
    glGetShaderInfoLog(s: u32, b: i32, l: *mut i32, log: *mut c_char);
    glGetShaderiv(s: u32, n: u32, v: *mut i32);
    glGetUniformLocation(p: u32, n: *const c_char) -> i32;
    glIsEnabled(c: u32) -> u8;
    glLinkProgram(p: u32);
    glPolygonOffset(f: f32, u: f32);
    glReadBuffer(m: u32);
    glReadPixels(x: i32, y: i32, w: i32, h: i32, f: u32, t: u32, d: *mut c_void);
    glScissor(x: i32, y: i32, w: i32, h: i32);
    glStencilFunc(f: u32, r: i32, m: u32);
    glStencilMask(m: u32);
    glStencilOp(a: u32, b: u32, c: u32);
    glTexImage3D(t: u32, l: i32, ifmt: i32, w: i32, h: i32, dp: i32, b: i32, f: u32, ty: u32, d: *const c_void);
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
    glUnmapBuffer(t: u32) -> u8;
    // --- additional ES 3.0 / desktop-common entry points ---
    glMapBufferRange(t: u32, o: isize, l: isize, a: u32) -> *mut c_void;
    glFlushMappedBufferRange(t: u32, o: isize, l: isize);
    glCopyBufferSubData(r: u32, w: u32, ro: isize, wo: isize, s: isize);
    glGetBufferParameteriv(t: u32, n: u32, v: *mut i32);
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
    glVertexAttrib1f(i: u32, x: f32);
    glVertexAttrib2f(i: u32, x: f32, y: f32);
    glVertexAttrib3f(i: u32, x: f32, y: f32, z: f32);
    glVertexAttrib4f(i: u32, x: f32, y: f32, z: f32, w: f32);
    glVertexAttrib4Nub(i: u32, x: u8, y: u8, z: u8, w: u8);
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
    glDrawElementsInstancedBaseVertex(m: u32, c: i32, t: u32, i: *const c_void, n: i32, base: i32);
    glDrawElementsBaseVertex(m: u32, c: i32, t: u32, i: *const c_void, base: i32);
    glGetFragDataLocation(p: u32, name: *const c_char) -> i32;
    glPrimitiveRestartIndex(index: u32);
    glBindFragDataLocationIndexed(program: u32, colorNumber: u32, index: u32, name: *const c_char);
    glColorMaski(buf: u32, r: u8, g: u8, b: u8, a: u8);
    glEnablei(cap: u32, index: u32);
    glDisablei(cap: u32, index: u32);
    glBlendEquationi(buf: u32, mode: u32);
    glBlendEquationSeparatei(buf: u32, modeRGB: u32, modeAlpha: u32);
    glBlendFunci(buf: u32, src: u32, dst: u32);
    glBlendFuncSeparatei(buf: u32, srcRGB: u32, dstRGB: u32, srcAlpha: u32, dstAlpha: u32);
    glGetUniformuiv(program: u32, location: i32, params: *mut u32);
    glTexParameterIiv(target: u32, pname: u32, params: *const i32);
    glTexParameterIuiv(target: u32, pname: u32, params: *const u32);
    glGetTexParameterIiv(target: u32, pname: u32, params: *mut i32);
    glGetTexParameterIuiv(target: u32, pname: u32, params: *mut u32);
    glVertexAttribP4ui(index: u32, ty: u32, normalized: u8, value: u32);
    glVertexAttribP3ui(index: u32, ty: u32, normalized: u8, value: u32);
    glVertexAttribP2ui(index: u32, ty: u32, normalized: u8, value: u32);
    glVertexAttribP1ui(index: u32, ty: u32, normalized: u8, value: u32);
    glDrawArraysIndirect(mode: u32, indirect: *const c_void);
    glDrawElementsIndirect(mode: u32, ty: u32, indirect: *const c_void);
    glUniformMatrix2x3fv(loc: i32, count: i32, transpose: u8, value: *const f32);
    glUniformMatrix3x2fv(loc: i32, count: i32, transpose: u8, value: *const f32);
    glUniformMatrix2x4fv(loc: i32, count: i32, transpose: u8, value: *const f32);
    glUniformMatrix4x2fv(loc: i32, count: i32, transpose: u8, value: *const f32);
    glUniformMatrix3x4fv(loc: i32, count: i32, transpose: u8, value: *const f32);
    glUniformMatrix4x3fv(loc: i32, count: i32, transpose: u8, value: *const f32);
    // --- GL 3.0-3.3 entry points whose ABI GLES 3.x already matches exactly ---
    // Buffer read-back / query (Minecraft reads VBO contents; the PBO path needs these).
    glGetBufferSubData(target: u32, offset: isize, size: isize, data: *mut c_void);
    glGetBufferPointerv(target: u32, pname: u32, params: *mut *mut c_void);
    // Texture targets/queries.
    glGetTexLevelParameterfv(target: u32, level: i32, pname: u32, params: *mut f32);
    glCompressedTexImage3D(t: u32, l: i32, ifmt: u32, w: i32, h: i32, d: i32, b: i32, size: i32, data: *const c_void);
    glCompressedTexSubImage3D(t: u32, l: i32, x: i32, y: i32, z: i32, w: i32, h: i32, d: i32, ifmt: u32, size: i32, data: *const c_void);
    glCopyTexSubImage3D(t: u32, l: i32, xoff: i32, yoff: i32, zoff: i32, x: i32, y: i32, w: i32, h: i32);
    glGetCompressedTexImage(t: u32, l: i32, format: u32, size: i32, data: *mut c_void);
    // Multisample textures (GL 3.2 / ARB_multisampled_textures).
    glTexImage2DMultisample(t: u32, samples: i32, ifmt: u32, w: i32, h: i32, fixed: u8);
    glTexImage3DMultisample(t: u32, samples: i32, ifmt: u32, w: i32, h: i32, d: i32, fixed: u8);
    glGetMultisamplefv(pname: u32, index: u32, data: *mut f32);
    // Layered framebuffer attachments without an explicit layer (GL 3.1).
    glFramebufferTexture(t: u32, a: u32, tex: u32, l: i32);
    // Generic vertex attributes: the scalar/vector integer setters.
    glVertexAttribI2i(i: u32, x: i32, y: i32);
    glVertexAttribI2iv(i: u32, v: *const i32);
    glVertexAttribI2ui(i: u32, x: u32, y: u32);
    glVertexAttribI2uiv(i: u32, v: *const u32);
    glVertexAttribI3i(i: u32, x: i32, y: i32, z: i32);
    glVertexAttribI3iv(i: u32, v: *const i32);
    glVertexAttribI3ui(i: u32, x: u32, y: u32, z: u32);
    glVertexAttribI3uiv(i: u32, v: *const u32);
    glVertexAttribI4iv(i: u32, v: *const i32);
    glVertexAttribI4uiv(i: u32, v: *const u32);
    // Per-draw-buffer state queries.
    glIsEnabledi(cap: u32, index: u32) -> u8;
    glGetBooleani_v(pname: u32, index: u32, data: *mut u8);
}


/// Symbol lookup used by LWJGL/GLFW-style loaders (`glXGetProcAddress` flavour).
/// Returns null for anything unimplemented -- never a stub that pretends to work.

#[no_mangle]
pub unsafe extern "C" fn glDrawArrays(mode: u32, first: i32, count: i32) {
    // Fixed-function draw (no shader program bound): emulate with our own program.
    if fixed_draw::try_draw_arrays(mode, first, count) {
        return;
    }
    let mode = fixed_func::map_draw_mode(mode);
    type F = unsafe extern "C" fn(u32, i32, i32);
    match driver_fn_cached::<F>("glDrawArrays") {
        Some(f) => f(mode, first, count),
        None => errors().set(GL_INVALID_OPERATION),
    }
}


fn resolve_legacy_stub(n: &[u8]) -> *const c_void {
    // Fixed-pipeline / 1.x symbols LWJGL enumerates. The immediate-mode subset is emulated
    // by ff_draw, so most of these are only reached when no program is bound; the remainder
    // genuinely have no ES equivalent.
    //
    // Resolving to a callable stub is what keeps the process alive, but a *silent* stub is
    // worse than an error: a caller that depends on the behaviour gets nothing and no clue.
    // So the first resolution of each name is logged.
    const LEGACY: &[&[u8]] = &[
        b"glAccum", b"glAlphaFunc", b"glAreTexturesResident", b"glArrayElement",
        b"glBegin", b"glBitmap", b"glCallList", b"glCallLists", b"glClearAccum",
        b"glClearIndex", b"glClipPlane", b"glColor3b", b"glColor3bv", b"glColor3d",
        b"glColor3dv", b"glColor3f", b"glColor3fv", b"glColor3i", b"glColor3iv",
        b"glColor3s", b"glColor3sv", b"glColor3ub", b"glColor3ubv", b"glColor3ui",
        b"glColor3uiv", b"glColor3us", b"glColor3usv", b"glColor4b", b"glColor4bv",
        b"glColor4d", b"glColor4dv", b"glColor4fv", b"glColor4i", b"glColor4iv",
        b"glColor4s", b"glColor4sv", b"glColor4ub", b"glColor4ubv", b"glColor4ui",
        b"glColor4uiv", b"glColor4us", b"glColor4usv", b"glColorMaterial",
 b"glCopyPixels", b"glDeleteLists", b"glDrawPixels",
        b"glEdgeFlag", b"glEdgeFlagPointer", b"glEdgeFlagv", b"glEnd",
        b"glEndList", b"glEvalCoord1d", b"glEvalCoord1dv", b"glEvalCoord1f",
        b"glEvalCoord1fv", b"glEvalCoord2d", b"glEvalCoord2dv", b"glEvalCoord2f",
        b"glEvalCoord2fv", b"glEvalMesh1", b"glEvalMesh2", b"glEvalPoint1",
        b"glEvalPoint2", b"glFeedbackBuffer", b"glFogf", b"glFogfv", b"glFogi",
        b"glFogiv", b"glFrustum", b"glGenLists", b"glGetClipPlane", b"glGetLightfv",
        b"glGetLightiv", b"glGetMapdv", b"glGetMapfv", b"glGetMapiv", b"glGetMaterialfv",
        b"glGetMaterialiv", b"glGetPixelMapfv", b"glGetPixelMapuiv", b"glGetPixelMapusv",
        b"glGetPolygonStipple", b"glGetTexEnvfv", b"glGetTexEnviv", b"glGetTexGendv",
        b"glGetTexGenfv", b"glGetTexGeniv", b"glIndexMask", b"glIndexPointer",
        b"glIndexd", b"glIndexdv", b"glIndexf", b"glIndexfv", b"glIndexi", b"glIndexiv",
        b"glIndexs", b"glIndexsv", b"glIndexub", b"glIndexubv", b"glInitNames",
        b"glInterleavedArrays", b"glIsList", b"glIsTextureEXT", b"glLightModelf",
        b"glLightModelfv", b"glLightModeli", b"glLightModeliv", b"glLightf",
        b"glLightfv", b"glLighti", b"glLightiv", b"glLineStipple", b"glListBase",
 b"glLoadMatrixd", b"glLoadMatrixf", b"glLoadName",
        b"glLoadTransposeMatrixd", b"glLoadTransposeMatrixf", b"glMap1d", b"glMap1f",
        b"glMap2d", b"glMap2f", b"glMapGrid1d", b"glMapGrid1f", b"glMapGrid2d",
        b"glMapGrid2f", b"glMaterialf", b"glMaterialfv", b"glMateriali", b"glMaterialiv",
 b"glMultMatrixd", b"glMultMatrixf", b"glMultTransposeMatrixd",
        b"glMultTransposeMatrixf", b"glNewList", b"glNormal3b", b"glNormal3bv",
        b"glNormal3d", b"glNormal3dv", b"glNormal3f", b"glNormal3fv", b"glNormal3i",
        b"glNormal3iv", b"glNormal3s", b"glNormal3sv", b"glNormalPointer",
 b"glPassThrough", b"glPixelMapfv", b"glPixelMapuiv", b"glPixelMapusv",
        b"glPixelTransferf", b"glPixelTransferi", b"glPixelZoom", b"glPolygonMode",
        b"glPolygonStipple", b"glPopAttrib", b"glPopClientAttrib", b"glPopMatrix",
        b"glPopName", b"glPrioritizeTextures", b"glPushAttrib", b"glPushClientAttrib",
 b"glPushName", b"glRasterPos2d", b"glRasterPos2dv",
        b"glRasterPos2f", b"glRasterPos2fv", b"glRasterPos2i", b"glRasterPos2iv",
        b"glRasterPos2s", b"glRasterPos2sv", b"glRasterPos3d", b"glRasterPos3dv",
        b"glRasterPos3f", b"glRasterPos3fv", b"glRasterPos3i", b"glRasterPos3iv",
        b"glRasterPos3s", b"glRasterPos3sv", b"glRasterPos4d", b"glRasterPos4dv",
        b"glRasterPos4f", b"glRasterPos4fv", b"glRasterPos4i", b"glRasterPos4iv",
        b"glRasterPos4s", b"glRasterPos4sv", b"glRectd", b"glRectdv", b"glRectf",
        b"glRectfv", b"glRecti", b"glRectiv", b"glRects", b"glRectsv", b"glRenderMode",
        b"glRotated", b"glRotatef", b"glScaled", b"glScalef", b"glSelectBuffer",
 b"glTexCoord1d", b"glTexCoord1dv", b"glTexCoord1f",
        b"glTexCoord1fv", b"glTexCoord1i", b"glTexCoord1iv", b"glTexCoord1s",
        b"glTexCoord1sv", b"glTexCoord2d", b"glTexCoord2dv", b"glTexCoord2f",
        b"glTexCoord2fv", b"glTexCoord2i", b"glTexCoord2iv", b"glTexCoord2s",
        b"glTexCoord2sv", b"glTexCoord3d", b"glTexCoord3dv", b"glTexCoord3f",
        b"glTexCoord3fv", b"glTexCoord3i", b"glTexCoord3iv", b"glTexCoord3s",
        b"glTexCoord3sv", b"glTexCoord4d", b"glTexCoord4dv", b"glTexCoord4f",
        b"glTexCoord4fv", b"glTexCoord4i", b"glTexCoord4iv", b"glTexCoord4s",
        b"glTexCoord4sv", b"glTexCoordPointer", b"glTexEnvf", b"glTexEnvfv",
 b"glTexEnviv", b"glTexGend", b"glTexGendv", b"glTexGenf",
        b"glTexGenfv", b"glTexGeni", b"glTexGeniv", b"glTranslated", b"glTranslatef",
        b"glVertex2d", b"glVertex2dv", b"glVertex2f", b"glVertex2fv", b"glVertex2i",
        b"glVertex2iv", b"glVertex2s", b"glVertex2sv", b"glVertex3d", b"glVertex3dv",
        b"glVertex3f", b"glVertex3fv", b"glVertex3i", b"glVertex3iv", b"glVertex3s",
        b"glVertex3sv", b"glVertex4d", b"glVertex4dv", b"glVertex4f", b"glVertex4fv",
        b"glVertex4i", b"glVertex4iv", b"glVertex4s", b"glVertex4sv", b"glVertexPointer",
        b"glWindowPos2d", b"glWindowPos2dv", b"glWindowPos2f", b"glWindowPos2fv",
        b"glWindowPos2i", b"glWindowPos2iv", b"glWindowPos2s", b"glWindowPos2sv",
        b"glWindowPos3d", b"glWindowPos3dv", b"glWindowPos3f", b"glWindowPos3fv",
        b"glWindowPos3i", b"glWindowPos3iv", b"glWindowPos3s", b"glWindowPos3sv",
    ];
    for s in LEGACY {
        if n == *s {
            return legacy_noop_fn as *const c_void;
        }
    }
    std::ptr::null()
}

#[inline(never)]
pub unsafe extern "C" fn legacy_noop_fn() {}

// ---- core GL 1.x-3.3 entry points that used to resolve to NULL --------------------------------
// LWJGL resolves every GL11..GL33 function at startup. With `-Dorg.lwjgl.util.NoChecks=true`
// a NULL pointer is *called* instead of rejected, which is the `SIGSEGV pc=0x0` the 1.16.5
// OptiFine launch died with. Everything below resolves to something callable.

/// Answers fog queries from the recorded state, then falls through to the driver. ES 3.x has
/// no fog, so forwarding these pnames raises GL_INVALID_ENUM and leaves the game's cached fog
/// state disagreeing with what it last set.
#[no_mangle]
pub unsafe extern "C" fn glGetFloatv(p: u32, d: *mut f32) {
    if d.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    if let Some(v) = fixed_func::fog_query_f(p) {
        *d = v;
        return;
    }
    match driver_fn_cached::<unsafe extern "C" fn(u32, *mut f32)>("glGetFloatv") {
        Some(f) => f(p, d),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

/// Desktop `glGetDoublev` -> ES `glGetFloatv`, widened. Matrices come from the emulated
/// fixed-function stack. Unknown pnames write one value (the safest count for the caller).
#[no_mangle]
pub unsafe extern "C" fn glGetDoublev(pname: u32, data: *mut f64) {
    if data.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    if let Some(m) = fixed_func::matrix_for_pname(pname) {
        for (i, v) in m.iter().enumerate() {
            *data.add(i) = *v as f64;
        }
        return;
    }
    let count = match pname {
        0x0BA8 => 16,                       // GL_TEXTURE_MATRIX
        0x0B70 | 0x0B21 | 0x0B12 | 0x0B22 => 2, // DEPTH_RANGE, LINE_WIDTH range/point size range
        0x0C22 | 0x0B13 | 0x0B03 | 0x0BA2 | 0x0C23 => 4, // CLEAR_COLOR, VIEWPORT, ...
        _ => 1,
    };
    let mut tmp = [0f32; 16];
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, *mut f32)>("glGetFloatv") {
        f(pname, tmp.as_mut_ptr());
    }
    for i in 0..count {
        *data.add(i) = tmp[i] as f64;
    }
}

#[no_mangle]
pub unsafe extern "C" fn glPixelStoref(pname: u32, param: f32) {
    glPixelStorei(pname, param as i32);
}

/// Desktop-only core calls with no GLES 3.0 equivalent. They are accepted, logged once, and
/// raise nothing, so a mod that probes them keeps running instead of dying in native code.
pub unsafe extern "C" fn core_unsupported_noop() -> usize {
    static ONCE: AtomicBool = AtomicBool::new(false);
    if !ONCE.swap(true, Ordering::Relaxed) {
        log("[GLCompat] a desktop-only core GL call (1D textures / double attribs / conditional render) was made and ignored");
    }
    0
}

const CORE_UNSUPPORTED: &[&[u8]] = &[
    b"glTexImage1D", b"glCopyTexImage1D", b"glCopyTexSubImage1D",
    b"glCompressedTexImage1D", b"glCompressedTexSubImage1D",
    b"glPointParameteri", b"glPointParameteriv",
    b"glVertexAttrib1s", b"glVertexAttrib1d", b"glVertexAttrib2s", b"glVertexAttrib2d",
    b"glVertexAttrib3s", b"glVertexAttrib3d", b"glVertexAttrib4s", b"glVertexAttrib4d",
    b"glVertexAttrib1sv", b"glVertexAttrib1dv", b"glVertexAttrib2sv", b"glVertexAttrib2dv",
    b"glVertexAttrib3sv", b"glVertexAttrib3dv", b"glVertexAttrib4sv", b"glVertexAttrib4dv",
    b"glVertexAttrib4iv", b"glVertexAttrib4bv", b"glVertexAttrib4ubv", b"glVertexAttrib4usv",
    b"glVertexAttrib4uiv", b"glVertexAttrib4Nbv", b"glVertexAttrib4Nsv", b"glVertexAttrib4Niv",
    b"glVertexAttrib4Nubv", b"glVertexAttrib4Nusv", b"glVertexAttrib4Nuiv",
    b"glGetVertexAttribdv",
    b"glVertexAttribI1i", b"glVertexAttribI1ui", b"glVertexAttribI1iv", b"glVertexAttribI1uiv",
    b"glVertexAttribI4bv", b"glVertexAttribI4sv", b"glVertexAttribI4ubv", b"glVertexAttribI4usv",
    b"glBeginConditionalRender", b"glEndConditionalRender",
    b"glFramebufferTexture1D", b"glFramebufferTexture3D",
    b"glMultiDrawElementsBaseVertex",
];

fn resolve_core_unsupported(n: &[u8]) -> *const c_void {
    if CORE_UNSUPPORTED.iter().any(|s| *s == n) {
        core_unsupported_noop as *const c_void
    } else {
        std::ptr::null()
    }
}

// ---- immutable buffer storage (GL 4.4 / ARB_buffer_storage) --------------------------------

/// The buffer *binding* enum for a target, when it differs from the target enum itself.
/// Pure so the table can be unit tested without a context.
fn buffer_binding_pname(target: u32) -> Option<u32> {
    const PAIRS: &[(u32, u32)] = &[
        (0x8892, 0x8894), // GL_ARRAY_BUFFER
        (0x8893, 0x8895), // GL_ELEMENT_ARRAY_BUFFER
        (0x8A11, 0x8A28), // GL_UNIFORM_BUFFER
        (0x90D2, 0x90D3), // GL_SHADER_STORAGE_BUFFER
        (0x8C8E, 0x8C8F), // GL_TRANSFORM_FEEDBACK_BUFFER
        (0x8F36, 0x8F36), // GL_COPY_READ_BUFFER (binding shares the target enum)
        (0x8F37, 0x8F37), // GL_COPY_WRITE_BUFFER
    ];
    PAIRS.iter().find(|(t, _)| *t == target).map(|(_, b)| *b)
}

/// Name currently bound to `target`, or 0 when the target is unknown or nothing is bound.
unsafe fn bound_buffer_name(target: u32) -> u32 {
    let Some(pname) = buffer_binding_pname(target) else {
        return 0;
    };
    let mut v = 0i32;
    if let Some(get) = driver_fn_cached::<GetIntFn>("glGetIntegerv") {
        get(pname, &mut v);
    }
    v.max(0) as u32
}

/// Usage hint for the `glBufferData` fallback: `GL_DYNAMIC_STORAGE_BIT` asks for a
/// dynamically-updated buffer, anything else is static.
fn storage_usage(flags: u32) -> u32 {
    const GL_DYNAMIC_STORAGE_BIT: u32 = 0x0300;
    const GL_STATIC_DRAW: u32 = 0x88E4;
    const GL_DYNAMIC_DRAW: u32 = 0x88E8;
    if flags & GL_DYNAMIC_STORAGE_BIT != 0 {
        GL_DYNAMIC_DRAW
    } else {
        GL_STATIC_DRAW
    }
}

/// Mask of bits in `flags` that ask for mapped/persistent storage, which a driver without
/// `glBufferStorage` cannot provide. Pure so it can be unit tested.
const MAPPING_BITS: u32 = 0x0001 /* GL_MAP_READ_BIT */
    | 0x0002 /* GL_MAP_WRITE_BIT */
    | 0x0040 /* GL_MAP_PERSISTENT_BIT */
    | 0x0080; /* GL_MAP_COHERENT_BIT */

fn wants_mapping(flags: u32) -> bool {
    flags & MAPPING_BITS != 0
}

/// Buffers the driver created through immutable storage, where re-specifying or updating
/// the store with `glBufferData`/`glBufferSubData` is a GL_INVALID_OPERATION.
static IMMUTABLE_BUFFERS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

fn with_immutable<R>(f: impl FnOnce(&mut Vec<u32>) -> R) -> R {
    let mut g = IMMUTABLE_BUFFERS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.as_mut())
}

fn mark_immutable(buffer: u32) {
    if buffer == 0 {
        return;
    }
    with_immutable(|v| {
        if !v.contains(&buffer) {
            v.push(buffer);
        }
    });
}

fn is_immutable(buffer: u32) -> bool {
    with_immutable(|v| v.contains(&buffer))
}

/// `glBufferStorage` -> ES 3.1 `glBufferStorage` when the driver has it, otherwise a
/// `glBufferData` with the closest usage hint. The fallback cannot be immutable and cannot
/// be persistently mapped, and says so rather than pretending otherwise.
#[no_mangle]
pub unsafe extern "C" fn glBufferStorage(target: u32, size: isize, data: *const c_void, flags: u32) {
    if size <= 0 {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, isize, *const c_void, u32)>("glBufferStorage") {
        f(target, size, data, flags);
        mark_immutable(bound_buffer_name(target));
        return;
    }
    if wants_mapping(flags) {
        log(
            "[GLCompat] glBufferStorage: driver has no immutable storage; \
             mapped/persistent storage request cannot be honoured",
        );
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, isize, *const c_void, u32)>("glBufferData") {
        f(target, size, data, storage_usage(flags));
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}

/// `glBufferData` is a GL error on an immutable buffer, and re-specifies the store
/// everywhere else.
#[no_mangle]
pub unsafe extern "C" fn glBufferData(t: u32, size: isize, data: *const c_void, usage: u32) {
    if is_immutable(bound_buffer_name(t)) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    match driver_fn_cached::<unsafe extern "C" fn(u32, isize, *const c_void, u32)>("glBufferData") {
        Some(f) => f(t, size, data, usage),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

/// `glBufferSubData` updates the store, which an immutable buffer forbids.
#[no_mangle]
pub unsafe extern "C" fn glBufferSubData(t: u32, o: isize, size: isize, data: *const c_void) {
    if is_immutable(bound_buffer_name(t)) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    match driver_fn_cached::<unsafe extern "C" fn(u32, isize, isize, *const c_void)>("glBufferSubData") {
        Some(f) => f(t, o, size, data),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteBuffers(n: i32, b: *const u32) {
    if n >= 0 && !b.is_null() {
        with_immutable(|v| {
            for i in 0..n as usize {
                let id = *b.add(i);
                v.retain(|x| *x != id);
            }
        });
    }
    match driver_fn_cached::<unsafe extern "C" fn(i32, *const u32)>("glDeleteBuffers") {
        Some(f) => f(n, b),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

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
        b"glDepthRange" => glDepthRange as *const c_void,
        b"glShaderSource" => glShaderSource as *const c_void,
        b"glTexImage2D" => glTexImage2D as *const c_void,
        b"glTexSubImage2D" => glTexSubImage2D as *const c_void,
        b"glTexParameteri" => glTexParameteri as *const c_void,
        b"glDrawBuffer" => glDrawBuffer as *const c_void,
        b"glMapBuffer" => glMapBuffer as *const c_void,
        b"glBufferStorage" => glBufferStorage as *const c_void,
        b"glBufferData" => glBufferData as *const c_void,
        b"glBufferSubData" => glBufferSubData as *const c_void,
        b"glDeleteBuffers" => glDeleteBuffers as *const c_void,
        b"glBindBuffer" => glBindBuffer as *const c_void,
        b"glDrawRangeElementsBaseVertex" => gl::v3_3::glDrawRangeElementsBaseVertex as *const c_void,
        // Direct State Access (GL 4.5 / ARB_DSA), emulated in dsa.rs
        b"glCreateBuffers" => vertex_state::glCreateBuffers as *const c_void,
        b"glCreateVertexArrays" => vertex_state::glCreateVertexArrays as *const c_void,
        b"glNamedBufferData" => vertex_state::glNamedBufferData as *const c_void,
        b"glNamedBufferStorage" => vertex_state::glNamedBufferStorage as *const c_void,
        b"glNamedBufferSubData" => vertex_state::glNamedBufferSubData as *const c_void,
        b"glGetNamedBufferSubData" => vertex_state::glGetNamedBufferSubData as *const c_void,
        b"glGetNamedBufferParameteriv" => vertex_state::glGetNamedBufferParameteriv as *const c_void,
        b"glVertexArrayVertexBuffer" => vertex_state::glVertexArrayVertexBuffer as *const c_void,
        b"glVertexArrayElementBuffer" => vertex_state::glVertexArrayElementBuffer as *const c_void,
        b"glVertexArrayAttribFormat" => vertex_state::glVertexArrayAttribFormat as *const c_void,
        b"glVertexArrayAttribIFormat" => vertex_state::glVertexArrayAttribIFormat as *const c_void,
        b"glVertexArrayAttribLFormat" => vertex_state::glVertexArrayAttribLFormat as *const c_void,
        b"glVertexArrayBindingDivisor" => vertex_state::glVertexArrayBindingDivisor as *const c_void,
        // Named-object (DSA) spellings, emulated in dsa_named.rs
        b"glCreateTextures" => named_objects::glCreateTextures as *const c_void,
        b"glCreateFramebuffers" => named_objects::glCreateFramebuffers as *const c_void,
        b"glCreateRenderbuffers" => named_objects::glCreateRenderbuffers as *const c_void,
        b"glCreateSamplers" => named_objects::glCreateSamplers as *const c_void,
        b"glCreateQueries" => named_objects::glCreateQueries as *const c_void,
        b"glTextureParameteri" => named_objects::glTextureParameteri as *const c_void,
        b"glTextureParameterf" => named_objects::glTextureParameterf as *const c_void,
        b"glTextureParameteriv" => named_objects::glTextureParameteriv as *const c_void,
        b"glTextureParameterfv" => named_objects::glTextureParameterfv as *const c_void,
        b"glTextureParameterIiv" => named_objects::glTextureParameterIiv as *const c_void,
        b"glTextureParameterIuiv" => named_objects::glTextureParameterIuiv as *const c_void,
        b"glGenerateTextureMipmap" => named_objects::glGenerateTextureMipmap as *const c_void,
        b"glGetTextureParameterIiv" => named_objects::glGetTextureParameterIiv as *const c_void,
        b"glGetTextureParameterIuiv" => named_objects::glGetTextureParameterIuiv as *const c_void,
        b"glGetTextureLevelParameterfv" => named_objects::glGetTextureLevelParameterfv as *const c_void,
        b"glGetCompressedTextureImage" => named_objects::glGetCompressedTextureImage as *const c_void,
        b"glNamedFramebufferTexture" => named_objects::glNamedFramebufferTexture as *const c_void,
        b"glNamedFramebufferTextureLayer" => named_objects::glNamedFramebufferTextureLayer as *const c_void,
        b"glNamedFramebufferRenderbuffer" => named_objects::glNamedFramebufferRenderbuffer as *const c_void,
        b"glNamedFramebufferDrawBuffer" => named_objects::glNamedFramebufferDrawBuffer as *const c_void,
        b"glNamedFramebufferDrawBuffers" => named_objects::glNamedFramebufferDrawBuffers as *const c_void,
        b"glNamedFramebufferReadBuffer" => named_objects::glNamedFramebufferReadBuffer as *const c_void,
        b"glCheckNamedFramebufferStatus" => named_objects::glCheckNamedFramebufferStatus as *const c_void,
        b"glGetNamedFramebufferAttachmentParameteriv" => named_objects::glGetNamedFramebufferAttachmentParameteriv as *const c_void,
        b"glClearNamedFramebufferiv" => named_objects::glClearNamedFramebufferiv as *const c_void,
        b"glClearNamedFramebufferuiv" => named_objects::glClearNamedFramebufferuiv as *const c_void,
        b"glClearNamedFramebufferfv" => named_objects::glClearNamedFramebufferfv as *const c_void,
        b"glClearNamedFramebufferfi" => named_objects::glClearNamedFramebufferfi as *const c_void,
        b"glNamedRenderbufferStorage" => named_objects::glNamedRenderbufferStorage as *const c_void,
        b"glNamedRenderbufferStorageMultisample" => named_objects::glNamedRenderbufferStorageMultisample as *const c_void,
        b"glGetNamedRenderbufferParameteriv" => named_objects::glGetNamedRenderbufferParameteriv as *const c_void,
        b"glMapNamedBuffer" => named_objects::glMapNamedBuffer as *const c_void,
        b"glMapNamedBufferRange" => named_objects::glMapNamedBufferRange as *const c_void,
        b"glUnmapNamedBuffer" => named_objects::glUnmapNamedBuffer as *const c_void,
        b"glFlushMappedNamedBufferRange" => named_objects::glFlushMappedNamedBufferRange as *const c_void,
        b"glGetNamedBufferPointerv" => named_objects::glGetNamedBufferPointerv as *const c_void,
        b"glGetNamedBufferParameteri64v" => named_objects::glGetNamedBufferParameteri64v as *const c_void,
        b"glCopyNamedBufferSubData" => named_objects::glCopyNamedBufferSubData as *const c_void,
        b"glDrawArraysInstancedARB" => named_objects::glDrawArraysInstancedARB as *const c_void,
        b"glDrawElementsInstancedARB" => named_objects::glDrawElementsInstancedARB as *const c_void,
        b"glVertexAttribDivisorARB" => named_objects::glVertexAttribDivisorARB as *const c_void,
        b"glBindTextures" => named_objects::glBindTextures as *const c_void,
        b"glBindSamplers" => named_objects::glBindSamplers as *const c_void,
        b"glBindBuffersBase" => named_objects::glBindBuffersBase as *const c_void,
        b"glBindBuffersRange" => named_objects::glBindBuffersRange as *const c_void,
        b"glBindVertexBuffers" => named_objects::glBindVertexBuffers as *const c_void,
        b"glVertexArrayVertexBuffers" => named_objects::glVertexArrayVertexBuffers as *const c_void,
        b"glGetVertexArrayiv" => named_objects::glGetVertexArrayiv as *const c_void,
        b"glGetVertexArrayIndexediv" => named_objects::glGetVertexArrayIndexediv as *const c_void,
        b"glGetVertexArrayIndexed64iv" => named_objects::glGetVertexArrayIndexed64iv as *const c_void,
        b"glTextureStorage1D" => named_objects::glTextureStorage1D as *const c_void,
        b"glTextureSubImage1D" => named_objects::glTextureSubImage1D as *const c_void,
        b"glCompressedTextureSubImage1D" => named_objects::glCompressedTextureSubImage1D as *const c_void,
        b"glCopyTextureSubImage1D" => named_objects::glCopyTextureSubImage1D as *const c_void,
        b"glTextureBuffer" => named_objects::glTextureBuffer as *const c_void,
        b"glNamedFramebufferTextureMultiviewOVR" => named_objects::glNamedFramebufferTextureMultiviewOVR as *const c_void,
        b"glGetProgramResourceLocationIndex" => named_objects::glGetProgramResourceLocationIndex as *const c_void,
        b"glTexStorage2DMultisample" => named_objects::glTexStorage2DMultisample as *const c_void,
        b"glTexStorage3DMultisample" => named_objects::glTexStorage3DMultisample as *const c_void,
        b"glTextureStorage2DMultisample" => named_objects::glTextureStorage2DMultisample as *const c_void,
        b"glVertexArrayAttribStride" => vertex_state::glVertexArrayAttribStride as *const c_void,
        b"glVertexAttribStride" => vertex_state::glVertexAttribStride as *const c_void,
        b"glGetVertexArrayAttribStride" => vertex_state::glGetVertexArrayAttribStride as *const c_void,
        b"glGetVertexAttribStride" => vertex_state::glGetVertexAttribStride as *const c_void,
        b"glVertexArrayAttribBinding" => vertex_state::glVertexArrayAttribBinding as *const c_void,
        b"glBindVertexBuffer" => vertex_state::glBindVertexBuffer as *const c_void,
        b"glEnableVertexArrayAttrib" => vertex_state::glEnableVertexArrayAttrib as *const c_void,
        b"glDisableVertexArrayAttrib" => vertex_state::glDisableVertexArrayAttrib as *const c_void,
        b"glTextureStorage2D" => vertex_state::glTextureStorage2D as *const c_void,
        b"glTextureStorage3D" => vertex_state::glTextureStorage3D as *const c_void,
        b"glTextureSubImage2D" => vertex_state::glTextureSubImage2D as *const c_void,
        b"glTextureSubImage3D" => vertex_state::glTextureSubImage3D as *const c_void,
        b"glMemoryBarrier" => vertex_state::glMemoryBarrier as *const c_void,
        b"glMemoryBarrierByRegion" => vertex_state::glMemoryBarrierByRegion as *const c_void,
        b"glObjectLabel" => vertex_state::glObjectLabel as *const c_void,
        b"glObjectPtrLabel" => vertex_state::glObjectPtrLabel as *const c_void,
        b"glPushDebugGroup" => vertex_state::glPushDebugGroup as *const c_void,
        b"glPopDebugGroup" => vertex_state::glPopDebugGroup as *const c_void,
        b"glDebugMessageCallback" => vertex_state::glDebugMessageCallback as *const c_void,
        b"glDebugMessageCallbackARB" => vertex_state::glDebugMessageCallbackARB as *const c_void,
        b"glDebugMessageControl" => named_objects::glDebugMessageControl as *const c_void,
        b"glDebugMessageControlARB" => named_objects::glDebugMessageControlARB as *const c_void,
        b"glBindImageTexture" => named_objects::glBindImageTexture as *const c_void,
        b"glMultiDrawElementsBaseVertex" => named_objects::glMultiDrawElementsBaseVertex as *const c_void,
        b"glGetGraphicsResetStatus" => vertex_state::glGetGraphicsResetStatus as *const c_void,
        b"glMultiDrawArraysIndirect" => vertex_state::glMultiDrawArraysIndirect as *const c_void,
        b"glMultiDrawElementsIndirect" => vertex_state::glMultiDrawElementsIndirect as *const c_void,
        b"glDispatchCompute" => vertex_state::glDispatchCompute as *const c_void,
        b"glPixelStorei" => glPixelStorei as *const c_void,
        b"glPolygonMode" => glPolygonMode as *const c_void,
        b"glXGetProcAddress" | b"glXGetProcAddressARB" | b"glGetProcAddress" => {
            glXGetProcAddress as *const c_void
        }
        b"eglGetProcAddress" => eglGetProcAddress as *const c_void,
                b"glDrawArrays" => glDrawArrays as *const c_void,
        b"glMatrixMode" => glMatrixMode as *const c_void,
        b"glLoadIdentity" => glLoadIdentity as *const c_void,
        b"glPushMatrix" => glPushMatrix as *const c_void,
        b"glPopMatrix" => glPopMatrix as *const c_void,
        b"glLoadMatrixf" => glLoadMatrixf as *const c_void,
        b"glMultMatrixf" => glMultMatrixf as *const c_void,
        b"glTranslatef" => glTranslatef as *const c_void,
        b"glScalef" => glScalef as *const c_void,
        b"glRotatef" => glRotatef as *const c_void,
        b"glOrtho" => glOrtho as *const c_void,
        b"glFrustum" => glFrustum as *const c_void,
        b"glEnableClientState" => glEnableClientState as *const c_void,
        b"glDisableClientState" => glDisableClientState as *const c_void,
        b"glVertexPointer" => glVertexPointer as *const c_void,
        b"glColorPointer" => glColorPointer as *const c_void,
        b"glTexCoordPointer" => glTexCoordPointer as *const c_void,
        b"glNormalPointer" => glNormalPointer as *const c_void,
        b"glClientActiveTexture" => glClientActiveTexture as *const c_void,
        b"glActiveTexture" => glActiveTexture as *const c_void,
        b"glActiveTextureARB" => glActiveTexture as *const c_void,
        b"glClientActiveTextureARB" => glClientActiveTexture as *const c_void,
        b"glColor4f" => glColor4f as *const c_void,
        b"glColor3f" => glColor3f as *const c_void,
        b"glAlphaFunc" => glAlphaFunc as *const c_void,
        b"glFogf" => glFogf as *const c_void,
        b"glFogi" => glFogi as *const c_void,
        b"glFogfv" => glFogfv as *const c_void,
        b"glShadeModel" => glShadeModel as *const c_void,
        b"glTexEnvf" => glTexEnvf as *const c_void,
        b"glTexEnvi" => glTexEnvi as *const c_void,
        b"glTexEnvfv" => glTexEnvfv as *const c_void,
        b"glLightf" => glLightf as *const c_void,
        b"glLightfv" => glLightfv as *const c_void,
        b"glLightModeli" => glLightModeli as *const c_void,
        b"glLightModelf" => glLightModelf as *const c_void,
        b"glLightModelfv" => glLightModelfv as *const c_void,
        b"glMaterialf" => glMaterialf as *const c_void,
        b"glMaterialfv" => glMaterialfv as *const c_void,
        b"glColorMaterial" => glColorMaterial as *const c_void,
        b"glGetDoublev" => glGetDoublev as *const c_void,
        b"glPixelStoref" => glPixelStoref as *const c_void,
        b"glTexParameterf" => glTexParameterf as *const c_void,
        b"glTexParameteriv" => glTexParameteriv as *const c_void,
        b"glTexParameterfv" => glTexParameterfv as *const c_void,
        b"glRenderbufferStorage" => glRenderbufferStorage as *const c_void,
        b"glLightModeliv" => glLightModeliv as *const c_void,
        b"glFramebufferTexture2D" => glFramebufferTexture2D as *const c_void,
        b"glFogColor" => fixed_func::glFogColor as *const c_void,
        b"glGetTexLevelParameteriv" => glGetTexLevelParameteriv as *const c_void,
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
            // Immediate mode is implemented here; it must win over the stub table below.
            let imm = immediate::resolve(n);
            if !imm.is_null() {
                return imm;
            }
            let compat = gl::v3_3::resolve(n);
            if !compat.is_null() {
                return compat;
            }
            let f = forwarded(n);
            if !f.is_null() {
                return f;
            }
            // LWJGL bridge: resolve unknown names from GLES driver / eglGetProcAddress
            let name = String::from_utf8_lossy(n);
            // Try ARB/EXT/OES suffix strip for desktop aliases
            let base = name
                .strip_suffix("ARB")
                .or_else(|| name.strip_suffix("EXT"))
                .or_else(|| name.strip_suffix("OES"))
                .or_else(|| name.strip_suffix("KHR"))
                .unwrap_or(&name);
            if base.as_bytes() != n {
                let f2 = forwarded(base.as_bytes());
                if !f2.is_null() {
                    return f2;
                }
            }
            if let Some(ptr) = unsafe {
                let mut out: Option<*const c_void> = None;
                // prefer direct GLES dlsym
                if let Some(drv) = gles_driver() {
                    let mut buf = [0u8; 128];
                    if n.len() < buf.len() {
                        buf[..n.len()].copy_from_slice(n);
                        buf[n.len()] = 0;
                        if let Ok(s) = drv.lib.get::<unsafe extern "C" fn()>(&buf[..=n.len()]) {
                            out = Some(*s as *const c_void);
                        }
                    }
                }
                out
            } {
                if !ptr.is_null() {
                    return ptr;
                }
            }
            if let Some(ptr) = sys_egl_get_proc(&name) {
                if !ptr.is_null() {
                    return ptr;
                }
            }
            let stub = resolve_legacy_stub(n);
            if !stub.is_null() {
                return stub;
            }
            let core = resolve_core_unsupported(n);
            if !core.is_null() {
                return core;
            }
            // Never hand back NULL for a GL name. LWJGL resolves a function pointer once and
            // calls it directly, so a null here is SIGSEGV at address 0 on the render thread
            // with no GL error to explain it -- which is how glFogfv killed 1.16.5. A no-op
            // stub costs the feature this one entry point provides and nothing else, so
            // degrade and say so loudly instead of killing the process.
            if name.starts_with("gl") {
                static MISSING_ONCE: Mutex<Vec<String>> = Mutex::new(Vec::new());
                let first_time = MISSING_ONCE
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .all(|s| s.as_str() != name.as_ref());
                if first_time {
                    MISSING_ONCE
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(name.to_string());
                    log(&format!(
                        "[GLBridge] unresolved entry point '{name}': returning a no-op stub. \
                         Whatever needs it will misbehave, but the process survives."
                    ));
                }
                return legacy_noop_fn as *const c_void;
            }
            log(&format!("[GLBridge] Missing entry point: {name}"));
            std::ptr::null()
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

/// Some LWJGL / GLFW paths probe the WGL name even on non-Windows hosts.
#[no_mangle]
pub extern "C" fn wglGetProcAddress(name: *const c_char) -> *const c_void {
    glXGetProcAddress(name)
}




// =============================================================================
// Fixed-function desktop GL shims (matrix stack, client arrays, legacy state)
// =============================================================================

#[no_mangle] pub extern "C" fn glMatrixMode(mode: u32) { fixed_func::gl_matrix_mode(mode); }
#[no_mangle] pub extern "C" fn glLoadIdentity() { fixed_func::gl_load_identity(); }
#[no_mangle] pub extern "C" fn glPushMatrix() { fixed_func::gl_push_matrix(); }
#[no_mangle] pub extern "C" fn glPopMatrix() { fixed_func::gl_pop_matrix(); }
#[no_mangle] pub unsafe extern "C" fn glLoadMatrixf(m: *const f32) { fixed_func::gl_load_matrixf(m); }
#[no_mangle] pub unsafe extern "C" fn glMultMatrixf(m: *const f32) { fixed_func::gl_mult_matrixf(m); }
#[no_mangle] pub extern "C" fn glTranslatef(x: f32, y: f32, z: f32) { fixed_func::gl_translatef(x, y, z); }
#[no_mangle] pub extern "C" fn glScalef(x: f32, y: f32, z: f32) { fixed_func::gl_scalef(x, y, z); }
#[no_mangle] pub extern "C" fn glRotatef(a: f32, x: f32, y: f32, z: f32) { fixed_func::gl_rotatef(a, x, y, z); }
#[no_mangle] pub extern "C" fn glOrtho(l: f64, r: f64, b: f64, t: f64, n: f64, f: f64) { fixed_func::gl_ortho(l, r, b, t, n, f); }
#[no_mangle] pub extern "C" fn glFrustum(l: f64, r: f64, b: f64, t: f64, n: f64, f: f64) { fixed_func::gl_frustum(l, r, b, t, n, f); }
#[no_mangle] pub extern "C" fn glEnableClientState(cap: u32) { fixed_func::gl_enable_client_state(cap); }
#[no_mangle] pub extern "C" fn glDisableClientState(cap: u32) { fixed_func::gl_disable_client_state(cap); }
#[no_mangle] pub unsafe extern "C" fn glVertexPointer(size: i32, ty: u32, stride: i32, ptr: *const c_void) { fixed_func::gl_vertex_pointer(size, ty, stride, ptr); }
#[no_mangle] pub unsafe extern "C" fn glColorPointer(size: i32, ty: u32, stride: i32, ptr: *const c_void) { fixed_func::gl_color_pointer(size, ty, stride, ptr); }
#[no_mangle] pub unsafe extern "C" fn glTexCoordPointer(size: i32, ty: u32, stride: i32, ptr: *const c_void) { fixed_func::gl_tex_coord_pointer(size, ty, stride, ptr); }
#[no_mangle] pub unsafe extern "C" fn glNormalPointer(ty: u32, stride: i32, ptr: *const c_void) { fixed_func::gl_normal_pointer(ty, stride, ptr); }
/// Tracks the active unit for the fixed-function emulation (texture matrices and the
/// per-unit `GL_TEXTURE_2D` enable), then forwards to the driver.
#[no_mangle]
pub unsafe extern "C" fn glActiveTexture(texture: u32) {
    fixed_func::set_active_texture(texture);
    match driver_fn_cached::<unsafe extern "C" fn(u32)>("glActiveTexture") {
        Some(f) => f(texture),
        None => errors().set(GL_INVALID_OPERATION),
    }
}
#[no_mangle] pub extern "C" fn glClientActiveTexture(texture: u32) { fixed_func::gl_client_active_texture(texture); }
#[no_mangle] pub extern "C" fn glColor4f(r: f32, g: f32, b: f32, a: f32) { fixed_func::gl_color4f(r, g, b, a); }
#[no_mangle] pub extern "C" fn glColor3f(r: f32, g: f32, b: f32) { fixed_func::gl_color3f(r, g, b); }
#[no_mangle] pub extern "C" fn glAlphaFunc(func: u32, ref_v: f32) { fixed_func::gl_alpha_func(func, ref_v); }
#[no_mangle]
#[export_name = "glFogf"]
#[inline(never)]
pub extern "C" fn glFogf(pname: u32, param: f32) { fixed_func::gl_fogf(pname, param); }
#[no_mangle]
#[export_name = "glFogi"]
#[inline(never)]
pub extern "C" fn glFogi(pname: u32, param: i32) { fixed_func::gl_fogi(pname, param); }
#[no_mangle]
#[export_name = "glFogfv"]
#[inline(never)]
pub unsafe extern "C" fn glFogfv(pname: u32, params: *const f32) { fixed_func::gl_fogfv(pname, params); }

#[no_mangle]
#[export_name = "glFogiv"]
#[inline(never)]
pub unsafe extern "C" fn glFogiv(pname: u32, params: *const i32) {
    let _ = (pname, params);
}


#[no_mangle] pub extern "C" fn glShadeModel(mode: u32) { fixed_func::gl_shade_model(mode); }
#[no_mangle] pub extern "C" fn glTexEnvf(target: u32, pname: u32, param: f32) { fixed_func::gl_tex_envf(target, pname, param); }
#[no_mangle] pub extern "C" fn glTexEnvi(target: u32, pname: u32, param: i32) { fixed_func::gl_tex_envi(target, pname, param); }
#[no_mangle] pub unsafe extern "C" fn glTexEnvfv(target: u32, pname: u32, params: *const f32) { fixed_func::gl_tex_envfv(target, pname, params); }

// ---- fixed-function lighting: accepted and ignored (NOT emulated yet) ---------------------
// Without these, lookups fall through to the driver's ES1 stubs, which raise
// "OpenGL ES API version mismatch" on every call. Visual effect: surfaces render unlit.
fn warn_once(name: &'static str) {
    static SEEN: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut g = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if !g.contains(&name) {
        g.push(name);
        log(&format!("[GLCompat] {name}: fixed-function lighting is not emulated yet; call ignored"));
    }
}
#[no_mangle] pub extern "C" fn glLightf(_l: u32, _p: u32, _v: f32) { warn_once("glLightf"); }
#[no_mangle] pub unsafe extern "C" fn glLightfv(_l: u32, _p: u32, _v: *const f32) { warn_once("glLightfv"); }
#[no_mangle] pub extern "C" fn glLightModeli(_p: u32, _v: i32) { warn_once("glLightModeli"); }
#[no_mangle] pub extern "C" fn glLightModeliv(_p: u32, _v: *const i32) { warn_once("glLightModeliv"); }
#[no_mangle] pub extern "C" fn glLightModelf(_p: u32, _v: f32) { warn_once("glLightModelf"); }
#[no_mangle] pub unsafe extern "C" fn glLightModelfv(_p: u32, _v: *const f32) { warn_once("glLightModelfv"); }
#[no_mangle] pub extern "C" fn glMaterialf(_f: u32, _p: u32, _v: f32) { warn_once("glMaterialf"); }
#[no_mangle] pub unsafe extern "C" fn glMaterialfv(_f: u32, _p: u32, _v: *const f32) { warn_once("glMaterialfv"); }
#[no_mangle] pub extern "C" fn glColorMaterial(_f: u32, _m: u32) { warn_once("glColorMaterial"); }

// =============================================================================
// EGL forwarding — MobileGL-style: same .so is the EGL provider for the launcher.
// We dlopen system libEGL once and re-export every entry the egl_loader needs.
// No DT_NEEDED on libEGL (avoids linker/constructor fights in the game process).
// =============================================================================

struct SystemEgl {
    lib: libloading::Library,
}

fn sys_egl() -> Option<&'static SystemEgl> {
    static EGL: OnceLock<Option<SystemEgl>> = OnceLock::new();
    EGL.get_or_init(|| {
        let paths = [
            "/system/lib64/libEGL.so",
            "/vendor/lib64/libEGL.so",
            "libEGL.so",
        ];
        for path in paths {
            if let Ok(lib) = unsafe { libloading::Library::new(path) } {
                log(&format!("[EGL] loaded system EGL from {path}"));
                return Some(SystemEgl { lib });
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

/// Makes a context current and drops every memoized entry point when the context changes.
///
/// `eglGetProcAddress` may return context-specific pointers, so cached addresses from a
/// previous context must not be reused after a switch (or a context loss and recreate).
/// This is the one EGL entry point where that transition is observable.
#[no_mangle]
pub unsafe extern "C" fn eglMakeCurrent(
    dpy: *mut c_void,
    draw: *mut c_void,
    read: *mut c_void,
    ctx: *mut c_void,
) -> u32 {
    type F = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> u32;
    let previous = match egl_sym::<unsafe extern "C" fn() -> *mut c_void>(b"eglGetCurrentContext\0") {
        Some(get) => get(),
        None => std::ptr::null_mut(),
    };
    let mut z: u32 = 0;
    match egl_sym::<F>(b"eglMakeCurrent\0") {
        Some(f) => {
            let r = f(dpy, draw, read, ctx);
            if r != 0 && previous != ctx {
                clear_driver_cache();
            }
            return r;
        }
        None => z = 0x3003, // EGL_BAD_MATCH: nothing to call
    }
    z
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


// =============================================================================
// Mesa DRI stubs — Zalith may set LIB_MESA_NAME to our .so when the V2 plugin
// is not counted as selectedRendererPlugin. Returning null extensions avoids
// a hard crash inside the Mesa loader.
// =============================================================================

#[no_mangle]
pub extern "C" fn __driDriverGetExtensions() -> *const *const c_void {
    log("[GLBridge] __driDriverGetExtensions stub (not a Mesa driver)");
    std::ptr::null()
}

#[no_mangle]
pub extern "C" fn __driDriverGetExtensions_zink() -> *const *const c_void {
    __driDriverGetExtensions()
}

#[no_mangle]
pub extern "C" fn __driDriverGetExtensions_virtio_gpu() -> *const *const c_void {
    __driDriverGetExtensions()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises tests that touch process-global state -- the driver entry-point cache and
    /// the merged extension list. They raced once a new test began resolving names, which made
    /// an unrelated cache assertion fail intermittently.
    pub(crate) fn global_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn name_of(entry: &[u8]) -> &[u8] {
        entry.strip_suffix(&[0u8]).unwrap_or(entry)
    }

    /// One representative entry point per advertised extension. Claiming an extension
    /// makes a client bind the call it advertises; if the layer resolves that symbol to
    /// null, the probe succeeded and the call crashes. `GL_KHR_debug` used to fail this.
    const ADVERTISED_ENTRY_POINTS: &[(&str, &[&str])] = &[
        ("GL_ARB_vertex_array_object", &["glGenVertexArrays", "glBindVertexArray", "glDeleteVertexArrays"]),
        ("GL_ARB_explicit_attrib_location", &["glBindAttribLocation", "glGetAttribLocation"]),
        ("GL_ARB_explicit_uniform_location", &["glGetUniformLocation"]),
        ("GL_ARB_instanced_arrays", &["glVertexAttribDivisor", "glDrawArraysInstanced"]),
        ("GL_ARB_draw_instanced", &["glDrawArraysInstanced", "glDrawElementsInstanced"]),
        ("GL_ARB_uniform_buffer_object", &["glBindBufferBase", "glBindBufferRange", "glGetUniformBlockIndex", "glUniformBlockBinding"]),
        ("GL_ARB_map_buffer_range", &["glMapBufferRange", "glFlushMappedBufferRange"]),
        ("GL_ARB_framebuffer_object", &["glGenFramebuffers", "glBindFramebuffer", "glFramebufferTexture2D", "glGenRenderbuffers"]),
        ("GL_ARB_texture_storage", &["glTexStorage2D", "glTexStorage3D"]),
        ("GL_ARB_copy_buffer", &["glCopyBufferSubData"]),
        ("GL_ARB_sync", &["glFenceSync", "glClientWaitSync", "glDeleteSync", "glWaitSync"]),
        ("GL_ARB_sampler_objects", &["glGenSamplers", "glBindSampler", "glIsSampler"]),
        ("GL_ARB_half_float_pixel", &["glTexImage2D", "glRenderbufferStorage"]),
        ("GL_ARB_half_float_vertex", &["glVertexAttribPointer"]),
        ("GL_ARB_vertex_type_2_10_10_10_rev", &["glVertexAttribP1ui", "glVertexAttribP4uiv"]),
        ("GL_ARB_multi_bind", &["glBindTextureUnit"]),
        ("GL_ARB_get_program_binary", &["glGetProgramBinary", "glProgramBinary"]),
        ("GL_ARB_direct_state_access", &["glGetTextureParameteriv", "glGetTextureImage", "glGetTextureLevelParameteriv"]),
        ("GL_ARB_program_interface_query", &["glGetProgramInterfaceiv", "glGetProgramResourceIndex", "glGetProgramResourceName"]),
        ("GL_EXT_texture_filter_anisotropic", &["glTexParameterf", "glTexParameteri"]),
        ("GL_OES_element_index_uint", &["glDrawElements", "glDrawElementsBaseVertex"]),
        ("GL_EXT_color_buffer_float", &["glTexImage2D", "glRenderbufferStorage"]),
        ("GL_EXT_color_buffer_half_float", &["glTexImage2D", "glRenderbufferStorage"]),
        ("GL_EXT_texture_format_BGRA8888", &["glTexImage2D", "glTexSubImage2D", "glRenderbufferStorage"]),
    ];

    #[test]
    fn every_advertised_extension_exports_its_entry_points() {
        // Aliases are now capability-filtered, so "advertised" depends on the probed device.
        // With no GL context the probe is invalid and only the always-on set is offered;
        // check the entry points for whatever is actually advertised, and additionally
        // require the always-on set to be present unconditionally.
        let advertised_now: Vec<&[u8]> = compat_extensions();
        for (ext, entry_points) in ADVERTISED_ENTRY_POINTS {
            for entry in *entry_points {
                assert!(
                    !resolve_proc(entry.as_bytes()).is_null(),
                    "{ext} may be advertised but {entry} resolves to null"
                );
            }
        }
        // Whatever we did advertise must have a backing entry point, and every always-on
        // alias must be advertised even with no context.
        for ext in ADVERTISED_ENTRY_POINTS {
            let is_always_on = gles3::supported_aliases(&gles3::GlesCapabilities {
                valid: true,
                ..Default::default()
            })
            .iter()
            .any(|e| name_of(e) == ext.0.as_bytes());
            if is_always_on {
                assert!(
                    advertised_now.iter().any(|e| name_of(e) == ext.0.as_bytes()),
                    "{} should be advertised unconditionally",
                    ext.0
                );
            }
        }
    }

    #[test]
    fn advertised_list_is_covered_by_the_entry_point_table() {
        for ext in compat_extensions() {
            let name = name_of(ext);
            let listed = ADVERTISED_ENTRY_POINTS
                .iter()
                .any(|(e, _)| e.as_bytes() == name);
            assert!(
                listed,
                "{} is advertised without a tested entry point in ADVERTISED_ENTRY_POINTS",
                String::from_utf8_lossy(name)
            );
        }
    }

    #[test]
    fn push_extension_dedups_and_terminates() {
        let mut out: Vec<Vec<u8>> = Vec::new();
        push_extension(&mut out, b"GL_ARB_sync");
        push_extension(&mut out, b"GL_ARB_sync");
        push_extension(&mut out, b"");
        push_extension(&mut out, b"has\0nul");
        push_extension(&mut out, b"GL_OES_element_index_uint");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], b"GL_ARB_sync\0");
        assert_eq!(out[1], b"GL_OES_element_index_uint\0");
        assert!(out.iter().all(|e| *e.last().unwrap() == 0));
    }

    #[test]
    fn driver_entry_points_are_resolved_once_and_reused() {
        let _lock = global_test_lock();
        // The regression this guards: driver_fn used to run dlsym (plus a CString malloc)
        // on every forwarded GL call, so a draw-heavy frame paid it per draw. The cache is
        // keyed by the address of the name literal, so repeated lookups must not re-resolve.
        fn cache_len() -> usize {
            // Scoped so the guard is released: std::sync::Mutex is not reentrant.
            DRIVER_CACHE.lock().unwrap().len()
        }
        let _ = driver_fn_cached::<unsafe extern "C" fn(u32)>("glCullFace");
        let after_first = cache_len();
        for _ in 0..1000 {
            let _ = driver_fn_cached::<unsafe extern "C" fn(u32)>("glCullFace");
        }
        assert_eq!(
            cache_len(),
            after_first,
            "repeated lookups must not grow the cache (each one is re-resolving)"
        );
        // A cached miss stays a miss instead of retrying dlsym every call.
        let _ = driver_fn_cached::<unsafe extern "C" fn()>("glDefinitelyNotAGLFunction");
        let after_miss = cache_len();
        let _ = driver_fn_cached::<unsafe extern "C" fn()>("glDefinitelyNotAGLFunction");
        assert_eq!(cache_len(), after_miss);
    }

    #[test]
    fn clearing_the_driver_cache_forgets_every_entry() {
        let _lock = global_test_lock();
        driver_fn_cached::<unsafe extern "C" fn(u32)>("glCullFace");
        clear_driver_cache();
        assert!(DRIVER_CACHE.lock().unwrap().is_empty());
    }

    #[test]
    fn fixed_function_calls_do_not_resolve_to_the_no_op_stub() {
        let _lock = global_test_lock();
        // Regression guard. These are all implemented by ES 3.x and are what the 1.12-1.16
        // fixed-function path draws through. A name can be exported *and* still resolve to the
        // shared legacy no-op when it only appears in the stub table -- the call then silently
        // does nothing, which is how glLightModeliv was lost. Comparing against the stub's own
        // address catches that, where a null check would not.
        let stub = legacy_noop_fn as *const c_void;
        for name in [
            "glBegin", "glEnd", "glVertex3f", "glVertex4f", "glColor3f", "glColor4f",
            "glTexCoord2f", "glTexCoord4f", "glNormal3f", "glArrayElement", "glAlphaFunc",
            "glShadeModel", "glPushAttrib", "glPopAttrib", "glPushClientAttrib",
            "glPopClientAttrib", "glColorMaterial", "glTexGenfv", "glTexGenf", "glTexGeni",
            "glTexGeniv", "glLightModeli", "glLightModeliv", "glGetTexEnvfv", "glGetTexEnviv",
            "glGetTexGenfv", "glGetTexGeniv", "glLineStipple", "glPolygonStipple", "glFogfv",
            "glFogf", "glFogi", "glLightfv", "glMaterialfv", "glTexEnvfv", "glTexEnvi",
            "glMatrixMode", "glLoadMatrixf", "glPushMatrix", "glPopMatrix", "glTranslatef",
            "glRotatef", "glScalef", "glOrtho", "glFrustum", "glVertexPointer",
            "glColorPointer", "glTexCoordPointer", "glNormalPointer", "glEnableClientState",
            "glDisableClientState",
        ] {
            let fp = resolve_proc(name.as_bytes());
            assert!(!fp.is_null(), "{name} resolves to null");
            assert_ne!(fp, stub, "{name} resolves to the legacy no-op stub");
        }
    }

    #[test]
    fn every_exported_gl_symbol_is_reachable_through_the_resolver() {
        let _lock = global_test_lock();
        // LWJGL resolves GL functions with eglGetProcAddress, which routes through
        // resolve_proc. A `#[no_mangle]` function missing from every resolver table is
        // exported but unreachable, so LWJGL binds null and the call faults. Hand-written
        // wrappers converted out of `forward_all!` are exactly how that happens: the macro
        // generates the resolver entry, and a replacement must add its own.
        const HAND_WRITTEN: &[&str] = &[
            "glGetError", "glGetString", "glGetStringi", "glGetIntegerv", "glClearColor",
            "glClear", "glViewport", "glEnable", "glDisable", "glDepthRange", "glClearDepth",
            "glTexParameteri", "glTexParameterf", "glTexParameteriv", "glTexParameterfv",
            "glPixelStorei", "glBindBuffer", "glRenderbufferStorage", "glBufferStorage",
            "glBufferData", "glBufferSubData", "glDeleteBuffers", "glDrawArrays",
            "glShaderSource", "glDrawBuffer", "glMapBuffer", "glPolygonMode",
        ];
        for name in HAND_WRITTEN {
            assert!(
                !resolve_proc(name.as_bytes()).is_null(),
                "{name} is exported but unreachable via eglGetProcAddress"
            );
        }
    }

    #[test]
    fn unpack_state_is_tracked_without_querying_the_driver() {
        // Default is a tight copy.
        assert!(unpack_is_tight(), "fresh state must be tight");
        for (set, pname) in [
            (set_unpack_row_length as fn(i32), 0x0CF2u32),
            (set_unpack_skip_rows, 0x0CF3),
            (set_unpack_skip_pixels, 0x0CF4),
        ] {
            set(7);
            assert!(!unpack_is_tight(), "pname 0x{pname:04X} must affect the upload path");
            track_pixel_store(pname, 0);
            assert!(unpack_is_tight(), "pname 0x{pname:04X} must be resettable");
        }
    }

    #[test]
    fn array_buffer_shadow_tracks_binding() {
        set_array_buffer_binding(17);
        assert_eq!(current_array_buffer(), 17);
        // and back to default for other tests
        set_array_buffer_binding(0);
        assert_eq!(current_array_buffer(), 0);
    }

    #[test]
    fn immutable_buffer_targets_resolve_to_their_binding_enums() {
        assert_eq!(buffer_binding_pname(0x8892), Some(0x8894)); // ARRAY_BUFFER
        assert_eq!(buffer_binding_pname(0x8893), Some(0x8895)); // ELEMENT_ARRAY_BUFFER
        assert_eq!(buffer_binding_pname(0x8A11), Some(0x8A28)); // UNIFORM_BUFFER
        assert_eq!(buffer_binding_pname(0x8F36), Some(0x8F36)); // COPY_READ_BUFFER
        assert_eq!(buffer_binding_pname(0x1234), None);
    }

    #[test]
    fn storage_flags_pick_the_matching_usage() {
        const GL_DYNAMIC_STORAGE_BIT: u32 = 0x0300;
        assert_eq!(storage_usage(0), 0x88E4); // static draw
        assert_eq!(storage_usage(GL_DYNAMIC_STORAGE_BIT), 0x88E8); // dynamic draw
    }

    #[test]
    fn only_mapped_storage_triggers_the_fallback_warning() {
        assert!(!wants_mapping(0));
        assert!(!wants_mapping(0x0300)); // dynamic storage alone is honoured
        assert!(wants_mapping(0x0040)); // GL_MAP_PERSISTENT_BIT
        assert!(wants_mapping(0x0002)); // GL_MAP_WRITE_BIT
    }

    #[test]
    fn immutable_buffers_are_tracked_and_released() {
        mark_immutable(7);
        mark_immutable(7);
        assert!(is_immutable(7));
        assert!(!is_immutable(8));
        let ids = [7u32, 8u32];
        unsafe { glDeleteBuffers(2, ids.as_ptr()) };
        assert!(!is_immutable(7));
    }
}
