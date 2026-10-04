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

/// Binds a named texture to the target it belongs to. Returns the target.
unsafe fn bind_tex(id: u32) -> u32 {
    let target = texture_target(id);
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
    // without this the name is unusable under the target the game asked for.
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindTexture") {
        for i in 0..n as usize {
            let id = *ids.add(i);
            bind(target, id);
            record_texture(id, target);
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
        if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindTexture") {
            f(texture_target(*textures.add(i as usize)), *textures.add(i as usize));
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
    "glGetProgramResourceLocationIndex",
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
