//! OpenGL 4.5 entry points backed by GLES calls or explicit compatibility fallbacks.

use std::ffi::c_void;
use std::ptr;

const INVALID_ENUM: u32 = 0x0500;
const INVALID_VALUE: u32 = 0x0501;
const INVALID_OPERATION: u32 = 0x0502;
const NO_ERROR: u32 = 0;
const TEXTURE_2D: u32 = 0x0DE1;
const FRAMEBUFFER: u32 = 0x8D40;
const READ_FRAMEBUFFER: u32 = 0x8CA8;
const DRAW_FRAMEBUFFER: u32 = 0x8CA9;
const COLOR_ATTACHMENT0: u32 = 0x8CE0;
const COPY_WRITE_BUFFER: u32 = 0x8F37;
const QUERY_BUFFER: u32 = 0x9192;
static CLIP_ORIGIN: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0x8CA1);
static CLIP_DEPTH: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0x935E);

unsafe fn f<T: Copy>(name: &'static str) -> Option<T> {
    crate::driver_fn_cached::<T>(name)
}

fn fail(name: &'static str) {
    static WARNED: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    if !warned.contains(&name) {
        warned.push(name);
        crate::log(&format!("[GLCompat] {name} has no GLES equivalent"));
    }
    crate::errors().set(INVALID_OPERATION);
}

fn binding_pname(target: u32) -> Option<u32> {
    Some(match target {
        0x0DE0 => 0x8068,
        0x0DE1 => 0x8069,
        0x806F => 0x806A,
        0x8513 => 0x8514,
        0x8C1A => 0x8C1D,
        0x9009 => 0x900A,
        0x8C2A => 0x8C2C,
        0x9100 => 0x9104,
        0x9102 => 0x9105,
        _ => return None,
    })
}

struct TextureBinding {
    target: u32,
    previous: i32,
}

impl Drop for TextureBinding {
    fn drop(&mut self) {
        if let Some(bind) = unsafe { f::<unsafe extern "C" fn(u32, u32)>("glBindTexture") } {
            unsafe { bind(self.target, self.previous as u32) };
        }
    }
}

unsafe fn bind_texture(texture: u32) -> Option<TextureBinding> {
    let target = crate::named_objects::texture_target(texture);
    let pname = binding_pname(target)?;
    let get = f::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv")?;
    let bind = f::<unsafe extern "C" fn(u32, u32)>("glBindTexture")?;
    let mut previous = 0;
    get(pname, &mut previous);
    bind(target, texture);
    Some(TextureBinding { target, previous })
}

struct BufferBinding {
    target: u32,
    previous: i32,
}

impl Drop for BufferBinding {
    fn drop(&mut self) {
        if let Some(bind) = unsafe { f::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") } {
            unsafe { bind(self.target, self.previous as u32) };
        }
    }
}

unsafe fn bind_buffer(target: u32) -> Option<BufferBinding> {
    let get = f::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv")?;
    let bind = f::<unsafe extern "C" fn(u32, u32)>("glBindBuffer")?;
    let mut previous = 0;
    let pname = if target == QUERY_BUFFER { 0x9193 } else { target };
    get(pname, &mut previous);
    Some(BufferBinding { target, previous })
}

#[no_mangle]
pub unsafe extern "C" fn glClipControl(origin: u32, depth: u32) {
    const LOWER_LEFT: u32 = 0x8CA1;
    const NEGATIVE_ONE_TO_ONE: u32 = 0x935E;
    if !matches!(origin, LOWER_LEFT | 0x8CA2)
        || !matches!(depth, NEGATIVE_ONE_TO_ONE | 0x935F)
    {
        crate::errors().set(INVALID_ENUM);
        return;
    }
    CLIP_ORIGIN.store(origin, std::sync::atomic::Ordering::Relaxed);
    CLIP_DEPTH.store(depth, std::sync::atomic::Ordering::Relaxed);
    if let Some(call) = f::<unsafe extern "C" fn(u32, u32)>("glClipControl") {
        call(origin, depth);
        return;
    }
    if crate::gles3::caps().has(b"GL_EXT_clip_control\0") {
        if let Some(call) = f::<unsafe extern "C" fn(u32, u32)>("glClipControlEXT") {
            call(origin, depth);
            return;
        }
    }
    if origin != LOWER_LEFT || depth != NEGATIVE_ONE_TO_ONE {
        static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            crate::log("[GLCompat] clip-control mode is recorded but not supported by this GLES driver");
        }
    }
}

pub(crate) fn clip_control_value(pname: u32) -> Option<i32> {
    match pname {
        0x935C => Some(CLIP_ORIGIN.load(std::sync::atomic::Ordering::Relaxed) as i32),
        0x935D => Some(CLIP_DEPTH.load(std::sync::atomic::Ordering::Relaxed) as i32),
        _ => None,
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTextureBarrier() {
    if let Some(call) = f::<unsafe extern "C" fn()>("glTextureBarrier") {
        call();
    } else if let Some(call) = f::<unsafe extern "C" fn()>("glTextureBarrierNV") {
        call();
    } else if let Some(call) = f::<unsafe extern "C" fn(u32)>("glMemoryBarrier") {
        call(0x00000008 | 0x00000400);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glCreateProgramPipelines(n: i32, pipelines: *mut u32) {
    if n < 0 || (n > 0 && pipelines.is_null()) {
        crate::errors().set(INVALID_VALUE);
        return;
    }
    if let Some(call) = f::<unsafe extern "C" fn(i32, *mut u32)>("glGenProgramPipelines") {
        call(n, pipelines);
    } else {
        fail("glCreateProgramPipelines");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glCreateTransformFeedbacks(n: i32, ids: *mut u32) {
    if n < 0 || (n > 0 && ids.is_null()) {
        crate::errors().set(INVALID_VALUE);
        return;
    }
    if let Some(call) = f::<unsafe extern "C" fn(i32, *mut u32)>("glGenTransformFeedbacks") {
        call(n, ids);
    } else {
        fail("glCreateTransformFeedbacks");
    }
}

unsafe fn with_transform_feedback<T: Copy>(
    object: u32,
    operation: impl FnOnce() -> T,
) -> Option<T> {
    let get = f::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv")?;
    let bind = f::<unsafe extern "C" fn(u32, u32)>("glBindTransformFeedback")?;
    let mut previous = 0;
    get(0x8E25, &mut previous);
    bind(0x8E22, object);
    let result = operation();
    bind(0x8E22, previous as u32);
    Some(result)
}

#[no_mangle]
pub unsafe extern "C" fn glTransformFeedbackBufferBase(xfb: u32, index: u32, buffer: u32) {
    if with_transform_feedback(xfb, || {
        if let Some(call) = f::<unsafe extern "C" fn(u32, u32, u32)>("glBindBufferBase") {
            call(0x8C8E, index, buffer);
        } else {
            fail("glTransformFeedbackBufferBase");
        }
    }).is_none() {
        fail("glTransformFeedbackBufferBase");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTransformFeedbackBufferRange(
    xfb: u32, index: u32, buffer: u32, offset: isize, size: isize,
) {
    if with_transform_feedback(xfb, || {
        if let Some(call) =
            f::<unsafe extern "C" fn(u32, u32, u32, isize, isize)>("glBindBufferRange")
        {
            call(0x8C8E, index, buffer, offset, size);
        } else {
            fail("glTransformFeedbackBufferRange");
        }
    }).is_none() {
        fail("glTransformFeedbackBufferRange");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetTransformFeedbackiv(xfb: u32, pname: u32, params: *mut i32) {
    if params.is_null() {
        crate::errors().set(INVALID_VALUE);
        return;
    }
    if with_transform_feedback(xfb, || {
        if let Some(call) =
            f::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetTransformFeedbackiv")
        {
            call(0x8E22, pname, params);
        } else {
            fail("glGetTransformFeedbackiv");
        }
    }).is_none() {
        fail("glGetTransformFeedbackiv");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetTransformFeedbacki_v(
    xfb: u32, pname: u32, index: u32, params: *mut i32,
) {
    if params.is_null() {
        crate::errors().set(INVALID_VALUE);
        return;
    }
    if with_transform_feedback(xfb, || {
        if let Some(call) = f::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetIntegeri_v") {
            call(pname, index, params);
        } else {
            fail("glGetTransformFeedbacki_v");
        }
    }).is_none() {
        fail("glGetTransformFeedbacki_v");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetTransformFeedbacki64_v(
    xfb: u32, pname: u32, index: u32, params: *mut i64,
) {
    if params.is_null() {
        crate::errors().set(INVALID_VALUE);
        return;
    }
    if with_transform_feedback(xfb, || {
        if let Some(call) = f::<unsafe extern "C" fn(u32, u32, *mut i64)>("glGetInteger64i_v") {
            call(pname, index, params);
        } else {
            fail("glGetTransformFeedbacki64_v");
        }
    }).is_none() {
        fail("glGetTransformFeedbacki64_v");
    }
}

fn buffer_pattern(internalformat: u32, format: u32, ty: u32, data: *const c_void) -> Option<Vec<u8>> {
    let components = match format {
        0x1903 | 0x8D94 => 1,
        0x8227 | 0x8228 => 2,
        0x1907 | 0x8D98 => 3,
        0x1908 | 0x8D99 => 4,
        _ => return None,
    };
    let width = match ty {
        0x1400 | 0x1401 => 1,
        0x1402 | 0x1403 | 0x140B => 2,
        0x1404 | 0x1405 | 0x1406 => 4,
        _ => return None,
    };
    let valid_internal = matches!(
        internalformat,
        0x8229 | 0x822A | 0x822B | 0x822C | 0x822D | 0x822E
            | 0x8231 | 0x8232 | 0x8233 | 0x8234 | 0x8235 | 0x8236
            | 0x8237 | 0x8238 | 0x8239 | 0x823A | 0x823B | 0x823C
            | 0x8058 | 0x8059 | 0x805A | 0x805B | 0x8814 | 0x8815 | 0x8816
            | 0x8D7C | 0x8D7D | 0x8D7E | 0x8D7F | 0x8D70 | 0x8D71
    );
    if !valid_internal {
        return None;
    }
    let size = components * width;
    let mut pattern = vec![0; size];
    if !data.is_null() {
        unsafe {
            ptr::copy_nonoverlapping(data.cast::<u8>(), pattern.as_mut_ptr(), size);
        }
    }
    Some(pattern)
}

unsafe fn clear_buffer(
    buffer: u32, offset: isize, size: isize, internalformat: u32, format: u32, ty: u32,
    data: *const c_void,
) {
    if offset < 0 || size < 0 {
        crate::errors().set(INVALID_VALUE);
        return;
    }
    let Some(pattern) = buffer_pattern(internalformat, format, ty, data) else {
        crate::errors().set(INVALID_ENUM);
        return;
    };
    let Some(_binding) = bind_buffer(COPY_WRITE_BUFFER) else {
        fail("glClearNamedBufferData");
        return;
    };
    let Some(bind) = f::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") else {
        fail("glClearNamedBufferData");
        return;
    };
    bind(COPY_WRITE_BUFFER, buffer);
    let Some(upload) =
        f::<unsafe extern "C" fn(u32, isize, isize, *const c_void)>("glBufferSubData")
    else {
        fail("glClearNamedBufferData");
        return;
    };
    let mut bytes = vec![0u8; size as usize];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = pattern[i % pattern.len()];
    }
    upload(COPY_WRITE_BUFFER, offset, size, bytes.as_ptr().cast());
}

#[no_mangle]
pub unsafe extern "C" fn glClearNamedBufferData(
    buffer: u32, internalformat: u32, format: u32, ty: u32, data: *const c_void,
) {
    let Some(_binding) = bind_buffer(COPY_WRITE_BUFFER) else {
        fail("glClearNamedBufferData");
        return;
    };
    let Some(bind) = f::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") else {
        fail("glClearNamedBufferData");
        return;
    };
    bind(COPY_WRITE_BUFFER, buffer);
    let size = if let Some(get) = f::<unsafe extern "C" fn(u32, u32, *mut i64)>("glGetBufferParameteri64v") {
        let mut size = 0i64;
        get(COPY_WRITE_BUFFER, 0x8764, &mut size);
        size
    } else {
        let mut size = 0i32;
        let Some(get) = f::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetBufferParameteriv") else {
            fail("glClearNamedBufferData");
            return;
        };
        get(COPY_WRITE_BUFFER, 0x8764, &mut size);
        size as i64
    };
    if size < 0 || size > isize::MAX as i64 {
        crate::errors().set(INVALID_OPERATION);
        return;
    }
    clear_buffer(buffer, 0, size as isize, internalformat, format, ty, data);
}

#[no_mangle]
pub unsafe extern "C" fn glClearNamedBufferSubData(
    buffer: u32, internalformat: u32, offset: isize, size: isize, format: u32, ty: u32,
    data: *const c_void,
) {
    clear_buffer(buffer, offset, size, internalformat, format, ty, data);
}

#[no_mangle]
pub unsafe extern "C" fn glCompressedTextureSubImage2D(
    texture: u32, level: i32, x: i32, y: i32, width: i32, height: i32, format: u32,
    image_size: i32, data: *const c_void,
) {
    let Some(binding) = bind_texture(texture) else { fail("glCompressedTextureSubImage2D"); return };
    if binding.target != TEXTURE_2D {
        crate::errors().set(INVALID_OPERATION);
        return;
    }
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, i32, *const c_void)>("glCompressedTexSubImage2D") {
        call(TEXTURE_2D, level, x, y, width, height, format, image_size, data);
    } else {
        fail("glCompressedTextureSubImage2D");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glCompressedTextureSubImage3D(
    texture: u32, level: i32, x: i32, y: i32, z: i32, width: i32, height: i32, depth: i32,
    format: u32, image_size: i32, data: *const c_void,
) {
    let Some(binding) = bind_texture(texture) else { fail("glCompressedTextureSubImage3D"); return };
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, i32, i32, u32, i32, *const c_void)>("glCompressedTexSubImage3D") {
        call(binding.target, level, x, y, z, width, height, depth, format, image_size, data);
    } else {
        fail("glCompressedTextureSubImage3D");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glCopyTextureSubImage2D(
    texture: u32, level: i32, xoffset: i32, yoffset: i32, x: i32, y: i32, width: i32,
    height: i32,
) {
    let Some(binding) = bind_texture(texture) else { fail("glCopyTextureSubImage2D"); return };
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, i32, i32)>("glCopyTexSubImage2D") {
        call(binding.target, level, xoffset, yoffset, x, y, width, height);
    } else {
        fail("glCopyTextureSubImage2D");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glCopyTextureSubImage3D(
    texture: u32, level: i32, xoffset: i32, yoffset: i32, zoffset: i32, x: i32, y: i32,
    width: i32, height: i32,
) {
    let Some(binding) = bind_texture(texture) else { fail("glCopyTextureSubImage3D"); return };
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, i32, i32, i32)>("glCopyTexSubImage3D") {
        call(binding.target, level, xoffset, yoffset, zoffset, x, y, width, height);
    } else {
        fail("glCopyTextureSubImage3D");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glNamedFramebufferParameteri(fbo: u32, pname: u32, param: i32) {
    if let Some(call) =
        f::<unsafe extern "C" fn(u32, u32, i32)>("glNamedFramebufferParameteri")
    {
        call(fbo, pname, param);
    } else {
        fail("glNamedFramebufferParameteri");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetNamedFramebufferParameteriv(
    fbo: u32, pname: u32, params: *mut i32,
) {
    if params.is_null() {
        crate::errors().set(INVALID_VALUE);
        return;
    }
    let Some(_scope) = crate::named_objects::scoped_fbo(fbo) else {
        fail("glGetNamedFramebufferParameteriv");
        return;
    };
    if let Some(call) = f::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetFramebufferParameteriv") {
        call(FRAMEBUFFER, pname, params);
    } else {
        fail("glGetNamedFramebufferParameteriv");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glInvalidateNamedFramebufferData(
    fbo: u32, count: i32, attachments: *const u32,
) {
    let Some(_scope) = crate::named_objects::scoped_fbo(fbo) else {
        fail("glInvalidateNamedFramebufferData");
        return;
    };
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, *const u32)>("glInvalidateFramebuffer") {
        call(FRAMEBUFFER, count, attachments);
    } else {
        fail("glInvalidateNamedFramebufferData");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glInvalidateNamedFramebufferSubData(
    fbo: u32, count: i32, attachments: *const u32, x: i32, y: i32, width: i32, height: i32,
) {
    let Some(_scope) = crate::named_objects::scoped_fbo(fbo) else {
        fail("glInvalidateNamedFramebufferSubData");
        return;
    };
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, *const u32, i32, i32, i32, i32)>("glInvalidateSubFramebuffer") {
        call(FRAMEBUFFER, count, attachments, x, y, width, height);
    } else {
        fail("glInvalidateNamedFramebufferSubData");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTextureBufferRange(
    texture: u32, internalformat: u32, buffer: u32, offset: isize, size: isize,
) {
    let Some(binding) = bind_texture(texture) else { fail("glTextureBufferRange"); return };
    if let Some(call) = f::<unsafe extern "C" fn(u32, u32, u32, isize, isize)>("glTexBufferRange") {
        call(binding.target, internalformat, buffer, offset, size);
    } else {
        fail("glTextureBufferRange");
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTextureStorage3DMultisample(
    texture: u32, samples: i32, internalformat: u32, width: i32, height: i32, depth: i32,
    fixed: u8,
) {
    let Some(binding) = bind_texture(texture) else { fail("glTextureStorage3DMultisample"); return };
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, u32, i32, i32, i32, u8)>("glTexStorage3DMultisample") {
        call(binding.target, samples, internalformat, width, height, depth, fixed);
    } else {
        fail("glTextureStorage3DMultisample");
    }
}

fn pixel_bytes(format: u32, ty: u32, width: i32, height: i32) -> Option<usize> {
    if width < 0 || height < 0 {
        return None;
    }
    let components = match format {
        0x1903 | 0x1904 | 0x1905 | 0x1906 | 0x8D94 | 0x8D95 | 0x8D96 => 1,
        0x8227 | 0x8D98 => 2,
        0x1907 | 0x8D97 => 3,
        0x1908 | 0x80E1 | 0x8D99 => 4,
        _ => return None,
    };
    let per = match ty {
        0x1401 | 0x1400 => 1,
        0x1403 | 0x1402 | 0x140B => 2,
        0x1405 | 0x1404 | 0x1406 => 4,
        _ => return None,
    };
    Some(width as usize * height as usize * components * per)
}

unsafe fn read_texture_subimage(
    texture: u32, level: i32, x: i32, y: i32, z: i32, width: i32, height: i32, depth: i32,
    format: u32, ty: u32, buf_size: i32, pixels: *mut c_void,
) {
    if buf_size < 0 || (buf_size > 0 && pixels.is_null()) {
        crate::errors().set(INVALID_VALUE);
        return;
    }
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, i32, i32, u32, u32, i32, *mut c_void)>("glGetTextureSubImage") {
        call(texture, level, x, y, z, width, height, depth, format, ty, buf_size, pixels);
        return;
    }
    if z != 0 || depth != 1 {
        fail("glGetTextureSubImage");
        return;
    }
    if let Some(required) = pixel_bytes(format, ty, width, height) {
        if required > buf_size as usize {
            crate::errors().set(INVALID_OPERATION);
            return;
        }
    }
    let Some(texture_binding) = bind_texture(texture) else { fail("glGetTextureSubImage"); return };
    if texture_binding.target != TEXTURE_2D {
        fail("glGetTextureSubImage");
        return;
    }
    let (Some(gen), Some(del), Some(attach), Some(read)) = (
        f::<unsafe extern "C" fn(i32, *mut u32)>("glGenFramebuffers"),
        f::<unsafe extern "C" fn(i32, *const u32)>("glDeleteFramebuffers"),
        f::<unsafe extern "C" fn(u32, u32, u32, u32, i32)>("glFramebufferTexture2D"),
        f::<unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut c_void)>("glReadPixels"),
    ) else {
        fail("glGetTextureSubImage");
        return;
    };
    let mut fbo = 0;
    gen(1, &mut fbo);
    let Some(scope) = crate::named_objects::scoped_fbo(fbo) else {
        del(1, &fbo);
        fail("glGetTextureSubImage");
        return;
    };
    attach(READ_FRAMEBUFFER, COLOR_ATTACHMENT0, TEXTURE_2D, texture, level);
    let status = f::<unsafe extern "C" fn(u32) -> u32>("glCheckFramebufferStatus")
        .map(|check| check(READ_FRAMEBUFFER))
        .unwrap_or(0);
    if status == 0x8CD5 {
        read(x, y, width, height, format, ty, pixels);
    } else {
        crate::errors().set(INVALID_OPERATION);
    }
    drop(scope);
    del(1, &fbo);
}

#[no_mangle]
pub unsafe extern "C" fn glGetTextureSubImage(
    texture: u32, level: i32, x: i32, y: i32, z: i32, width: i32, height: i32, depth: i32,
    format: u32, ty: u32, buf_size: i32, pixels: *mut c_void,
) {
    read_texture_subimage(texture, level, x, y, z, width, height, depth, format, ty, buf_size, pixels);
}

#[no_mangle]
pub unsafe extern "C" fn glGetCompressedTextureSubImage(
    texture: u32, level: i32, x: i32, y: i32, z: i32, width: i32, height: i32, depth: i32,
    buf_size: i32, pixels: *mut c_void,
) {
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, i32, i32, i32, *mut c_void)>("glGetCompressedTextureSubImage") {
        call(texture, level, x, y, z, width, height, depth, buf_size, pixels);
    } else {
        fail("glGetCompressedTextureSubImage");
    }
}

unsafe fn robust_pixels(
    x: i32, y: i32, width: i32, height: i32, format: u32, ty: u32, buf_size: i32, pixels: *mut c_void,
) {
    if buf_size < 0 || (buf_size > 0 && pixels.is_null()) {
        crate::errors().set(INVALID_VALUE);
        return;
    }
    if let Some(required) = pixel_bytes(format, ty, width, height) {
        if required > buf_size as usize {
            crate::errors().set(INVALID_OPERATION);
            return;
        }
    }
    let Some(read) = f::<unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, *mut c_void)>("glReadPixels") else {
        fail("glReadnPixels");
        return;
    };
    read(x, y, width, height, format, ty, pixels);
}

#[no_mangle]
pub unsafe extern "C" fn glReadnPixels(
    x: i32, y: i32, width: i32, height: i32, format: u32, ty: u32, buf_size: i32,
    pixels: *mut c_void,
) {
    if let Some(call) = f::<unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, i32, *mut c_void)>("glReadnPixelsKHR")
        .or_else(|| f::<unsafe extern "C" fn(i32, i32, i32, i32, u32, u32, i32, *mut c_void)>("glReadnPixelsEXT"))
    {
        call(x, y, width, height, format, ty, buf_size, pixels);
    } else {
        robust_pixels(x, y, width, height, format, ty, buf_size, pixels);
    }
}

macro_rules! uniform_getn {
    ($name:ident, $base:literal, $ty:ty) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(
            program: u32, location: i32, buf_size: i32, params: *mut $ty,
        ) {
            if buf_size < 0 || (buf_size > 0 && params.is_null()) {
                crate::errors().set(INVALID_VALUE);
                return;
            }
            let khr = concat!(stringify!($name), "KHR");
            let ext = concat!(stringify!($name), "EXT");
            let call = f::<unsafe extern "C" fn(u32, i32, i32, *mut $ty)>(khr)
                .or_else(|| f::<unsafe extern "C" fn(u32, i32, i32, *mut $ty)>(ext));
            if let Some(call) = call {
                call(program, location, buf_size, params);
            } else if buf_size < std::mem::size_of::<$ty>() as i32 {
                crate::errors().set(INVALID_OPERATION);
            } else if let Some(call) =
                f::<unsafe extern "C" fn(u32, i32, *mut $ty)>($base)
            {
                call(program, location, params);
            } else {
                fail(stringify!($name));
            }
        }
    };
}

uniform_getn!(glGetnUniformfv, "glGetUniformfv", f32);
uniform_getn!(glGetnUniformiv, "glGetUniformiv", i32);
uniform_getn!(glGetnUniformuiv, "glGetUniformuiv", u32);
uniform_getn!(glGetnUniformdv, "glGetUniformdv", f64);

#[no_mangle]
pub unsafe extern "C" fn glGetnTexImage(
    target: u32, level: i32, format: u32, ty: u32, buf_size: i32, pixels: *mut c_void,
) {
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, u32, u32, i32, *mut c_void)>("glGetnTexImageKHR")
        .or_else(|| f::<unsafe extern "C" fn(u32, i32, u32, u32, i32, *mut c_void)>("glGetnTexImageEXT"))
    {
        call(target, level, format, ty, buf_size, pixels);
        return;
    }
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, u32, u32, *mut c_void)>("glGetTexImage") {
        if buf_size < 0 || (buf_size > 0 && pixels.is_null()) {
            crate::errors().set(INVALID_VALUE);
        } else {
            call(target, level, format, ty, pixels);
        }
        return;
    }
    let pname = binding_pname(target);
    let Some(pname) = pname else { fail("glGetnTexImage"); return };
    let Some(get) = f::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv") else {
        fail("glGetnTexImage");
        return;
    };
    let mut texture = 0;
    get(pname, &mut texture);
    if texture == 0 {
        fail("glGetnTexImage");
        return;
    }
    let mut width = 0;
    let mut height = 0;
    crate::gl::v3_3::glGetTextureLevelParameteriv(texture as u32, level, 0x1000, &mut width);
    crate::gl::v3_3::glGetTextureLevelParameteriv(texture as u32, level, 0x1001, &mut height);
    read_texture_subimage(texture as u32, level, 0, 0, 0, width, height, 1, format, ty, buf_size, pixels);
}

#[no_mangle]
pub unsafe extern "C" fn glGetnCompressedTexImage(
    target: u32, level: i32, buf_size: i32, pixels: *mut c_void,
) {
    if let Some(call) = f::<unsafe extern "C" fn(u32, i32, i32, *mut c_void)>("glGetnCompressedTexImageKHR")
        .or_else(|| f::<unsafe extern "C" fn(u32, i32, i32, *mut c_void)>("glGetnCompressedTexImageEXT"))
    {
        call(target, level, buf_size, pixels);
    } else if let Some(call) = f::<unsafe extern "C" fn(u32, i32, *mut c_void)>("glGetCompressedTexImage") {
        if buf_size < 0 || (buf_size > 0 && pixels.is_null()) {
            crate::errors().set(INVALID_VALUE);
        } else {
            call(target, level, pixels);
        }
    } else {
        fail("glGetnCompressedTexImage");
    }
}

macro_rules! query_buffer_get {
    ($name:ident, $getter:path, $ty:ty) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(query: u32, buffer: u32, pname: u32, offset: isize) {
            if offset < 0 {
                crate::errors().set(INVALID_VALUE);
                return;
            }
            let Some(_binding) = bind_buffer(QUERY_BUFFER) else { fail(stringify!($name)); return };
            let Some(bind) = f::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") else {
                fail(stringify!($name)); return;
            };
            let mut value: $ty = 0 as $ty;
            $getter(query, pname, &mut value);
            bind(QUERY_BUFFER, buffer);
            if let Some(upload) = f::<unsafe extern "C" fn(u32, isize, isize, *const c_void)>("glBufferSubData") {
                upload(QUERY_BUFFER, offset, std::mem::size_of::<$ty>() as isize, (&value as *const $ty).cast());
            } else {
                fail(stringify!($name));
            }
        }
    };
}

query_buffer_get!(glGetQueryBufferObjectiv, crate::gl::v3_3::glGetQueryObjectiv, i32);
query_buffer_get!(glGetQueryBufferObjectuiv, crate::gl::v3_3::glGetQueryObjectuiv, u32);
query_buffer_get!(glGetQueryBufferObjecti64v, crate::gl::v3_3::glGetQueryObjecti64v, i64);
query_buffer_get!(glGetQueryBufferObjectui64v, crate::gl::v3_3::glGetQueryObjectui64v, u64);

pub const EXPORTS: &[&str] = &[
    "glClipControl", "glTextureBarrier", "glCreateProgramPipelines",
    "glCreateTransformFeedbacks", "glTransformFeedbackBufferBase",
    "glTransformFeedbackBufferRange", "glGetTransformFeedbackiv",
    "glGetTransformFeedbacki_v", "glGetTransformFeedbacki64_v",
    "glClearNamedBufferData", "glClearNamedBufferSubData",
    "glCompressedTextureSubImage2D",
    "glCompressedTextureSubImage3D", "glCopyTextureSubImage2D",
    "glCopyTextureSubImage3D", "glNamedFramebufferParameteri",
    "glGetNamedFramebufferParameteriv", "glInvalidateNamedFramebufferData",
    "glInvalidateNamedFramebufferSubData", "glTextureBufferRange",
    "glTextureStorage3DMultisample", "glGetTextureSubImage",
    "glGetCompressedTextureSubImage", "glReadnPixels", "glGetnUniformfv",
    "glGetnUniformiv", "glGetnUniformuiv", "glGetnUniformdv", "glGetnTexImage",
    "glGetnCompressedTexImage", "glGetQueryBufferObjectiv",
    "glGetQueryBufferObjectuiv", "glGetQueryBufferObjecti64v",
    "glGetQueryBufferObjectui64v",
];

pub fn resolve(name: &[u8]) -> *const c_void {
    macro_rules! resolve_entries {
        ($($name:literal => $function:ident),* $(,)?) => {
            match name {
                $( $name => $function as *const c_void, )*
                _ => ptr::null(),
            }
        };
    }
    resolve_entries!(
        b"glClipControl" => glClipControl,
        b"glTextureBarrier" => glTextureBarrier,
        b"glCreateProgramPipelines" => glCreateProgramPipelines,
        b"glCreateTransformFeedbacks" => glCreateTransformFeedbacks,
        b"glTransformFeedbackBufferBase" => glTransformFeedbackBufferBase,
        b"glTransformFeedbackBufferRange" => glTransformFeedbackBufferRange,
        b"glGetTransformFeedbackiv" => glGetTransformFeedbackiv,
        b"glGetTransformFeedbacki_v" => glGetTransformFeedbacki_v,
        b"glGetTransformFeedbacki64_v" => glGetTransformFeedbacki64_v,
        b"glClearNamedBufferData" => glClearNamedBufferData,
        b"glClearNamedBufferSubData" => glClearNamedBufferSubData,
        b"glCompressedTextureSubImage2D" => glCompressedTextureSubImage2D,
        b"glCompressedTextureSubImage3D" => glCompressedTextureSubImage3D,
        b"glCopyTextureSubImage2D" => glCopyTextureSubImage2D,
        b"glCopyTextureSubImage3D" => glCopyTextureSubImage3D,
        b"glNamedFramebufferParameteri" => glNamedFramebufferParameteri,
        b"glGetNamedFramebufferParameteriv" => glGetNamedFramebufferParameteriv,
        b"glInvalidateNamedFramebufferData" => glInvalidateNamedFramebufferData,
        b"glInvalidateNamedFramebufferSubData" => glInvalidateNamedFramebufferSubData,
        b"glTextureBufferRange" => glTextureBufferRange,
        b"glTextureStorage3DMultisample" => glTextureStorage3DMultisample,
        b"glGetTextureSubImage" => glGetTextureSubImage,
        b"glGetCompressedTextureSubImage" => glGetCompressedTextureSubImage,
        b"glReadnPixels" => glReadnPixels,
        b"glGetnUniformfv" => glGetnUniformfv,
        b"glGetnUniformiv" => glGetnUniformiv,
        b"glGetnUniformuiv" => glGetnUniformuiv,
        b"glGetnUniformdv" => glGetnUniformdv,
        b"glGetnTexImage" => glGetnTexImage,
        b"glGetnCompressedTexImage" => glGetnCompressedTexImage,
        b"glGetQueryBufferObjectiv" => glGetQueryBufferObjectiv,
        b"glGetQueryBufferObjectuiv" => glGetQueryBufferObjectuiv,
        b"glGetQueryBufferObjecti64v" => glGetQueryBufferObjecti64v,
        b"glGetQueryBufferObjectui64v" => glGetQueryBufferObjectui64v,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_size_covers_basic_color_formats() {
        assert_eq!(pixel_bytes(0x1908, 0x1401, 2, 3), Some(24));
        assert_eq!(pixel_bytes(0x1903, 0x1406, 2, 3), Some(24));
        assert_eq!(pixel_bytes(0xDEAD, 0x1401, 2, 3), None);
    }

    #[test]
    fn all_exports_resolve_to_this_module() {
        for name in EXPORTS {
            assert!(!resolve(name.as_bytes()).is_null(), "{name}");
        }
    }
}
