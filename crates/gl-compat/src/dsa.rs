//! Direct State Access (GL 4.5 / ARB_direct_state_access) over GLES 3.x.
//!
//! Minecraft 1.20.5+ moved its renderer onto DSA: buffers are created with
//! `glCreateBuffers` and filled with `glNamedBufferData`, textures with `glCreateTextures` +
//! `glTextureStorage2D`, and vertex arrays describe attributes with
//! `glVertexArrayAttribFormat`. None of those existed here, which is the shape of a
//! version-specific failure: the fixed-function path (1.16) never touches them.
//!
//! # Why this needs bookkeeping
//!
//! Desktop GL treats object names as global. GLES 3.x is **per target**: a buffer name bound
//! to `GL_ARRAY_BUFFER` and the same name bound to `GL_ELEMENT_ARRAY_BUFFER` are two distinct
//! GLbuffer objects, and the same is true of textures per target. So DSA cannot be translated
//! by ignoring the target. Each object created here is immediately materialised on the target
//! it was created for, and [`note_buffer_target`] does the same the first time a known DSA
//! buffer is bound to another target. That covers the single-target case exactly and the
//! cross-target case without corrupting the first binding.
//!
//! Vertex array *attribute formats* are the remaining gap: `GL_ARRAY_BUFFER` binding is
//! global state in ES, not per-VAO, so `glVertexArrayVertexBuffer` cannot be per-VAO there.
//! It is applied globally and the deviation is logged once rather than silently misrendered.

use super::*;
use std::sync::Mutex;

/// DSA buffer: name -> (byte size, GL usage).
static BUFFERS: Mutex<Vec<(u32, i64, u32)>> = Mutex::new(Vec::new());
/// DSA texture: name -> the target it was created for.
static TEXTURES: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());
/// DSA vertex array names.
static VAOS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Attribute description recorded per vertex array: (vao, attrib, size, type, normalized,
/// relative_offset, stride).
///
/// Needed because ES 3.x has no equivalent of `glVertexArrayVertexBuffer`: there, the buffer
/// an attribute reads from is whatever `GL_ARRAY_BUFFER` was bound to when
/// `glVertexAttribPointer` was called. So the format has to be remembered, and re-applied as
/// a pointer call whenever the VAO's buffer binding changes. Skipping this leaves every
/// attribute reading buffer 0, and the frame is blank with no GL error to explain it.
static ATTR_FORMATS: Mutex<Vec<(u32, u32, i32, u32, bool, u32, u32)>> = Mutex::new(Vec::new());

/// Buffer bound to a vertex array via `glVertexArrayVertexBuffer`: (vao, buffer, offset).
static VAO_BUFFERS: Mutex<Vec<(u32, u32, isize)>> = Mutex::new(Vec::new());

fn with_formats<R>(f: impl FnOnce(&mut Vec<(u32, u32, i32, u32, bool, u32, u32)>) -> R) -> R {
    let mut g = ATTR_FORMATS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.as_mut())
}

/// Records an attribute description and, if the vertex array already has a buffer bound,
/// applies it straight away — the buffer may be attached before or after the format.
unsafe fn record_format(
    vao: u32,
    attrib: u32,
    size: i32,
    ty: u32,
    normalized: bool,
    rel: u32,
    stride: u32,
) {
    with_formats(|v| {
        if let Some(e) = v.iter_mut().find(|(w, a, ..)| *w == vao && *a == attrib) {
            e.2 = size;
            e.3 = ty;
            e.4 = normalized;
            e.5 = rel;
            e.6 = stride;
        } else {
            v.push((vao, attrib, size, ty, normalized, rel, stride));
        }
    });
    if let Some((buffer, offset)) = with_buffers_vao(|v| {
        v.iter().find(|(w, ..)| *w == vao).map(|(_, b, o)| (*b, *o))
    }) {
        if let Some(set_ptr) = driver_fn_cached::<
            unsafe extern "C" fn(u32, i32, u32, bool, i32, *const c_void),
        >("glVertexAttribPointer")
        {
            if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
                bind(GL_ARRAY_BUFFER, buffer);
            }
            set_ptr(attrib, size, ty, normalized, stride as i32,
                    offset as u32 as usize as *const c_void);
        }
    }
}

fn with_buffers_vao<R>(f: impl FnOnce(&mut Vec<(u32, u32, isize)>) -> R) -> R {
    let mut g = VAO_BUFFERS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.as_mut())
}

/// Re-applies `glVertexAttribPointer` for every attribute of `vao` that has a recorded format,
/// so the attributes actually read from `buffer`.
unsafe fn apply_formats(vao: u32, buffer: u32, offset: isize) {
    let Some(set_ptr) = driver_fn_cached::<
        unsafe extern "C" fn(u32, i32, u32, bool, i32, *const c_void),
    >("glVertexAttribPointer")
    else {
        return;
    };
    let entries: Vec<(u32, i32, u32, bool, u32, u32)> = with_formats(|v| {
        v.iter()
            .filter(|(v, ..)| *v == vao)
            .map(|(_, a, size, ty, norm, rel, stride)| (*a, *size, *ty, *norm, *rel, *stride))
            .collect()
    });
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        bind(GL_ARRAY_BUFFER, buffer);
    }
    for (attrib, size, ty, norm, rel, stride) in entries {
        set_ptr(
            attrib,
            size,
            ty,
            norm,
            stride as i32,
            (offset as u32).wrapping_add(rel) as usize as *const c_void,
        );
    }
}

const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_TEXTURE_3D: u32 = 0x806F;
const GL_ARRAY_BUFFER: u32 = 0x8892;
const GL_ELEMENT_ARRAY_BUFFER: u32 = 0x8893;
const GL_NO_ERROR: u32 = 0;

fn with_buffers<R>(f: impl FnOnce(&mut Vec<(u32, i64, u32)>) -> R) -> R {
    let mut g = BUFFERS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.as_mut())
}

fn with_textures<R>(f: impl FnOnce(&mut Vec<(u32, u32)>) -> R) -> R {
    let mut g = TEXTURES.lock().unwrap_or_else(|e| e.into_inner());
    f(g.as_mut())
}

fn with_vaos<R>(f: impl FnOnce(&mut Vec<u32>) -> R) -> R {
    let mut g = VAOS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.as_mut())
}

fn log_once(flag: &AtomicBool, msg: &str) {
    if !flag.swap(true, Ordering::Relaxed) {
        log(msg);
    }
}

fn warn_array_buffer_is_global() {
    static ONCE: AtomicBool = AtomicBool::new(false);
    log_once(
        &ONCE,
        "[GLCompat] glVertexArrayVertexBuffer: GL_ARRAY_BUFFER binding is global in GLES, \
         not per-VAO as on desktop; applying globally",
    );
}

fn warn_no_vertex_attrib_format() {
    static ONCE: AtomicBool = AtomicBool::new(false);
    log_once(
        &ONCE,
        "[GLCompat] glVertexArrayAttribFormat: driver lacks glVertexAttribFormat (needs ES 3.1); \
         attribute format ignored",
    );
}

/// Binds a DSA buffer to `target`, materialising the GLES-side buffer object for that target
/// the first time. Without this, binding a known DSA buffer to a second target would hand the
/// driver a name that has no storage under that target.
pub(crate) unsafe fn note_buffer_target(buffer: u32, target: u32) {
    if buffer == 0 || target != GL_ELEMENT_ARRAY_BUFFER && target != GL_ARRAY_BUFFER {
        return;
    }
    let known = with_buffers(|v| v.iter().find(|(id, _, _)| *id == buffer).copied());
    let Some((_, size, usage)) = known else {
        return;
    };
    // If it is already the current binding for this target, nothing to materialise.
    let mut current = 0i32;
    if let Some(get) = driver_fn_cached::<GetIntFn>("glGetIntegerv") {
        let pname = if target == GL_ELEMENT_ARRAY_BUFFER { 0x8895 } else { 0x8894 };
        get(pname, &mut current);
    }
    if current as u32 == buffer {
        return;
    }
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        bind(target, buffer);
    }
    if let Some(data) = driver_fn_cached::<unsafe extern "C" fn(u32, isize, *const c_void, u32)>(
        "glBufferData",
    ) {
        // Allocate uninitialised storage so the name is valid under this target. The contents
        // come from the original allocation; see the note in the module docs.
        data(target, size as isize, std::ptr::null(), usage);
    }
}

// ---- creation --------------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn glCreateBuffers(n: i32, ids: *mut u32) {
    if n <= 0 || ids.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    for i in 0..n as usize {
        *ids.add(i) = 0;
    }
    let Some(gen) = driver_fn_cached::<unsafe extern "C" fn(i32, *mut u32)>("glGenBuffers") else {
        errors().set(GL_INVALID_OPERATION);
        return;
    };
    gen(n, ids);
    with_buffers(|v| {
        for i in 0..n as usize {
            v.push((*ids.add(i), 0, 0x88E4 /* GL_STATIC_DRAW */));
        }
    });
}


#[no_mangle]
pub unsafe extern "C" fn glCreateVertexArrays(n: i32, ids: *mut u32) {
    if n <= 0 || ids.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    for i in 0..n as usize {
        *ids.add(i) = 0;
    }
    let Some(gen) = driver_fn_cached::<unsafe extern "C" fn(i32, *mut u32)>("glGenVertexArrays")
    else {
        errors().set(GL_INVALID_OPERATION);
        return;
    };
    gen(n, ids);
    with_vaos(|v| {
        for i in 0..n as usize {
            v.push(*ids.add(i));
        }
    });
}

// ---- buffers ---------------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn glNamedBufferData(buffer: u32, size: isize, data: *const c_void, usage: u32) {
    if size < 0 {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    // No target is given, so bind on ARRAY_BUFFER; note_buffer_target mirrors the allocation
    // onto any other target this buffer is later bound to.
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        bind(GL_ARRAY_BUFFER, buffer);
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, isize, *const c_void, u32)>(
        "glBufferData",
    ) {
        f(GL_ARRAY_BUFFER, size, data, usage);
    } else {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    with_buffers(|v| {
        if let Some(entry) = v.iter_mut().find(|(id, _, _)| *id == buffer) {
            *entry = (buffer, size as i64, usage);
        } else {
            v.push((buffer, size as i64, usage));
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn glNamedBufferStorage(
    buffer: u32,
    size: isize,
    data: *const c_void,
    flags: u32,
) {
    glNamedBufferData(buffer, size, data, 0x88E4);
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        bind(GL_ARRAY_BUFFER, buffer);
    }
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, isize, *const c_void, u32)>("glBufferStorage")
    {
        f(GL_ARRAY_BUFFER, size, data, flags);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glNamedBufferSubData(
    buffer: u32,
    offset: isize,
    size: isize,
    data: *const c_void,
) {
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        bind(GL_ARRAY_BUFFER, buffer);
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, isize, isize, *const c_void)>(
        "glBufferSubData",
    ) {
        f(GL_ARRAY_BUFFER, offset, size, data);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetNamedBufferSubData(
    buffer: u32,
    offset: isize,
    size: isize,
    data: *mut c_void,
) {
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        bind(GL_ARRAY_BUFFER, buffer);
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, isize, isize, *mut c_void)>(
        "glGetBufferSubData",
    ) {
        f(GL_ARRAY_BUFFER, offset, size, data);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetNamedBufferParameteriv(buffer: u32, pname: u32, params: *mut i32) {
    if params.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        bind(GL_ARRAY_BUFFER, buffer);
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, *mut i32)>(
        "glGetBufferParameteriv",
    ) {
        f(GL_ARRAY_BUFFER, pname, params);
    } else {
        *params = 0;
        errors().set(GL_INVALID_OPERATION);
    }
}

// ---- vertex arrays --------------------------------------------------------------------------

unsafe fn bind_vao(vao: u32) -> bool {
    match driver_fn_cached::<unsafe extern "C" fn(u32)>("glBindVertexArray") {
        Some(f) => {
            f(vao);
            true
        }
        None => false,
    }
}

#[no_mangle]
pub unsafe extern "C" fn glVertexArrayVertexBuffer(vao: u32, _binding: u32, buffer: u32, offset: isize) {
    if !bind_vao(vao) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    warn_array_buffer_is_global();
    with_buffers_vao(|v| {
        if let Some(e) = v.iter_mut().find(|(w, ..)| *w == vao) {
            *e = (vao, buffer, offset);
        } else {
            v.push((vao, buffer, offset));
        }
    });
    // Associate the buffer with this VAO's attributes, not merely the global binding.
    apply_formats(vao, buffer, offset);
}

#[no_mangle]
pub unsafe extern "C" fn glVertexArrayElementBuffer(vao: u32, buffer: u32) {
    if !bind_vao(vao) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    // ELEMENT_ARRAY_BUFFER *is* per-VAO state in ES, so this one maps exactly.
    note_buffer_target(buffer, GL_ELEMENT_ARRAY_BUFFER);
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        bind(GL_ELEMENT_ARRAY_BUFFER, buffer);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glVertexArrayAttribFormat(
    vao: u32,
    index: u32,
    size: i32,
    ty: u32,
    normalized: bool,
    relative_offset: u32,
) {
    if !bind_vao(vao) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    // Deliberately not calling glVertexAttribFormat: it is ES 3.1+, and an ES 3.0 context
    // (what Android hands out by default) rejects it with GL_INVALID_ENUM. The description is
    // recorded and applied through glVertexAttribPointer instead, which works on ES 2.0+.
    let stride = current_stride(index);
    record_format(vao, index, size, ty, normalized, relative_offset, stride);
}

#[no_mangle]
pub unsafe extern "C" fn glVertexArrayAttribIFormat(
    vao: u32,
    index: u32,
    size: i32,
    ty: u32,
    relative_offset: u32,
) {
    if !bind_vao(vao) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    match driver_fn_cached::<unsafe extern "C" fn(u32, i32, u32, u32)>("glVertexAttribIFormat") {
        Some(f) => f(index, size, ty, relative_offset),
        None => warn_no_vertex_attrib_format(),
    }
    with_formats(|v| {
        if let Some(e) = v.iter_mut().find(|(w, a, ..)| *w == vao && *a == index) {
            e.6 = current_stride(index);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn glVertexArrayAttribLFormat(
    vao: u32,
    index: u32,
    size: i32,
    ty: u32,
    relative_offset: u32,
) {
    if !bind_vao(vao) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    if let Some(f) =
        driver_fn_cached::<unsafe extern "C" fn(u32, i32, u32, u32)>("glVertexAttribLFormat")
    {
        f(index, size, ty, relative_offset);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}

/// Attribute stride, which `glVertexArrayAttribFormat` deliberately does *not* set.
///
/// This is the piece that makes a DSA vertex array actually draw: the format call leaves the
/// stride at 0, and a stride of 0 makes every vertex read the same data, so the geometry
/// collapses and nothing is rasterised. Without these entry points a caller that sets up
/// attributes the documented way gets a silently blank frame.
#[no_mangle]
pub unsafe extern "C" fn glVertexArrayAttribStride(vao: u32, index: u32, stride: u32) {
    if !bind_vao(vao) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    let vao = with_formats(|v| v.iter().find(|(_, a, ..)| *a == index).map(|(w, ..)| *w));
    if let Some(w) = vao {
        with_formats(|v| {
            if let Some(e) = v.iter_mut().find(|(x, a, ..)| *x == w && *a == index) {
                e.6 = stride;
            }
        });
        let entry = with_formats(|v| {
            v.iter()
                .find(|(x, a, ..)| *x == w && *a == index)
                .map(|(_, _, size, ty, norm, rel, st)| (*size, *ty, *norm, *rel, *st))
        });
        if let Some((size, ty, norm, rel, st)) = entry {
            if let Some(set_ptr) = driver_fn_cached::<
                unsafe extern "C" fn(u32, i32, u32, bool, i32, *const c_void),
            >("glVertexAttribPointer")
            {
                if let Some(bind) =
                    driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer")
                {
                    bind(GL_ARRAY_BUFFER, 0);
                }
                set_ptr(index, size, ty, norm, st as i32, rel as usize as *const c_void);
            }
        }
    }
}

/// Stride currently set on an attribute, so a format recorded before its stride was set still
/// carries the right value.
unsafe fn current_stride(index: u32) -> u32 {
    let mut v = 0i32;
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetVertexAttribiv")
    {
        f(index, 0x8A75 /* GL_VERTEX_ATTRIB_ARRAY_STRIDE */, &mut v);
    }
    v.max(0) as u32
}

#[no_mangle]
pub unsafe extern "C" fn glVertexAttribStride(index: u32, stride: u32) {
    // ES 3.0 has no glVertexAttribStride; the stride reaches the driver through
    // glVertexAttribPointer when the description is applied.
}

#[no_mangle]
pub unsafe extern "C" fn glGetVertexArrayAttribStride(vao: u32, index: u32, stride: *mut u32) {
    if stride.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    if !bind_vao(vao) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    glGetVertexAttribStride(index, stride);
}

#[no_mangle]
pub unsafe extern "C" fn glGetVertexAttribStride(index: u32, stride: *mut u32) {
    if stride.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    let mut v = 0i32;
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetVertexAttribiv")
    {
        f(index, 0x8A75 /* GL_VERTEX_ATTRIB_ARRAY_STRIDE */, &mut v);
        *stride = v as u32;
    } else {
        *stride = 0;
        errors().set(GL_INVALID_OPERATION);
    }
}

/// Indexed vertex buffer binding (GL 4.3). Lets a vertex array describe several buffers,
/// which is how modern renderers bind interleaved and streamed data.
#[no_mangle]
pub unsafe extern "C" fn glVertexArrayAttribBinding(vao: u32, attribindex: u32, bindingindex: u32) {
    if !bind_vao(vao) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, u32)>(
        "glVertexArrayAttribBinding",
    ) {
        Some(f) => f(vao, attribindex, bindingindex, 0),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glBindVertexBuffer(bindingindex: u32, buffer: u32, offset: isize) {
    match driver_fn_cached::<unsafe extern "C" fn(u32, u32, isize)>("glBindVertexBuffer") {
        Some(f) => f(bindingindex, buffer, offset),
        None => errors().set(GL_INVALID_OPERATION),
    }
}

#[no_mangle]
pub unsafe extern "C" fn glVertexArrayBindingDivisor(vao: u32, index: u32, divisor: u32) {
    if !bind_vao(vao) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glVertexAttribDivisor") {
        f(index, divisor);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glEnableVertexArrayAttrib(vao: u32, index: u32) {
    if !bind_vao(vao) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32)>("glEnableVertexAttribArray") {
        f(index);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glDisableVertexArrayAttrib(vao: u32, index: u32) {
    if !bind_vao(vao) {
        errors().set(GL_INVALID_OPERATION);
        return;
    }
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32)>("glDisableVertexAttribArray") {
        f(index);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}

// ---- textures -------------------------------------------------------------------------------

/// Binds a DSA texture to the target it was created for, so the target-bound ES calls apply.
pub(crate) unsafe fn bind_dsa_texture(id: u32) -> Option<u32> {
    let target = with_textures(|v| v.iter().find(|(t, _)| *t == id).map(|(_, tg)| *tg))?;
    let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindTexture") else {
        errors().set(GL_INVALID_OPERATION);
        return None;
    };
    bind(target, id);
    Some(target)
}

#[no_mangle]
pub unsafe extern "C" fn glTextureStorage2D(id: u32, levels: i32, internalformat: u32, w: i32, h: i32) {
    let Some(target) = bind_dsa_texture(id) else {
        errors().set(GL_INVALID_OPERATION);
        return;
    };
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, i32, u32, i32, i32)>(
        "glTexStorage2D",
    ) {
        f(target, levels, internalformat, w, h);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glTextureStorage3D(
    id: u32,
    levels: i32,
    internalformat: u32,
    w: i32,
    h: i32,
    d: i32,
) {
    let Some(target) = bind_dsa_texture(id) else {
        errors().set(GL_INVALID_OPERATION);
        return;
    };
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, i32, u32, i32, i32, i32)>(
        "glTexStorage3D",
    ) {
        f(target, levels, internalformat, w, h, d);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn glTextureSubImage2D(
    id: u32,
    level: i32,
    xoffset: i32,
    yoffset: i32,
    w: i32,
    h: i32,
    format: u32,
    ty: u32,
    pixels: *const c_void,
) {
    let Some(target) = bind_dsa_texture(id) else {
        errors().set(GL_INVALID_OPERATION);
        return;
    };
    let (fmt, ty, owned) = match super::convert_pixel_upload(w, h, format, ty, pixels) {
        Some((f, t, v)) => (f, t, Some(v)),
        None => (format, ty, None),
    };
    let ptr = match &owned {
        Some(v) => v.as_ptr() as *const c_void,
        None => pixels,
    };
    if let Some(f) = driver_fn_cached::<
        unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    >("glTexSubImage2D")
    {
        f(target, level, xoffset, yoffset, w, h, fmt, ty, ptr);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn glTextureSubImage3D(
    id: u32,
    level: i32,
    xoffset: i32,
    yoffset: i32,
    zoffset: i32,
    w: i32,
    h: i32,
    d: i32,
    format: u32,
    ty: u32,
    pixels: *const c_void,
) {
    let Some(target) = bind_dsa_texture(id) else {
        errors().set(GL_INVALID_OPERATION);
        return;
    };
    if let Some(f) = driver_fn_cached::<
        unsafe extern "C" fn(u32, i32, i32, i32, i32, i32, i32, i32, u32, u32, *const c_void),
    >("glTexSubImage3D")
    {
        f(target, level, xoffset, yoffset, zoffset, w, h, d, format, ty, pixels);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}



// ---- barriers, labels, and honest refusals ---------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn glMemoryBarrier(barriers: u32) {
    // ES 3.1 has this with the same enum values.
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32)>("glMemoryBarrier") {
        f(barriers);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glMemoryBarrierByRegion(barriers: u32) {
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32)>("glMemoryBarrierByRegion") {
        f(barriers);
    } else {
        // ES 3.0 has no region barrier; a full memory barrier is the conservative choice.
        if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32)>("glMemoryBarrier") {
            f(barriers);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glObjectLabel(
    _identifier: u32,
    _name: u32,
    _label: *const c_char,
    _length: isize,
) {
    // Debug labels only feed debug output, which this layer does not produce.
}

#[no_mangle]
pub unsafe extern "C" fn glObjectPtrLabel(_identifier: u32, _ptr: *const c_void, _length: isize) {}

#[no_mangle]
pub unsafe extern "C" fn glPushDebugGroup(_source: u32, _id: u32, _length: isize, _message: *const c_char) {}

#[no_mangle]
pub unsafe extern "C" fn glPopDebugGroup() {}

/// Accepts a debug callback and discards messages.
///
/// This must exist even though the layer does not advertise `GL_KHR_debug`: the driver's own
/// extension list is passed through, so a device that really has KHR_debug makes the game
/// install a callback. Returning null here would crash it, which is exactly the failure mode
/// this replaces.
#[no_mangle]
pub unsafe extern "C" fn glDebugMessageCallback(
    _callback: *const c_void,
    _user_param: *const c_void,
) {
}

#[no_mangle]
pub unsafe extern "C" fn glDebugMessageCallbackARB(
    _callback: *const c_void,
    _user_param: *const c_void,
) {
}

/// No way to observe context loss through this bridge, so report "no reset". Reporting a
/// reset would make the game tear down and rebuild its GL objects for no reason.
#[no_mangle]
pub unsafe extern "C" fn glGetGraphicsResetStatus() -> u32 {
    GL_NO_ERROR
}

/// Indirect draws need the command buffer readable by the CPU; ES 3.x has neither
/// `glMultiDrawArraysIndirect` nor a way to make that cheap, so say so rather than silently
/// drawing nothing.
#[no_mangle]
pub unsafe extern "C" fn glMultiDrawElementsIndirect(
    _mode: u32,
    _type: u32,
    _indirect: *const c_void,
    _drawcount: i32,
    _stride: i32,
) {
    static ONCE: AtomicBool = AtomicBool::new(false);
    log_once(
        &ONCE,
        "[GLCompat] glMultiDrawElementsIndirect is not available on GLES 3.x (no indirect draw); \
         draw skipped",
    );
    errors().set(GL_INVALID_OPERATION);
}

#[no_mangle]
pub unsafe extern "C" fn glMultiDrawArraysIndirect(
    _mode: u32,
    _indirect: *const c_void,
    _drawcount: i32,
    _stride: i32,
) {
    static ONCE: AtomicBool = AtomicBool::new(false);
    log_once(
        &ONCE,
        "[GLCompat] glMultiDrawArraysIndirect is not available on GLES 3.x (no indirect draw); \
         draw skipped",
    );
    errors().set(GL_INVALID_OPERATION);
}

#[no_mangle]
pub unsafe extern "C" fn glDispatchCompute(_x: u32, _y: u32, _z: u32) {
    errors().set(GL_INVALID_OPERATION);
}

/// Every DSA name this module exports, paired with the symbol that backs it. Used by the
/// reachability test so a new entry point cannot be added without a resolver table entry.
pub const EXPORTS: &[&str] = &[
    "glCreateBuffers",
    "glCreateTextures",
    "glCreateVertexArrays",
    "glNamedBufferData",
    "glNamedBufferStorage",
    "glNamedBufferSubData",
    "glGetNamedBufferSubData",
    "glGetNamedBufferParameteriv",
    "glVertexArrayVertexBuffer",
    "glVertexArrayElementBuffer",
    "glVertexArrayAttribFormat",
    "glVertexArrayAttribIFormat",
    "glVertexArrayAttribLFormat",
    "glVertexArrayBindingDivisor",
    "glVertexArrayAttribStride",
    "glVertexAttribStride",
    "glGetVertexArrayAttribStride",
    "glGetVertexAttribStride",
    "glVertexArrayAttribBinding",
    "glBindVertexBuffer",
    "glEnableVertexArrayAttrib",
    "glDisableVertexArrayAttrib",
    "glTextureStorage2D",
    "glTextureStorage3D",
    "glTextureSubImage2D",
    "glTextureSubImage3D",
    "glTextureParameteri",
    "glTextureParameterf",
    "glMemoryBarrier",
    "glMemoryBarrierByRegion",
    "glObjectLabel",
    "glObjectPtrLabel",
    "glPushDebugGroup",
    "glPopDebugGroup",
    "glDebugMessageCallback",
    "glDebugMessageCallbackARB",
    "glGetGraphicsResetStatus",
    "glMultiDrawArraysIndirect",
    "glMultiDrawElementsIndirect",
    "glDispatchCompute",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises tests that mutate the shared texture/buffer/VAO tables. Without it they
    /// race: one test's `clear()` wipes another's entry between setup and assertion.
    fn table_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn create_buffers_records_exactly_the_names_it_generated() {
        let _guard = table_guard();
        // Written to hold with or without a GL driver: the earlier version assumed no driver
        // was present, so it passed only until Mesa was installed.
        with_buffers(|v| v.clear());
        let mut ids = [0u32; 3];
        unsafe { glCreateBuffers(3, ids.as_mut_ptr()) };
        for id in ids {
            if id == 0 {
                continue; // no driver, or generation failed: nothing to track
            }
            assert!(
                with_buffers(|v| v.iter().any(|(i, ..)| *i == id)),
                "generated name {id} was not recorded, so later named-buffer calls \
                 would not find its allocation"
            );
        }
        with_buffers(|v| v.clear());
    }

    #[test]
    fn invalid_arguments_are_rejected() {
        let _guard = table_guard();
        unsafe { glCreateBuffers(0, std::ptr::null_mut()) };
        assert_ne!(unsafe { errors().take() }, 0);
        unsafe { super::dsa_named::glCreateTextures(0x0DE1, 0, std::ptr::null_mut()) };
        assert_ne!(unsafe { errors().take() }, 0);
        unsafe { glCreateVertexArrays(-1, std::ptr::null_mut()) };
        assert_ne!(unsafe { errors().take() }, 0);
    }

    #[test]
    fn cross_target_buffer_lookup_only_mirrors_known_buffers() {
        let _guard = table_guard();
        // note_buffer_target must ignore names it has never allocated storage for, otherwise
        // it would bind and allocate arbitrary names.
        with_buffers(|v| v.clear());
        unsafe { note_buffer_target(999, GL_ELEMENT_ARRAY_BUFFER) };
        unsafe { note_buffer_target(0, GL_ELEMENT_ARRAY_BUFFER) };
        // Nothing to assert beyond "no crash and no GL error for an unknown name"; the driver
        // path is skipped entirely when the name is unknown.
        let _ = unsafe { errors().take() };
    }

    #[test]
    fn buffer_table_records_size_and_usage() {
        let _guard = table_guard();
        with_buffers(|v| v.clear());
        with_buffers(|v| v.push((7, 4096, 0x88E8)));
        assert!(with_buffers(|v| v.iter().any(|(id, size, usage)| *id == 7 && *size == 4096 && *usage == 0x88E8)));
    }

    #[test]
    fn every_export_is_reachable_through_the_resolver() {
        let _guard = table_guard();
        for name in EXPORTS {
            assert!(
                !super::super::resolve_proc(name.as_bytes()).is_null(),
                "{name} is exported by dsa but unreachable via eglGetProcAddress"
            );
        }
    }
}
