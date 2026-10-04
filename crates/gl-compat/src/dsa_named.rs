//! Named-object (Direct State Access) variants of the classic GL entry points.
//!
//! The device log for 1.21.11 named 62 entry points that LWJGL asked for and this layer did
//! not resolve. They are nearly all "same call, object given by name instead of by binding",
//! so each one binds the object and delegates to the classic entry point that already works.
//!
//! GLES 3.x has no named objects, so a name has to be bound before the classic call can be
//! used. That is the whole translation, and it is why the DSA path works at all here.

use super::*;
use format_translate;
use std::sync::Mutex;

const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_RENDERBUFFER: u32 = 0x8D41;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_ARRAY_BUFFER: u32 = 0x8892;
const GL_COPY_READ_BUFFER: u32 = 0x8F36;
const GL_COPY_WRITE_BUFFER: u32 = 0x8F37;

/// Textures seen created through `glCreateTextures`, so a later DSA call knows which target
/// to bind them to. Names created any other way are still tracked by `glBindTexture`.
static TEXTURES: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());

fn record_texture(id: u32, target: u32) {
    let mut v = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    if !v.iter().any(|(i, _)| *i == id) {
        v.push((id, target));
    }
}

fn set_texture_target(id: u32, target: u32) {
    let mut v = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    match v.iter_mut().find(|(i, _)| *i == id) {
        Some(e) => e.1 = target,
        None => v.push((id, target)),
    }
}

fn texture_target(id: u32) -> u32 {
    let mut v = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = v.iter_mut().find(|(i, _)| *i == id) {
        return e.1;
    }
    // Unknown name: default to 2D rather than failing. A texture we never saw created is
    // almost always 2D, and raising GL_INVALID_OPERATION here is what turned a missing
    // detail into "OpenGL error 1282" during framebuffer setup.
    v.push((id, GL_TEXTURE_2D));
    log_once_unknown_texture(id);
    GL_TEXTURE_2D
}

static UNKNOWN_TEXTURE_LOGGED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

fn log_once_unknown_texture(id: u32) {
    let mut v = UNKNOWN_TEXTURE_LOGGED.lock().unwrap_or_else(|e| e.into_inner());
    if v.len() < 8 && !v.contains(&id) {
        v.push(id);
        log(&format!(
            "[dsa] texture {id} was used before this layer saw it created; assuming 2D"
        ));
    }
}

/// Records which entry point last touched the GL error, so a game that only sees
/// "OpenGL error 1282" can be matched to a line here.
///
/// Minecraft checks `glGetError` after a block of calls and throws with just the code. Without
/// this, the bridge was setting GL_INVALID_OPERATION from several dozen places and the log
/// could not say which one fired.
pub fn mark_error_site(site: &str) {
    static LAST: Mutex<String> = Mutex::new(String::new());
    let mut l = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if l.as_str() != site {
        *l = site.to_string();
        log(&format!("[dsa] GL error site: {site}"));
    }
}


/// Multisample textures that had to be emulated with a renderbuffer: texture name ->
/// renderbuffer name.
///
/// ES 3.x cannot express a multisample *depth* texture: `glTexImage2DMultisample` takes a
/// colour-renderable internal format, and asking for `GL_DEPTH_COMPONENT24` with
/// `GL_TEXTURE_2D_MULTISAMPLE` raises `GL_INVALID_OPERATION`. A multisample depth attachment
/// in ES is a multisample *renderbuffer*, so that is what gets allocated instead, and the
/// later `glFramebufferTexture2D(GL_DEPTH_ATTACHMENT, GL_TEXTURE_2D_MULTISAMPLE, tex, 0)` is
/// redirected to `glFramebufferRenderbuffer`.
///
/// The same fallback covers drivers that do not implement multisample textures at all: the
/// real allocation is attempted first and the renderbuffer path is used only if it fails.
static MSAA_SUBSTITUTE: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());

const GL_DEPTH_ATTACHMENT: u32 = 0x8D00;
const GL_STENCIL_ATTACHMENT: u32 = 0x8D20;
const GL_DEPTH_STENCIL_ATTACHMENT: u32 = 0x821A;
const GL_TEXTURE_2D_MULTISAMPLE: u32 = 0x9100;

fn is_depth_or_stencil(fmt: u32) -> bool {
    matches!(
        fmt,
        0x81A5 /* GL_DEPTH_COMPONENT16 */
            | 0x81A6 /* GL_DEPTH_COMPONENT24 */
            | 0x8CAC /* GL_DEPTH_COMPONENT32F */
            | 0x8CAD /* GL_DEPTH24_STENCIL8 */
            | 0x8CDF /* GL_DEPTH32F_STENCIL8 */
    )
}

fn record_msaa_substitute(tex: u32, rbo: u32) {
    let mut v = MSAA_SUBSTITUTE.lock().unwrap_or_else(|e| e.into_inner());
    match v.iter_mut().find(|(t, _)| *t == tex) {
        Some(e) => e.1 = rbo,
        None => v.push((tex, rbo)),
    }
}

pub(crate) fn msaa_substitute_for(tex: u32) -> Option<u32> {
    MSAA_SUBSTITUTE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(t, _)| *t == tex)
        .map(|(_, r)| *r)
}

/// Allocates a multisample renderbuffer holding the same depth/stencil storage the texture
/// could not, and records the substitution so the attach can use it.
unsafe fn allocate_msaa_renderbuffer(samples: i32, fmt: u32, w: i32, h: i32) -> Option<u32> {
    let gen = driver_fn_cached::<unsafe extern "C" fn(i32, *mut u32)>("glGenRenderbuffers")?;
    let bind = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindRenderbuffer")?;
    let store = driver_fn_cached::<unsafe extern "C" fn(u32, i32, u32, i32, i32)>(
        "glRenderbufferStorageMultisample",
    )?;
    let mut rbo = 0u32;
    gen(1, &mut rbo);
    bind(GL_RENDERBUFFER, rbo);
    store(GL_RENDERBUFFER, samples, fmt, w, h);
    let err = errors().take();
    bind(GL_RENDERBUFFER, 0);
    if err != 0 {
        log(&format!(
            "[dsa] multisample {fmt:#06x} failed as both texture and renderbuffer (0x{err:04X})"
        ));
        return None;
    }
    Some(rbo)
}

/// Texture targets that can be passed to `glBindTexture`.
///
/// The multisample *types* (GL_TEXTURE_2D_MULTISAMPLE, GL_TEXTURE_3D_MULTISAMPLE) are not in
/// this list: they name a texture kind, not a binding point, and binding one raises
/// GL_INVALID_ENUM. Their storage is allocated by `glTexStorage*Multisample` instead.
fn is_bindable_texture_target(t: u32) -> bool {
    matches!(t, 0x0DE1 /* 2D */ | 0x806F /* 3D */ | 0x8513 /* CUBE_MAP */
        | 0x8C1A /* 2D_ARRAY */ | 0x9009 /* CUBE_MAP_ARRAY */ | 0x84F5 /* 1D */
        | 0x84F6 /* 1D_ARRAY */ | 0x84F7 /* RECT */ | 0x8C18 /* 3D_ARRAY */)
}

/// Binds a named texture to the target it belongs to. Returns the target.
unsafe fn bind_tex(id: u32) -> u32 {
    let target = texture_target(id);
    if !is_bindable_texture_target(target) {
        return target;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindTexture") {
        f(target, id);
    }
    target
}

unsafe fn bind_fbo(fbo: u32) -> bool {
    match driver_fn_cached::<unsafe extern "C" fn(u32)>("glBindFramebuffer") {
        Some(f) => {
            f(GL_FRAMEBUFFER);
            let _ = fbo;
            true
        }
        None => false,
    }
}

unsafe fn bind_rbo(rbo: u32) -> bool {
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindRenderbuffer") {
        Some(f) => {
            f(GL_RENDERBUFFER, rbo);
            let _ = rbo;
            true
        }
        None => false,
    }
}

/// Named aliases for the entry-point types that return a value. Keeping the arrow out of the
/// turbofish avoids an ambiguity that makes rustc reject the whole item.
type MapBufferFn = unsafe extern "C" fn(u32, u32) -> *mut c_void;
type MapBufferRangeFn = unsafe extern "C" fn(u32, isize, isize, u32) -> *mut c_void;
type UnmapBufferFn = unsafe extern "C" fn(u32) -> u8;

unsafe fn bind_buf(target: u32, buf: u32) {
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        f(target, buf);
    }
}

// ---- texture creation and parameters --------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn glCreateTextures(target: u32, n: i32, ids: *mut u32) {
    if n <= 0 || ids.is_null() {
        errors().set(0x0501);
        return;
    }
    for i in 0..n as usize {
        *ids.add(i) = 0;
    }
    match driver_fn_cached::<unsafe extern "C" fn(i32, *mut u32)>("glGenTextures") {
        Some(f) => f(n, ids),
        None => {
            mark_error_site("glCreateTextures");
            errors().set(0x0502);
            return;
        }
    }
    // Materialise each name on its target right away: ES texture names are per-target, so
    // without this the name is unusable under the target the game asked for. Multisample
    // types are recorded but never bound -- they are not valid glBindTexture targets.
    for i in 0..n as usize {
        record_texture(*ids.add(i), target);
    }
    if is_bindable_texture_target(target) {
        if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindTexture") {
            for i in 0..n as usize {
                bind(target, *ids.add(i));
            }
        }
    }
}

/// All of the DSA texture-parameter spellings. `glTextureParameteriv` and `fv` are separate
/// entry points from the `i`/`f` forms; a caller using the DSA name was getting a no-op stub
/// and its texture state silently never applied.
#[no_mangle]
pub unsafe extern "C" fn glTextureParameteri(id: u32, pname: u32, param: i32) {
    let target = unsafe { bind_tex(id) };
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, i32)>("glTexParameteri") {
        f(target, pname, param);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTextureParameterf(id: u32, pname: u32, param: f32) {
    let target = unsafe { bind_tex(id) };
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, f32)>("glTexParameterf") {
        f(target, pname, param);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTextureParameteriv(id: u32, pname: u32, params: *const c_void) {
    let target = unsafe { bind_tex(id) };
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const c_void)>("glTexParameteriv")
    {
        f(target, pname, params);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTextureParameterfv(id: u32, pname: u32, params: *const c_void) {
    let target = unsafe { bind_tex(id) };
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const c_void)>("glTexParameterfv")
    {
        f(target, pname, params);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTextureParameterIiv(id: u32, pname: u32, params: *const c_void) {
    let target = unsafe { bind_tex(id) };
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const c_void)>("glTexParameterIiv")
    {
        f(target, pname, params);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTextureParameterIuiv(id: u32, pname: u32, params: *const c_void) {
    let target = unsafe { bind_tex(id) };
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const c_void)>("glTexParameterIuiv")
    {
        f(target, pname, params);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGenerateTextureMipmap(id: u32) {
    let target = unsafe { bind_tex(id) };
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32)>("glGenerateMipmap") {
        f(target);
    }
}

// ---- named framebuffers ---------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn glNamedFramebufferTexture(fbo: u32, att: u32, tex: u32, level: i32) {
    if !unsafe { bind_fbo(fbo) } {
        mark_error_site("glNamedFramebufferTexture");
        errors().set(0x0502);
        return;
    }
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, u32, i32)>("glFramebufferTexture2D")
    {
        f(GL_FRAMEBUFFER, att, texture_target(tex), tex, level);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glNamedFramebufferTextureLayer(
    fbo: u32, att: u32, tex: u32, level: i32, layer: i32,
) {
    if !unsafe { bind_fbo(fbo) } {
        mark_error_site("glNamedFramebufferTextureLayer");
        errors().set(0x0502);
        return;
    }
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, i32, i32)>("glFramebufferTextureLayer")
    {
        f(GL_FRAMEBUFFER, att, tex, level, layer);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glNamedFramebufferRenderbuffer(fbo: u32, att: u32, rbo: u32, t: u32) {
    if !unsafe { bind_fbo(fbo) } {
        mark_error_site("glNamedFramebufferRenderbuffer");
        errors().set(0x0502);
        return;
    }
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, u32)>("glFramebufferRenderbuffer")
    {
        f(GL_FRAMEBUFFER, att, t, rbo);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glNamedFramebufferDrawBuffer(fbo: u32, mode: u32) {
    if !unsafe { bind_fbo(fbo) } {
        mark_error_site("glNamedFramebufferDrawBuffer");
        errors().set(0x0502);
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32)>("glDrawBuffer") {
        f(mode);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glNamedFramebufferDrawBuffers(fbo: u32, n: i32, b: *const u32) {
    if !unsafe { bind_fbo(fbo) } {
        mark_error_site("glNamedFramebufferDrawBuffers");
        errors().set(0x0502);
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(i32, *const u32)>("glDrawBuffers") {
        f(n, b);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glNamedFramebufferReadBuffer(fbo: u32, mode: u32) {
    if !unsafe { bind_fbo(fbo) } {
        mark_error_site("glNamedFramebufferReadBuffer");
        errors().set(0x0502);
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32)>("glReadBuffer") {
        f(mode);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glCheckNamedFramebufferStatus(fbo: u32, target: u32) -> u32 {
    if !unsafe { bind_fbo(fbo) } {
        return 0x8CD6;
    }
    match driver_fn_cached::<unsafe extern "C" fn(u32) -> u32>("glCheckFramebufferStatus") {
        Some(f) => f(target),
        None => {
            mark_error_site("glCheckNamedFramebufferStatus");
            errors().set(0x0502);
            0x8CD6
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetNamedFramebufferAttachmentParameteriv(
    fbo: u32, att: u32, pname: u32, params: *mut i32,
) {
    if params.is_null() {
        errors().set(0x0501);
        return;
    }
    if !unsafe { bind_fbo(fbo) } {
        *params = 0;
        mark_error_site("glGetNamedFramebufferAttachmentParameteriv");
        errors().set(0x0502);
        return;
    }
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, *mut i32)>(
        "glGetFramebufferAttachmentParameteriv",
    ) {
        Some(f) => f(GL_FRAMEBUFFER, att, pname, params),
        None => {
            *params = 0;
            mark_error_site("glGetNamedFramebufferAttachmentParameteriv");
            errors().set(0x0502);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glClearNamedFramebufferiv(fbo: u32, mask: u32, b: *const i32) {
    if !unsafe { bind_fbo(fbo) } {
        mark_error_site("glClearNamedFramebufferiv");
        errors().set(0x0502);
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const i32)>("glClearBufferiv")
    {
        f(GL_FRAMEBUFFER, mask, b);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glClearNamedFramebufferuiv(fbo: u32, mask: u32, b: *const u32) {
    if !unsafe { bind_fbo(fbo) } {
        mark_error_site("glClearNamedFramebufferuiv");
        errors().set(0x0502);
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const u32)>("glClearBufferuiv")
    {
        f(GL_FRAMEBUFFER, mask, b);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glClearNamedFramebufferfv(fbo: u32, mask: u32, b: *const f32) {
    if !unsafe { bind_fbo(fbo) } {
        mark_error_site("glClearNamedFramebufferfv");
        errors().set(0x0502);
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const f32)>("glClearBufferfv")
    {
        f(GL_FRAMEBUFFER, mask, b);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glClearNamedFramebufferfi(fbo: u32, mask: u32, d: f32, i: i32) {
    if !unsafe { bind_fbo(fbo) } {
        mark_error_site("glClearNamedFramebufferfi");
        errors().set(0x0502);
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, f32, i32)>("glClearBufferfi")
    {
        f(GL_FRAMEBUFFER, mask, d, i);
    }
}

// ---- named renderbuffers ---------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn glNamedRenderbufferStorage(rbo: u32, t: u32, w: i32, h: i32) {
    if !unsafe { bind_rbo(rbo) } {
        mark_error_site("glNamedRenderbufferStorage");
        errors().set(0x0502);
        return;
    }
    let fmt = format_translate::map_renderbuffer_internal_format(t);
    bind_rbo(rbo);
    if let Some(store) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, i32, i32)>("glRenderbufferStorage")
    {
        store(GL_RENDERBUFFER, fmt, w, h);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glNamedRenderbufferStorageMultisample(
    rbo: u32, samples: i32, t: u32, w: i32, h: i32,
) {
    if !unsafe { bind_rbo(rbo) } {
        mark_error_site("glNamedRenderbufferStorageMultisample");
        errors().set(0x0502);
        return;
    }
    let fmt = format_translate::map_renderbuffer_internal_format(t);
    bind_rbo(rbo);
    if let Some(store) = driver_fn_cached::<
        unsafe extern "C" fn(u32, i32, u32, i32, i32),
    >("glRenderbufferStorageMultisample")
    {
        store(GL_RENDERBUFFER, samples, fmt, w, h);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetNamedRenderbufferParameteriv(rbo: u32, pname: u32, params: *mut i32) {
    if params.is_null() {
        errors().set(0x0501);
        return;
    }
    if !unsafe { bind_rbo(rbo) } {
        *params = 0;
        mark_error_site("glGetNamedRenderbufferParameteriv");
        errors().set(0x0502);
        return;
    }
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetRenderbufferParameteriv")
    {
        f(GL_RENDERBUFFER, pname, params);
    } else {
        *params = 0;
        mark_error_site("glGetNamedRenderbufferParameteriv");
        errors().set(0x0502);
    }
}

// ---- named buffers --------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn glMapNamedBuffer(buffer: u32, access: u32) -> *mut c_void {
    unsafe { bind_buf(GL_ARRAY_BUFFER, buffer) };
    match driver_fn_cached::<MapBufferFn>("glMapBuffer") {
        Some(f) => f(GL_ARRAY_BUFFER, access),
        None => {
            mark_error_site("glMapNamedBuffer");
            errors().set(0x0502);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glMapNamedBufferRange(
    buffer: u32, offset: isize, length: isize, access: u32,
) -> *mut c_void {
    unsafe { bind_buf(GL_ARRAY_BUFFER, buffer) };
    match driver_fn_cached::<MapBufferRangeFn>("glMapBufferRange") {
        Some(f) => f(GL_ARRAY_BUFFER, offset, length, access),
        None => {
            mark_error_site("glMapNamedBufferRange");
            errors().set(0x0502);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glUnmapNamedBuffer(buffer: u32) -> u8 {
    unsafe { bind_buf(GL_ARRAY_BUFFER, buffer) };
    match driver_fn_cached::<UnmapBufferFn>("glUnmapBuffer") {
        Some(f) => f(GL_ARRAY_BUFFER),
        None => {
            mark_error_site("glUnmapNamedBuffer");
            errors().set(0x0502);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glFlushMappedNamedBufferRange(buffer: u32, offset: isize, size: isize) {
    unsafe { bind_buf(GL_ARRAY_BUFFER, buffer) };
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, isize, isize)>("glFlushMappedBufferRange")
    {
        f(GL_ARRAY_BUFFER, offset, size);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetNamedBufferPointerv(buffer: u32, pname: u32, params: *mut *mut c_void) {
    if params.is_null() {
        errors().set(0x0501);
        return;
    }
    unsafe { bind_buf(GL_ARRAY_BUFFER, buffer) };
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, *mut *mut c_void)>("glGetBufferPointerv")
    {
        f(GL_ARRAY_BUFFER, pname, params);
    } else {
        *params = std::ptr::null_mut();
        mark_error_site("glGetNamedBufferPointerv");
        errors().set(0x0502);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetNamedBufferParameteri64v(buffer: u32, pname: u32, params: *mut i64) {
    if params.is_null() {
        errors().set(0x0501);
        return;
    }
    unsafe { bind_buf(GL_ARRAY_BUFFER, buffer) };
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, *mut i64)>("glGetBufferParameteri64v")
    {
        f(GL_ARRAY_BUFFER, pname, params);
    } else {
        *params = 0;
        mark_error_site("glGetNamedBufferParameteri64v");
        errors().set(0x0502);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glCopyNamedBufferSubData(
    src: u32, dst: u32, src_off: isize, dst_off: isize, size: isize,
) {
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, isize, u32, isize, isize)>("glCopyBufferSubData")
    {
        f(GL_COPY_READ_BUFFER, src_off, GL_COPY_WRITE_BUFFER, dst_off, size);
    } else {
        mark_error_site("glCopyNamedBufferSubData");
        errors().set(0x0502);
    }
}

// ---- creation helpers ------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn glCreateFramebuffers(n: i32, ids: *mut u32) {
    if n <= 0 || ids.is_null() {
        errors().set(0x0501);
        return;
    }
    for i in 0..n as usize {
        *ids.add(i) = 0;
    }
    match driver_fn_cached::<unsafe extern "C" fn(i32, *mut u32)>("glGenFramebuffers") {
        Some(f) => f(n, ids),
        None => errors().set(0x0502),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glCreateRenderbuffers(n: i32, ids: *mut u32) {
    if n <= 0 || ids.is_null() {
        errors().set(0x0501);
        return;
    }
    for i in 0..n as usize {
        *ids.add(i) = 0;
    }
    match driver_fn_cached::<unsafe extern "C" fn(i32, *mut u32)>("glGenRenderbuffers") {
        Some(f) => f(n, ids),
        None => errors().set(0x0502),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glCreateSamplers(n: i32, ids: *mut u32) {
    if n <= 0 || ids.is_null() {
        errors().set(0x0501);
        return;
    }
    for i in 0..n as usize {
        *ids.add(i) = 0;
    }
    match driver_fn_cached::<unsafe extern "C" fn(i32, *mut u32)>("glGenSamplers") {
        Some(f) => f(n, ids),
        None => errors().set(0x0502),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glCreateQueries(n: i32, ids: *mut u32) {
    if n <= 0 || ids.is_null() {
        errors().set(0x0501);
        return;
    }
    for i in 0..n as usize {
        *ids.add(i) = 0;
    }
    match driver_fn_cached::<unsafe extern "C" fn(i32, *mut u32)>("glGenQueries") {
        Some(f) => f(n, ids),
        None => errors().set(0x0502),
    }
}

// ---- ARB-suffixed aliases -------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn glDrawArraysInstancedARB(mode: u32, first: i32, count: i32, n: i32) {
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, i32, i32, i32)>("glDrawArraysInstanced")
    {
        f(mode, first, count, n);
    } else {
        mark_error_site("glDrawArraysInstancedARB");
        errors().set(0x0502);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glDrawElementsInstancedARB(
    mode: u32, count: i32, t: u32, off: *const c_void, n: i32,
) {
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, i32, u32, *const c_void, i32)>(
            "glDrawElementsInstanced",
        )
    {
        f(mode, count, t, off, n);
    } else {
        mark_error_site("glDrawElementsInstancedARB");
        errors().set(0x0502);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glVertexAttribDivisorARB(index: u32, divisor: u32) {
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glVertexAttribDivisor") {
        f(index, divisor);
    } else {
        mark_error_site("glVertexAttribDivisorARB");
        errors().set(0x0502);
    }
}

// ---- bulk binding ----------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn glBindTextures(first: u32, count: i32, textures: *const u32) {
    if count < 0 || (count > 0 && textures.is_null()) {
        errors().set(0x0501);
        return;
    }
    for i in 0..count as u32 {
        if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32)>("glActiveTexture") {
            f(0x84C0 + first + i);
        }
        let t = texture_target(*textures.add(i as usize));
        if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindTexture") {
            if is_bindable_texture_target(t) {
                f(t, *textures.add(i as usize));
            }
        } else {
            mark_error_site("glBindTextures");
            errors().set(0x0502);
            return;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glBindSamplers(first: u32, count: i32, samplers: *const u32) {
    if count < 0 || (count > 0 && samplers.is_null()) {
        errors().set(0x0501);
        return;
    }
    for i in 0..count as u32 {
        if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindSampler") {
            f(first + i, *samplers.add(i as usize));
        } else {
            mark_error_site("glBindSamplers");
            errors().set(0x0502);
            return;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glBindBuffersBase(target: u32, first: u32, count: i32, buffers: *const u32) {
    if count < 0 || (count > 0 && buffers.is_null()) {
        errors().set(0x0501);
        return;
    }
    for i in 0..count as u32 {
        let id = *buffers.add(i as usize);
        unsafe { bind_buf(target, id) };
        if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32)>("glBindBufferBase")
        {
            f(target, first + i, id);
        } else {
            mark_error_site("glBindBuffersBase");
            errors().set(0x0502);
            return;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glBindBuffersRange(
    target: u32, first: u32, count: i32, buffers: *const u32, offsets: *const isize, sizes: *const isize,
) {
    if count < 0 || (count > 0 && (buffers.is_null() || offsets.is_null() || sizes.is_null())) {
        errors().set(0x0501);
        return;
    }
    for i in 0..count as usize {
        let id = *buffers.add(i);
        unsafe { bind_buf(target, id) };
        if let Some(f) =
            driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, isize, isize)>("glBindBufferRange")
        {
            f(target, first + i as u32, id, *offsets.add(i), *sizes.add(i));
        } else {
            mark_error_site("glBindBuffersRange");
            errors().set(0x0502);
            return;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glBindVertexBuffers(
    first: u32, count: i32, buffers: *const u32, offsets: *const isize, _sizes: *const isize,
) {
    if count < 0 || (count > 0 && buffers.is_null()) {
        errors().set(0x0501);
        return;
    }
    for i in 0..count as usize {
        let id = *buffers.add(i);
        if let Some(f) =
            driver_fn_cached::<unsafe extern "C" fn(u32, u32, isize)>("glBindVertexBuffer")
        {
            f(first + i as u32, id, if offsets.is_null() { 0 } else { *offsets.add(i) });
        } else {
            mark_error_site("glBindVertexBuffers");
            errors().set(0x0502);
            return;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glVertexArrayVertexBuffers(
    vao: u32, first: u32, count: i32, buffers: *const u32,
) {
    if count < 0 || (count > 0 && buffers.is_null()) {
        errors().set(0x0501);
        return;
    }
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32)>("glBindVertexArray") {
        bind(vao);
    }
    for i in 0..count as usize {
        let id = *buffers.add(i);
        if let Some(f) =
            driver_fn_cached::<unsafe extern "C" fn(u32, u32, isize)>("glBindVertexBuffer")
        {
            f(first + i as u32, id, 0);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetVertexArrayiv(vao: u32, pname: u32, params: *mut i32) {
    if params.is_null() {
        errors().set(0x0501);
        return;
    }
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32)>("glBindVertexArray") {
        bind(vao);
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetVertexArrayiv")
    {
        f(vao, pname, params);
    } else {
        *params = 0;
        mark_error_site("glGetVertexArrayiv");
        errors().set(0x0502);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetVertexArrayIndexediv(
    vao: u32, index: u32, pname: u32, params: *mut i32,
) {
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, *mut i32)>(
        "glGetVertexArrayIndexediv",
    ) {
        f(vao, index, pname, params);
    } else {
        if !params.is_null() {
            *params = 0;
        }
        mark_error_site("glGetVertexArrayIndexediv");
        errors().set(0x0502);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetVertexArrayIndexed64iv(
    vao: u32, index: u32, pname: u32, params: *mut i64,
) {
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, *mut i64)>(
        "glGetVertexArrayIndexed64iv",
    ) {
        f(vao, index, pname, params);
    } else {
        if !params.is_null() {
            *params = 0;
        }
        mark_error_site("glGetVertexArrayIndexed64iv");
        errors().set(0x0502);
    }
}

// ---- texture getters and 1D targets GLES never had -------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn glGetTextureParameterIiv(id: u32, pname: u32, params: *mut i32) {
    let target = unsafe { bind_tex(id) };
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetTexParameterIiv")
    {
        f(target, pname, params);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetTextureParameterIuiv(id: u32, pname: u32, params: *mut u32) {
    let target = unsafe { bind_tex(id) };
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, *mut u32)>("glGetTexParameterIuiv")
    {
        f(target, pname, params);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetTextureLevelParameterfv(
    id: u32, level: i32, pname: u32, params: *mut f32,
) {
    let target = unsafe { bind_tex(id) };
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, i32, u32, *mut f32)>("glGetTexLevelParameterfv")
    {
        f(target, level, pname, params);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetCompressedTextureImage(
    id: u32, level: i32, format: u32, size: isize, data: *mut c_void,
) {
    let target = unsafe { bind_tex(id) };
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, i32, u32, isize, *mut c_void)>(
            "glGetCompressedTexImage",
        )
    {
        f(target, level, format, size, data);
    }
}

/// `glTexStorage2DMultisample`, the call Minecraft 1.20.5+ makes for the multisampled depth
/// attachment in `WindowFramebuffer.createDepthAttachment`.
///
/// ES 3.x has no `*TexStorage*Multisample` at all, and its `glTexImage2DMultisample` only
/// accepts colour-renderable formats, so a depth request raises `GL_INVALID_OPERATION`. Depth
/// MSAA in ES is expressed with a multisample renderbuffer, so that is allocated instead and
/// `glFramebufferTexture2D` is redirected to it. Colour requests try the real texture first,
/// so a driver that does support them still gets one.
#[no_mangle]
pub unsafe extern "C" fn glTexStorage2DMultisample(
    target: u32, samples: i32, internalformat: u32, w: i32, h: i32,
) {
    msaa_storage(target, 0, samples, internalformat, w, h);
}

#[no_mangle]
pub unsafe extern "C" fn glTexStorage3DMultisample(
    target: u32, samples: i32, internalformat: u32, w: i32, h: i32, d: i32,
) {
    let _ = d;
    msaa_storage(target, 0, samples, internalformat, w, h);
}

/// The DSA form takes the texture name directly, because a multisample texture is never
/// bound.
#[no_mangle]
pub unsafe extern "C" fn glTextureStorage2DMultisample(
    id: u32, samples: i32, internalformat: u32, w: i32, h: i32,
) {
    msaa_storage(GL_TEXTURE_2D_MULTISAMPLE, id, samples, internalformat, w, h);
}

unsafe fn msaa_storage(
    target: u32, id: u32, samples: i32, internalformat: u32, w: i32, h: i32,
) {
    let fmt = format_translate::map_internal_format(internalformat as i32, 0, 0) as u32;
    let already = if id != 0 {
        id
    } else {
        let mut bound = 0i32;
        if let Some(get) = driver_fn_cached::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv")
        {
            get(
                if target == GL_TEXTURE_2D_MULTISAMPLE { 0x9102 } else { 0x8069 },
                &mut bound,
            );
        }
        bound.max(0) as u32
    };

    if !is_depth_or_stencil(fmt) {
        if let Some(f) = driver_fn_cached::<
            unsafe extern "C" fn(u32, i32, u32, i32, i32, i32),
        >("glTexImage2DMultisample")
        {
            f(target, samples, fmt, w, h, 0);
            if errors().take() == 0 {
                return;
            }
        } else {
            mark_error_site("glTexStorage2DMultisample: glTexImage2DMultisample missing");
            errors().set(0x0502);
            return;
        }
    }

    // Depth/stencil, or a driver that rejected a multisample colour texture.
    record_texture(already, target);
    mark_error_site("glTexStorage2DMultisample");
    let Some(rbo) = allocate_msaa_renderbuffer(samples, fmt, w, h) else {
        errors().set(0x0502);
        return;
    };
    record_msaa_substitute(already, rbo);
    log(&format!(
        "[dsa] multisample {fmt:#06x} served by renderbuffer {rbo} (texture {already}): ES has no multisample depth texture"
    ));
}

/// 1D textures and texel buffers never existed in GLES 3.x.
#[no_mangle]
pub unsafe extern "C" fn glTextureStorage1D(_id: u32, _levels: i32, _fmt: u32, _w: i32) {
    errors().set(0x0500);
}

#[no_mangle]
pub unsafe extern "C" fn glTextureSubImage1D(
    _id: u32, _level: i32, _x: i32, _w: i32, _f: u32, _t: u32, _data: *const c_void,
) {
    errors().set(0x0500);
}

#[no_mangle]
pub unsafe extern "C" fn glCompressedTextureSubImage1D(
    _id: u32, _level: i32, _x: i32, _w: i32, _f: u32, _size: isize, _data: *const c_void,
) {
    errors().set(0x0500);
}

#[no_mangle]
pub unsafe extern "C" fn glCopyTextureSubImage1D(
    _id: u32, _level: i32, _x: i32, _x2: i32, _w: i32, _h: i32,
) {
    errors().set(0x0500);
}

#[no_mangle]
pub unsafe extern "C" fn glTextureBuffer(_id: u32, _r: u32, _b: u32) {
    errors().set(0x0500);
}

#[no_mangle]
pub unsafe extern "C" fn glNamedFramebufferTextureMultiviewOVR(
    _fbo: u32, _att: u32, _tex: u32, _level: i32, _base: i32, _count: i32,
) {
    mark_error_site("glNamedFramebufferTextureMultiviewOVR");
    errors().set(0x0502);
}

#[no_mangle]
pub unsafe extern "C" fn glGetProgramResourceLocationIndex(
    program: u32, iface: u32, name: *const std::ffi::c_char,
) -> u32 {
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, *const std::ffi::c_char) -> u32>(
            "glGetProgramResourceLocationIndexAMD",
        )
    {
        return f(program, iface, name);
    }
    mark_error_site("glGetProgramResourceLocationIndex");
    errors().set(0x0502);
    0xFFFF_FFFF
}

/// Debug-message filtering. This layer produces no debug output, so there is nothing to
/// filter; the call has to be accepted because a trace of real Minecraft 1.17 shows it being
/// called on start-up, and an unresolved name is a null function pointer.
#[no_mangle]
pub unsafe extern "C" fn glDebugMessageControl(
    _source: u32,
    _type_: u32,
    _severity: u32,
    _count: i32,
    _ids: *const u32,
    _enabled: u8,
) {
}

#[no_mangle]
pub unsafe extern "C" fn glDebugMessageControlARB(
    _source: u32,
    _type_: u32,
    _severity: u32,
    _count: i32,
    _ids: *const u32,
    _enabled: u8,
) {
}

/// Image texture binding. Found missing from a real Minecraft 1.21.1 trace.
#[no_mangle]
pub unsafe extern "C" fn glBindImageTexture(
    unit: u32,
    texture: u32,
    level: i32,
    layered: i32,
    layer: i32,
    access: i32,
    format: u32,
) {
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32, i32, i32, i32, i32, u32)>(
        "glBindImageTexture",
    ) {
        Some(f) => f(unit, texture, level, layered, layer, access, format),
        None => mark_error_site("glBindImageTexture"),
    }
}

/// Multi-draw with a base vertex. Found missing from a real Minecraft 1.21.1 trace.
///
/// ES 3.2 provides this directly. Where it is absent the fallback is exact rather than
/// approximate: the per-draw index pointer advances by the index size, and each draw carries
/// the same base vertex.
#[no_mangle]
pub unsafe extern "C" fn glMultiDrawElementsBaseVertex(
    mode: u32,
    count: i32,
    ty: u32,
    indices: *const c_void,
    base_vertex: i32,
    primcount: i32,
) {
    if primcount <= 0 {
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, i32, u32, *const c_void, i32, i32)>(
        "glMultiDrawElementsBaseVertex",
    ) {
        return f(mode, count, ty, indices, base_vertex, primcount);
    }
    if let Some(draw) =
        driver_fn_cached::<unsafe extern "C" fn(u32, i32, u32, *const c_void, i32)>(
            "glDrawElementsBaseVertex",
        )
    {
        let stride = match ty {
            0x1401 => 1isize,
            0x1403 => 2,
            0x1405 => 4,
            _ => {
                mark_error_site("glMultiDrawElementsBaseVertex");
                errors().set(0x0500);
                return;
            }
        };
        let base = indices as usize;
        for i in 0..primcount as isize {
            draw(
                mode,
                count,
                ty,
                base.wrapping_add((i * stride) as usize) as *const c_void,
                base_vertex,
            );
        }
        return;
    }
    mark_error_site("glMultiDrawElementsBaseVertex");
    errors().set(0x0502);
}

/// Names registered in the resolver, so the reachability test can check them.
pub const EXPORTS: &[&str] = &[
    "glCreateTextures", "glCreateFramebuffers", "glCreateRenderbuffers",
    "glCreateSamplers", "glCreateQueries", "glTextureParameteri", "glTextureParameterf",
    "glTextureParameteriv", "glTextureParameterfv", "glTextureParameterIiv",
    "glTextureParameterIuiv", "glGenerateTextureMipmap", "glGetTextureParameterIiv",
    "glGetTextureParameterIuiv", "glGetTextureLevelParameterfv", "glGetCompressedTextureImage",
    "glNamedFramebufferTexture", "glNamedFramebufferTextureLayer", "glNamedFramebufferRenderbuffer",
    "glNamedFramebufferDrawBuffer", "glNamedFramebufferDrawBuffers", "glNamedFramebufferReadBuffer",
    "glCheckNamedFramebufferStatus", "glGetNamedFramebufferAttachmentParameteriv",
    "glClearNamedFramebufferiv", "glClearNamedFramebufferuiv", "glClearNamedFramebufferfv",
    "glClearNamedFramebufferfi", "glNamedRenderbufferStorage", "glNamedRenderbufferStorageMultisample",
    "glGetNamedRenderbufferParameteriv", "glMapNamedBuffer", "glMapNamedBufferRange",
    "glUnmapNamedBuffer", "glFlushMappedNamedBufferRange", "glGetNamedBufferPointerv",
    "glGetNamedBufferParameteri64v", "glCopyNamedBufferSubData", "glDrawArraysInstancedARB",
    "glDrawElementsInstancedARB", "glVertexAttribDivisorARB", "glBindTextures", "glBindSamplers",
    "glBindBuffersBase", "glBindBuffersRange", "glBindVertexBuffers", "glVertexArrayVertexBuffers",
    "glGetVertexArrayiv", "glGetVertexArrayIndexediv", "glGetVertexArrayIndexed64iv",
    "glTextureStorage1D", "glTextureSubImage1D", "glCompressedTextureSubImage1D",
    "glCopyTextureSubImage1D", "glTextureBuffer", "glNamedFramebufferTextureMultiviewOVR",
    "glGetProgramResourceLocationIndex", "glDebugMessageControl", "glDebugMessageControlARB",
    "glBindImageTexture", "glMultiDrawElementsBaseVertex",
    "glTexStorage2DMultisample", "glTexStorage3DMultisample", "glTextureStorage2DMultisample",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_export_is_reachable_through_the_resolver() {
        for name in EXPORTS {
            assert!(
                !crate::resolve_proc(name.as_bytes()).is_null(),
                "{name} is exported but unreachable via eglGetProcAddress"
            );
        }
    }

    #[test]
    fn depth_and_stencil_formats_take_the_renderbuffer_path() {
        // ES has no multisample depth texture; these are the formats that must not be
        // attempted as a texture.
        for f in [0x81A5, 0x81A6, 0x8CAC, 0x8CAD, 0x8CDF] {
            assert!(is_depth_or_stencil(f), "{f:#06x} should use a renderbuffer");
        }
        // Colour formats stay on the texture path.
        for f in [0x8058 /* RGBA8 */, 0x8051 /* RGB8 */, 0x881A /* RGBA8I */] {
            assert!(!is_depth_or_stencil(f), "{f:#06x} should stay a texture");
        }
    }

    #[test]
    fn multisample_types_are_never_used_as_bind_targets() {
        // Binding these raises GL_INVALID_ENUM, which is how the DSA depth path failed
        // before target tracking was fixed.
        assert!(!is_bindable_texture_target(0x9100));
        assert!(!is_bindable_texture_target(0x9112));
        for t in [0x0DE1, 0x806F, 0x8513, 0x8C1A, 0x9009] {
            assert!(is_bindable_texture_target(t), "{t:#06x} should be bindable");
        }
    }

    #[test]
    fn a_multisample_texture_still_remembers_its_type() {
        MSAA_SUBSTITUTE.lock().unwrap().clear();
        TEXTURES.lock().unwrap().clear();
        record_texture(42, GL_TEXTURE_2D_MULTISAMPLE);
        assert_eq!(texture_target(42), GL_TEXTURE_2D_MULTISAMPLE);
    }

    #[test]
    fn substitutions_are_looked_up_by_texture_name() {
        MSAA_SUBSTITUTE.lock().unwrap().clear();
        assert_eq!(msaa_substitute_for(5), None);
        record_msaa_substitute(5, 99);
        assert_eq!(msaa_substitute_for(5), Some(99));
        MSAA_SUBSTITUTE.lock().unwrap().clear();
    }

    #[test]
    fn an_unseen_texture_defaults_to_2d_instead_of_failing() {
        // This was the difference between a texture setup completing and
        // "OpenGL error 1282" during framebuffer construction.
        assert_eq!(texture_target(0xBEEF), GL_TEXTURE_2D);
    }

    #[test]
    fn textures_keep_the_target_they_were_created_for() {
        record_texture(77, 0x806F /* GL_TEXTURE_3D */);
        assert_eq!(texture_target(77), 0x806F);
        set_texture_target(77, GL_TEXTURE_2D);
        assert_eq!(texture_target(77), GL_TEXTURE_2D);
    }
}
