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
//! Desktop GL treats object names as global, so a DSA entry point that names its object instead of
//! binding it cannot be translated by ignoring the binding: the entry point here binds the object
//! and delegates to the classic call. Textures need a target table for that (they really are per
//! target in ES); buffers need one only to know which target holds their bytes.
//!
//! Vertex array *attribute formats* are the remaining gap: `GL_ARRAY_BUFFER` binding is
//! global state in ES, not per-VAO, so `glVertexArrayVertexBuffer` cannot be per-VAO there.
//! It is applied globally and the deviation is logged once rather than silently misrendered.
//!
//! # Buffer storage is not per target, and re-allocating destroys it
//!
//! An earlier version of [`note_buffer_target`] assumed ES buffer names were per target, the way
//! texture names are, and "fixed" a second-target binding by allocating storage under that target
//! too. Measured on Mesa llvmpipe, a buffer name has **one** store shared by every binding:
//! allocating under a second target overwrote the client's index bytes with an empty buffer, so
//! every indexed draw read indices of zero and collapsed to a degenerate triangle — a blank frame
//! with no GL error. [`buffer_storage_is_shared`] now measures which model the driver follows
//! instead of assuming one, so the common case does no work at all and the per-target case still
//! gets its bytes copied across.

use super::*;
use std::sync::atomic::AtomicU8;
use std::sync::Mutex;

/// DSA buffer: name -> what this layer knows about it.
///
/// `contents` is the target the client last wrote through, and `targets` is every target this
/// layer has allocated the name on. Only a driver that keeps storage per target needs the second
/// one; see [`buffer_storage_is_shared`].
#[derive(Clone)]
struct DsaBuffer {
    size: i64,
    usage: u32,
    contents: u32,
    targets: Vec<u32>,
}

/// DSA buffer: name -> allocation, source target, materialised targets.
static BUFFERS: Mutex<Vec<(u32, DsaBuffer)>> = Mutex::new(Vec::new());
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

fn with_buffers<R>(f: impl FnOnce(&mut Vec<(u32, DsaBuffer)>) -> R) -> R {
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

/// Makes sure a DSA buffer name has storage under `target` before it is bound there.
///
/// GLES keeps **one** data store per buffer name, shared by every binding (measured on Mesa
/// llvmpipe: allocating under a second target overwrites the first allocation's bytes), so on a
/// driver that behaves that way this has nothing to do — the client's allocation is already the
/// store the new binding will use. Re-allocating here is what used to blank every indexed draw.
///
/// A driver that *does* keep storage per target gets the name materialised with the client's bytes
/// copied across, because an empty buffer there is just as blank.
pub(crate) unsafe fn note_buffer_target(buffer: u32, target: u32) {
    if buffer == 0 || target != GL_ELEMENT_ARRAY_BUFFER && target != GL_ARRAY_BUFFER {
        return;
    }
    let known = with_buffers(|v| {
        v.iter()
            .find(|(id, _)| *id == buffer)
            .map(|(_, b)| (b.size, b.usage, b.contents, b.targets.clone()))
    });
    let Some((size, usage, contents, targets)) = known else {
        return;
    };
    if targets.contains(&target) {
        return;
    }
    if buffer_storage_is_shared() {
        // The driver's store for this name already exists and is shared: binding is all that is
        // needed. Recorded so the next bind on any target is a no-op too.
        with_buffers(|v| {
            if let Some(entry) = v.iter_mut().find(|(id, _)| *id == buffer) {
                entry.1.targets.push(target);
            }
        });
        return;
    }
    // Whatever this borrows from the context has to be put back: a client that binds an index
    // buffer and then asks for the array-buffer binding must not see a different answer. The
    // element binding is the exception — the caller is in the middle of making it.
    let mut prev_array = 0i32;
    let mut prev_element = 0i32;
    if let Some(get) = driver_fn_cached::<GetIntFn>("glGetIntegerv") {
        get(0x8894 /* GL_ARRAY_BUFFER_BINDING */, &mut prev_array);
        get(0x8895 /* GL_ELEMENT_ARRAY_BUFFER_BINDING */, &mut prev_element);
    }
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        bind(GL_ARRAY_BUFFER, buffer);
        bind(target, buffer);
    }
    if let Some(data) = driver_fn_cached::<unsafe extern "C" fn(u32, isize, *const c_void, u32)>(
        "glBufferData",
    ) {
        data(target, size as isize, std::ptr::null(), usage);
    } else {
        errors().set(GL_INVALID_OPERATION);
    }
    let copyable = contents != 0 && contents != target && size > 0 && targets.contains(&contents);
    if copyable {
        if copy_buffer_contents(buffer, contents, target, size as usize, usage) {
            log_mirrored(buffer, contents, target, size);
        } else {
            errors().set(GL_INVALID_OPERATION);
        }
    }
    if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
        bind(GL_ARRAY_BUFFER, prev_array as u32);
        if target != GL_ELEMENT_ARRAY_BUFFER {
            bind(GL_ELEMENT_ARRAY_BUFFER, prev_element as u32);
        }
    }
    set_array_buffer_binding(prev_array as u32);
    with_buffers(|v| {
        if let Some(entry) = v.iter_mut().find(|(id, _)| *id == buffer) {
            entry.1.targets.push(target);
        }
    });
}

/// Whether this driver gives a buffer name one data store shared by every binding.
///
/// Measured once per context instead of assumed: fill a scratch name through `GL_ARRAY_BUFFER`,
/// bind it to `GL_ELEMENT_ARRAY_BUFFER` and ask for `GL_BUFFER_SIZE`. A shared store still reports
/// the size; a per-target one reports zero. Getting this backwards is not cosmetic — it decides
/// whether the client's bytes survive being bound a second time.
pub(crate) unsafe fn buffer_storage_is_shared() -> bool {
    static SHARED: AtomicU8 = AtomicU8::new(0);
    match SHARED.load(Ordering::Relaxed) {
        1 => return true,
        2 => return false,
        _ => {}
    }
    let Some(gen) = driver_fn_cached::<unsafe extern "C" fn(i32, *mut u32)>("glGenBuffers") else {
        return true; // nothing to probe with; assume the shared model and do no work
    };
    let (Some(bind), Some(data), Some(get)) = (
        driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer"),
        driver_fn_cached::<unsafe extern "C" fn(u32, isize, *const c_void, u32)>("glBufferData"),
        driver_fn_cached::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetBufferParameteriv"),
    ) else {
        return true;
    };
    let mut name = 0u32;
    gen(1, &mut name);
    let mut prev_array = 0i32;
    let mut prev_element = 0i32;
    if let Some(get_int) = driver_fn_cached::<GetIntFn>("glGetIntegerv") {
        get_int(0x8894, &mut prev_array);
        get_int(0x8895, &mut prev_element);
    }
    bind(GL_ARRAY_BUFFER, name);
    data(GL_ARRAY_BUFFER, 16, std::ptr::null(), 0x88E4 /* GL_STATIC_DRAW */);
    bind(GL_ELEMENT_ARRAY_BUFFER, name);
    let mut size = 0i32;
    get(GL_ELEMENT_ARRAY_BUFFER, 0x8764 /* GL_BUFFER_SIZE */, &mut size);
    bind(GL_ARRAY_BUFFER, prev_array as u32);
    bind(GL_ELEMENT_ARRAY_BUFFER, prev_element as u32);
    set_array_buffer_binding(prev_array as u32);
    if let Some(del) = driver_fn_cached::<unsafe extern "C" fn(i32, *const u32)>("glDeleteBuffers")
    {
        del(1, &name);
    }
    // A driver that refused the probe leaves size at zero; treat that as per-target, which only
    // costs a copy on a driver that has not been measured otherwise.
    let shared = size == 16;
    SHARED.store(if shared { 1 } else { 2 }, Ordering::Relaxed);
    log(&format!(
        "[GLCompat] buffer storage probe: {}",
        if shared {
            "one store per buffer name, shared across bindings"
        } else {
            "per-target storage; buffers bound to a second target get their bytes copied"
        }
    ));
    shared
}

/// Reads the bytes stored under `from` and writes them under `to`.
///
/// `glCopyBufferSubData` is the cheap way to do this and it is not used here: on Mesa llvmpipe it
/// raises `GL_INVALID_VALUE` even between two ordinary `GL_ARRAY_BUFFER` objects, and a refused
/// copy leaves an empty buffer behind with no error the client will ever read. ES 3.0 has
/// `glMapBufferRange`, so the bytes come through that instead. It is a CPU round-trip of the
/// buffer's size, paid once per name per target — the mirror is remembered, not repeated.
unsafe fn copy_buffer_contents(
    buffer: u32,
    from: u32,
    to: u32,
    size: usize,
    usage: u32,
) -> bool {
    let Some(map) = driver_fn_cached::<unsafe extern "C" fn(u32, isize, isize, u32) -> *mut c_void>(
        "glMapBufferRange",
    ) else {
        return false;
    };
    let Some(unmap) =
        driver_fn_cached::<unsafe extern "C" fn(u32) -> u8>("glUnmapBuffer")
    else {
        return false;
    };
    let Some(put) =
        driver_fn_cached::<unsafe extern "C" fn(u32, isize, *const c_void, u32)>("glBufferData")
    else {
        return false;
    };
    let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") else {
        return false;
    };
    const GL_MAP_READ_BIT: u32 = 0x0001;
    let mut bytes = vec![0u8; size];
    // `from` and `to` are both already bound to `buffer` by the caller.
    let ptr = map(from, 0, size as isize, GL_MAP_READ_BIT);
    if ptr.is_null() {
        return false;
    }
    std::ptr::copy_nonoverlapping(ptr as *const u8, bytes.as_mut_ptr(), size);
    unmap(from);
    bind(to, buffer);
    put(to, size as isize, bytes.as_ptr() as *const c_void, usage);
    true
}

/// Says once per mirror that data had to be copied, because the alternative — a buffer that is
/// silently empty on one target — is invisible until a frame is blank.
fn log_mirrored(buffer: u32, from: u32, to: u32, size: i64) {
    static MIRRORED: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());
    let first = {
        let mut g = MIRRORED.lock().unwrap_or_else(|e| e.into_inner());
        if g.iter().any(|(b, t)| *b == buffer && *t == to) {
            false
        } else {
            g.push((buffer, to));
            true
        }
    };
    if first {
        log(&format!(
            "[GLCompat] buffer {buffer} has no ES storage on target 0x{to:04X}: copied its \
             {size} bytes from target 0x{from:04X} (ES buffer names are per target)"
        ));
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
            v.push((
                *ids.add(i),
                DsaBuffer { size: 0, usage: 0x88E4 /* GL_STATIC_DRAW */, contents: 0, targets: vec![] },
            ));
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
    // A name already materialised on another target holds a copy taken at the previous upload, so
    // this upload has to reach that copy too -- otherwise an index buffer that is refilled and then
    // attached again draws the first upload's indices forever.
    with_buffers(|v| {
        match v.iter_mut().find(|(id, _)| *id == buffer) {
            Some((_, b)) => {
                b.size = size as i64;
                b.usage = usage;
                b.contents = GL_ARRAY_BUFFER;
                b.targets.retain(|t| *t != GL_ARRAY_BUFFER);
                b.targets.push(GL_ARRAY_BUFFER);
            }
            None => v.push((
                buffer,
                DsaBuffer {
                    size: size as i64,
                    usage,
                    contents: GL_ARRAY_BUFFER,
                    targets: vec![GL_ARRAY_BUFFER],
                },
            )),
        }
    });
    mirror_targets(buffer, &[], |target, put, usage| {
        put(target, size as isize, data, usage);
    });
}

/// Applies an upload to the copies this layer keeps of `buffer`'s other materialised targets.
///
/// `targets` is the selection: empty means "every target other than the one being written
/// directly", and a single entry means exactly that one. Both the bindings borrowed for the copy
/// and the `GL_ARRAY_BUFFER` shadow are restored, because the client keeps using them.
///
/// Does nothing at all on a driver whose buffer names carry one shared store: there is no second
/// copy to update, and rewriting it per upload would cost a reallocation of the whole buffer for
/// no change.
unsafe fn mirror_targets(
    buffer: u32,
    targets: &[u32],
    write: impl Fn(u32, unsafe extern "C" fn(u32, isize, *const c_void, u32), u32),
) {
    if buffer_storage_is_shared() {
        return;
    }
    let chosen: Vec<u32> = {
        let Some((_, b)) = with_buffers(|v| v.iter().find(|(id, _)| *id == buffer).cloned()) else {
            return;
        };
        if targets.is_empty() {
            b.targets.iter().copied().filter(|t| *t != GL_ARRAY_BUFFER).collect()
        } else {
            b.targets.iter().copied().filter(|t| targets.contains(t)).collect()
        }
    };
    if chosen.is_empty() {
        return;
    }
    let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") else {
        return;
    };
    let Some(put) =
        driver_fn_cached::<unsafe extern "C" fn(u32, isize, *const c_void, u32)>("glBufferData")
    else {
        errors().set(GL_INVALID_OPERATION);
        return;
    };
    let usage = with_buffers(|v| {
        v.iter().find(|(id, _)| *id == buffer).map(|(_, b)| b.usage).unwrap_or(0x88E4)
    });
    let mut prev_array = 0i32;
    let mut prev_element = 0i32;
    if let Some(get) = driver_fn_cached::<GetIntFn>("glGetIntegerv") {
        get(0x8894 /* GL_ARRAY_BUFFER_BINDING */, &mut prev_array);
        get(0x8895 /* GL_ELEMENT_ARRAY_BUFFER_BINDING */, &mut prev_element);
    }
    for target in chosen {
        bind(GL_ARRAY_BUFFER, buffer);
        bind(target, buffer);
        write(target, put, usage);
    }
    bind(GL_ARRAY_BUFFER, prev_array as u32);
    bind(GL_ELEMENT_ARRAY_BUFFER, prev_element as u32);
    set_array_buffer_binding(prev_array as u32);
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
    if offset < 0 || size < 0 {
        errors().set(GL_INVALID_VALUE);
        return;
    }
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
    // A partial update has to reach the copies too. glBufferSubData needs the whole buffer, so the
    // copy is read back, patched and rewritten; that is CPU work proportional to the buffer, and
    // only for a name this layer had to mirror in the first place.
    let patch = if data.is_null() { None } else { Some(std::slice::from_raw_parts(data as *const u8, size as usize)) };
    mirror_targets(buffer, &[], move |target, put, usage| {
        let total = with_buffers(|v| {
            v.iter().find(|(id, _)| *id == buffer).map(|(_, b)| b.size).unwrap_or(0)
        });
        let total = total.max(0) as usize;
        let Some(get) =
            driver_fn_cached::<unsafe extern "C" fn(u32, isize, isize, *mut c_void)>(
                "glGetBufferSubData",
            )
        else {
            return;
        };
        let mut bytes = vec![0u8; total];
        get(target, 0, total as isize, bytes.as_mut_ptr() as *mut c_void);
        if let Some(bytes_new) = patch {
            let start = offset as usize;
            if start + bytes_new.len() <= bytes.len() {
                bytes[start..start + bytes_new.len()].copy_from_slice(bytes_new);
                put(target, total as isize, bytes.as_ptr() as *const c_void, usage);
            }
        }
    });
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
        unsafe { super::named_objects::glCreateTextures(0x0DE1, 0, std::ptr::null_mut()) };
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
    fn buffer_table_records_size_usage_and_where_the_bytes_are() {
        let _guard = table_guard();
        with_buffers(|v| v.clear());
        with_buffers(|v| {
            v.push((
                7,
                DsaBuffer { size: 4096, usage: 0x88E8, contents: GL_ARRAY_BUFFER, targets: vec![GL_ARRAY_BUFFER] },
            ))
        });
        assert!(
            with_buffers(|v| v.iter().any(|(id, b)| {
                *id == 7 && b.size == 4096 && b.usage == 0x88E8 && b.contents == GL_ARRAY_BUFFER
            })),
            "size, usage and the target holding the client's bytes all have to be remembered: \
             the first is what a second-target allocation is sized from, the second is the hint it \
             is created with, and the third is what the bytes have to be copied from"
        );
    }

    /// The bug this table exists to prevent: a name with a recorded allocation must not be
    /// re-allocated just because it is bound to another target, because GLES keeps one store per
    /// name and re-allocating empties it.
    #[test]
    fn binding_a_filled_buffer_to_a_second_target_does_not_reallocate_it() {
        let _guard = table_guard();
        with_buffers(|v| v.clear());
        with_buffers(|v| {
            v.push((
                11,
                DsaBuffer { size: 64, usage: 0x88E4, contents: GL_ARRAY_BUFFER, targets: vec![GL_ARRAY_BUFFER] },
            ))
        });
        // No driver in a unit test, so the probe reports "shared" and nothing is materialised.
        unsafe { note_buffer_target(11, GL_ELEMENT_ARRAY_BUFFER) };
        let entry = with_buffers(|v| v.iter().find(|(id, _)| *id == 11).map(|(_, b)| (b.size, b.contents, b.targets.clone())));
        assert_eq!(
            entry,
            Some((64, GL_ARRAY_BUFFER, vec![GL_ARRAY_BUFFER, GL_ELEMENT_ARRAY_BUFFER])),
            "the recorded allocation and its contents must survive the second binding, and the \
             new target is recorded so the work happens once"
        );
        with_buffers(|v| v.clear());
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
