//! OpenGL ES backend. Phase 1: loads the system GLES driver and forwards a small subset.
//! It does NOT create an EGL context; the launcher/GLFW layer owns the context.

use libloading::Library;
use renderer_core::{Backend, BackendError, BackendKind, DeviceInfo};
use std::ffi::{c_char, CStr};

const GL_VENDOR: u32 = 0x1F00;
const GL_RENDERER: u32 = 0x1F01;
const GL_VERSION: u32 = 0x1F02;
const GL_SHADING_LANGUAGE_VERSION: u32 = 0x8B8C;

type GetStringFn = unsafe extern "C" fn(u32) -> *const u8;
type GetErrorFn = unsafe extern "C" fn() -> u32;
type ClearColorFn = unsafe extern "C" fn(f32, f32, f32, f32);
type ClearFn = unsafe extern "C" fn(u32);
type ViewportFn = unsafe extern "C" fn(i32, i32, i32, i32);
type CapFn = unsafe extern "C" fn(u32);

pub struct GlesBackend {
    // Keeps the driver mapped for as long as the fn pointers below exist.
    _lib: Library,
    info: DeviceInfo,
    get_string: GetStringFn,
    get_error: GetErrorFn,
    clear_color: ClearColorFn,
    clear: ClearFn,
    viewport: ViewportFn,
    enable: CapFn,
    disable: CapFn,
}

impl GlesBackend {
    /// Loads the driver and queries device strings.
    ///
    /// Requires a current GLES context on the calling thread; otherwise `glGetString`
    /// returns null and we return `InitFailed` instead of pretending it worked.
    pub fn new() -> Result<Self, BackendError> {
        // SAFETY: loading a system library runs its initializers. libGLESv3/v2 are platform
        // libraries with no unsound constructors; failure is handled.
        let lib = unsafe { Library::new("libGLESv3.so").or_else(|_| Library::new("libGLESv2.so")) }
            .map_err(|e| BackendError::InitFailed(format!("cannot load GLES driver: {e}")))?;

        macro_rules! sym {
            ($name:literal, $ty:ty) => {{
                // SAFETY: name and signature match the GLES 2.0+ C ABI. The fn pointer is
                // copied out and stays valid because `_lib` lives in the same struct and is
                // never unloaded while the pointer is reachable.
                let s = unsafe { lib.get::<$ty>(concat!($name, "\0").as_bytes()) }
                    .map_err(|e| BackendError::InitFailed(format!("missing {}: {e}", $name)))?;
                *s
            }};
        }

        let get_string: GetStringFn = sym!("glGetString", GetStringFn);
        let get_error: GetErrorFn = sym!("glGetError", GetErrorFn);
        let clear_color: ClearColorFn = sym!("glClearColor", ClearColorFn);
        let clear: ClearFn = sym!("glClear", ClearFn);
        let viewport: ViewportFn = sym!("glViewport", ViewportFn);
        let enable: CapFn = sym!("glEnable", CapFn);
        let disable: CapFn = sym!("glDisable", CapFn);

        let read = |name: u32| -> Result<String, BackendError> {
            // SAFETY: valid fn pointer; needs a current context, verified via null check.
            let p = unsafe { get_string(name) };
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

        Ok(Self { _lib: lib, info, get_string, get_error, clear_color, clear, viewport, enable, disable })
    }
}

// SAFETY: only fn pointers and an immutable library handle are held. GL calls must still be
// made on the context's thread; that is the caller's responsibility (spec section 24).
unsafe impl Send for GlesBackend {}
unsafe impl Sync for GlesBackend {}

impl Backend for GlesBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Gles
    }
    fn device_info(&self) -> &DeviceInfo {
        &self.info
    }
    fn clear_color(&self, r: f32, g: f32, b: f32, a: f32) {
        // SAFETY: valid fn pointer, plain value args.
        unsafe { (self.clear_color)(r, g, b, a) }
    }
    fn clear(&self, mask: u32) {
        // SAFETY: as above.
        unsafe { (self.clear)(mask) }
    }
    fn viewport(&self, x: i32, y: i32, w: i32, h: i32) {
        // SAFETY: as above.
        unsafe { (self.viewport)(x, y, w, h) }
    }
    fn enable(&self, cap: u32) {
        // SAFETY: as above.
        unsafe { (self.enable)(cap) }
    }
    fn disable(&self, cap: u32) {
        // SAFETY: as above.
        unsafe { (self.disable)(cap) }
    }
    fn get_error(&self) -> u32 {
        // SAFETY: as above.
        unsafe { (self.get_error)() }
    }
    fn get_string(&self, name: u32) -> *const u8 {
        // SAFETY: valid fn pointer; invalid enums just set GL_INVALID_ENUM in the driver.
        unsafe { (self.get_string)(name) }
    }
}
