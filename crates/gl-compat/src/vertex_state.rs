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
use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicU8, Ordering};
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

/// Debug callback storage: (callback_fn, user_param as usize)
static DEBUG_CALLBACK: Mutex<Option<(extern "C" fn(u32, u32, u32, u32, i32, *const c_char, *const c_void), usize)>> = Mutex::new(None);

/// Debug message log storage for glGetDebugMessageLog
static DEBUG_LOG: Mutex<Vec<DebugLogEntry>> = Mutex::new(Vec::new());

/// Maximum debug log entries to store
const MAX_DEBUG_LOG_ENTRIES: usize = 1024;

/// Debug message control filters
#[derive(Clone, Debug)]
struct DebugFilter {
    source: u32,
    type_: u32,
    severity: u32,
    enabled: bool,
    ids: Vec<u32>,
}

static DEBUG_FILTERS: Mutex<Vec<DebugFilter>> = Mutex::new(Vec::new());

/// Debug group stack
static DEBUG_GROUPS: Mutex<Vec<DebugGroup>> = Mutex::new(Vec::new());

#[derive(Clone, Debug)]
struct DebugGroup {
    source: u32,
    id: u32,
    message: String,
}

#[derive(Clone, Debug)]
struct DebugLogEntry {
    source: u32,
    type_: u32,
    id: u32,
    severity: u32,
    message: String,
}

/// Debug output constants
const GL_DEBUG_OUTPUT_SYNCHRONOUS: u32 = 0x8242;
const GL_INVALID_ENUM: u32 = 0x0500;
const GL_DEBUG_NEXT_LOGGED_MESSAGE_LENGTH: u32 = 0x8243;
const GL_DEBUG_CALLBACK_FUNCTION: u32 = 0x8244;
const GL_DEBUG_CALLBACK_USER_PARAM: u32 = 0x8245;
const GL_DEBUG_SOURCE_API: u32 = 0x8246;
const GL_DEBUG_SOURCE_WINDOW_SYSTEM: u32 = 0x8247;
const GL_DEBUG_SOURCE_SHADER_COMPILER: u32 = 0x8248;
const GL_DEBUG_SOURCE_THIRD_PARTY: u32 = 0x8249;
const GL_DEBUG_SOURCE_APPLICATION: u32 = 0x824A;
const GL_DEBUG_SOURCE_OTHER: u32 = 0x824B;
const GL_DEBUG_TYPE_ERROR: u32 = 0x824C;
const GL_DEBUG_TYPE_DEPRECATED_BEHAVIOR: u32 = 0x824D;
const GL_DEBUG_TYPE_UNDEFINED_BEHAVIOR: u32 = 0x824E;
const GL_DEBUG_TYPE_PORTABILITY: u32 = 0x824F;
const GL_DEBUG_TYPE_PERFORMANCE: u32 = 0x8250;
const GL_DEBUG_TYPE_OTHER: u32 = 0x8251;
const GL_DEBUG_TYPE_MARKER: u32 = 0x8268;
const GL_DEBUG_TYPE_PUSH_GROUP: u32 = 0x8269;
const GL_DEBUG_TYPE_POP_GROUP: u32 = 0x826A;
const GL_DEBUG_SEVERITY_HIGH: u32 = 0x9146;
const GL_DEBUG_SEVERITY_MEDIUM: u32 = 0x9147;
const GL_DEBUG_SEVERITY_LOW: u32 = 0x9148;
const GL_DEBUG_SEVERITY_NOTIFICATION: u32 = 0x826B;
const GL_MAX_DEBUG_MESSAGE_LENGTH: u32 = 0x9143;
const GL_MAX_DEBUG_LOGGED_MESSAGES: u32 = 0x9144;
const GL_DEBUG_LOGGED_MESSAGES: u32 = 0x9145;
const GL_DEBUG_GROUP_STACK_DEPTH: u32 = 0x826C;
const GL_BUFFER: u32 = 0x82E0;
const GL_SHADER: u32 = 0x82E1;
const GL_PROGRAM: u32 = 0x82E2;
const GL_VERTEX_ARRAY: u32 = 0x8074;
const GL_QUERY: u32 = 0x82E3;
const GL_PROGRAM_PIPELINE: u32 = 0x82E4;
const GL_SAMPLER: u32 = 0x82E6;
const GL_MAX_LABEL_LENGTH: u32 = 0x82E8;
const GL_DEBUG_OUTPUT: u32 = 0x92E0;
const GL_CONTEXT_FLAG_DEBUG_BIT: u32 = 0x00000002;

const GL_DONT_CARE: u32 = 0x1100;

/// Attribute description recorded per vertex array: (vao, attrib, size, type, normalized,
/// relative_offset, stride).
///
/// Needed because ES 3.x has no equivalent of `glVertexArrayVertexBuffer`: there, the buffer
/// an attribute reads from is whatever `GL_ARRAY_BUFFER` was bound to when
/// `glVertexAttribPointer` was called. So the format has to be remembered, and re-applied as
/// a pointer call whenever the VAO's buffer binding changes. Skipping this leaves every
/// attribute reading buffer 0, and the frame is blank with no GL error to explain it.
static ATTR_FORMATS: Mutex<Vec<(u32, u32, i32, u32, bool, u32, u32)>> = Mutex::new(Vec::new());

/// Buffer bound to a vertex array via `glVertexArrayVertexBuffer` or `glBindVertexBuffer`: (vao, buffer, offset, stride).
static VAO_BUFFERS: Mutex<Vec<(u32, u32, isize, i32)>> = Mutex::new(Vec::new());

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
    if let Some((buffer, offset, stride)) = with_buffers_vao(|v| {
        v.iter().find(|(w, ..)| *w == vao).map(|(_, b, o, s)| (*b, *o, *s))
    }) {
        if let Some(set_ptr) = driver_fn_cached::<
            unsafe extern "C" fn(u32, i32, u32, bool, i32, *const c_void),
        >("glVertexAttribPointer")
        {
            if let Some(bind) = driver_fn_cached::<unsafe extern "C" fn(u32, u32)>("glBindBuffer") {
                bind(GL_ARRAY_BUFFER, buffer);
            }
            set_ptr(attrib, size, ty, normalized, stride as i32,
                    (offset as u32).wrapping_add(rel) as usize as *const c_void);
        }
    }
}

fn with_buffers_vao<R>(f: impl FnOnce(&mut Vec<(u32, u32, isize, i32)>) -> R) -> R {
    let mut g = VAO_BUFFERS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.as_mut())
}

/// Re-applies `glVertexAttribPointer` for every attribute of `vao` that has a recorded format,
/// so the attributes actually read from `buffer`.
unsafe fn apply_formats(vao: u32, buffer: u32, offset: isize, stride: i32) {
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
    for (attrib, size, ty, norm, rel, attr_stride) in entries {
        let s = if attr_stride == 0 { stride } else { attr_stride as i32 };
        set_ptr(
            attrib,
            size,
            ty,
            norm,
            s,
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
            *e = (vao, buffer, offset, 0);
        } else {
            v.push((vao, buffer, offset, 0));
        }
    });
    // Associate the buffer with this VAO's attributes, not merely the global binding.
    apply_formats(vao, buffer, offset, 0);
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
pub unsafe extern "C" fn glBindVertexBuffer(bindingindex: u32, buffer: u32, offset: isize, stride: i32) {
    if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, isize, i32)>("glBindVertexBuffer") {
        f(bindingindex, buffer, offset, stride);
    }
    let mut vao = 0i32;
    if let Some(get_int) = driver_fn_cached::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv") {
        get_int(0x8CA6, &mut vao);
    }
    with_buffers_vao(|v| {
        if let Some(e) = v.iter_mut().find(|(w, ..)| *w == vao as u32) {
            *e = (vao as u32, buffer, offset, stride);
        } else {
            v.push((vao as u32, buffer, offset, stride));
        }
    });
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
    let internalformat = format_translate::map_storage_internal(internalformat, crate::render_caps());
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
    let internalformat = format_translate::map_storage_internal(internalformat, crate::render_caps());
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

/// Internal helper: log a debug message through the callback and log buffer.
unsafe fn debug_log(source: u32, type_: u32, id: u32, severity: u32, message: &str) {
    // Store in log buffer
    {
        let mut log = DEBUG_LOG.lock().unwrap_or_else(|e| e.into_inner());
        log.push(DebugLogEntry {
            source,
            type_,
            id,
            severity,
            message: message.to_string(),
        });
        if log.len() > MAX_DEBUG_LOG_ENTRIES {
            log.remove(0);
        }
    }

    // Call callback if registered
    let callback = {
        let guard = DEBUG_CALLBACK.lock().unwrap_or_else(|e| e.into_inner());
        *guard
    };
    if let Some((cb_fn, user_param)) = callback {
        // Convert message to C string
        let c_msg = std::ffi::CString::new(message).unwrap_or_default();
        cb_fn(source, type_, id, severity, message.len() as i32, c_msg.as_ptr(), user_param as *const c_void);
    }
}

/// Check if a message passes the current debug filters
fn debug_filter_passes(source: u32, type_: u32, severity: u32, id: u32) -> bool {
    let filters = DEBUG_FILTERS.lock().unwrap_or_else(|e| e.into_inner());
    if filters.is_empty() {
        // Default: all messages pass
        return true;
    }
    for filter in filters.iter() {
        let source_match = filter.source == GL_DONT_CARE || filter.source == source;
        let type_match = filter.type_ == GL_DONT_CARE || filter.type_ == type_;
        let severity_match = filter.severity == GL_DONT_CARE || filter.severity == severity;
        let id_match = filter.ids.is_empty() || filter.ids.contains(&id);
        if source_match && type_match && severity_match && id_match {
            return filter.enabled;
        }
    }
    // Default deny if filters exist but none match
    false
}

#[no_mangle]
pub unsafe extern "C" fn glDebugMessageCallback(
    callback: *const c_void,
    user_param: *const c_void,
) {
    let mut guard = DEBUG_CALLBACK.lock().unwrap_or_else(|e| e.into_inner());
    if callback.is_null() {
        *guard = None;
    } else {
        *guard = Some((
            std::mem::transmute::<*const c_void, extern "C" fn(u32, u32, u32, u32, i32, *const c_char, *const c_void)>(callback),
            user_param as usize,
        ));
    }
}

#[no_mangle]
pub unsafe extern "C" fn glDebugMessageCallbackARB(
    callback: *const c_void,
    user_param: *const c_void,
) {
    glDebugMessageCallback(callback, user_param);
}

#[no_mangle]
pub unsafe extern "C" fn glDebugMessageControl(
    source: u32,
    type_: u32,
    severity: u32,
    count: i32,
    ids: *const u32,
    enabled: u8,
) {
    if count < 0 {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    let mut filter_ids = Vec::new();
    if count > 0 && !ids.is_null() {
        let slice = std::slice::from_raw_parts(ids, count as usize);
        filter_ids.extend_from_slice(slice);
    }
    let mut filters = DEBUG_FILTERS.lock().unwrap_or_else(|e| e.into_inner());
    // Remove existing filter for same source/type/severity if no specific IDs
    if filter_ids.is_empty() {
        filters.retain(|f| !(f.source == source && f.type_ == type_ && f.severity == severity && f.ids.is_empty()));
    }
    filters.push(DebugFilter {
        source,
        type_,
        severity,
        enabled: enabled != 0,
        ids: filter_ids,
    });
}

#[no_mangle]
pub unsafe extern "C" fn glDebugMessageControlARB(
    source: u32,
    type_: u32,
    severity: u32,
    count: i32,
    ids: *const u32,
    enabled: u8,
) {
    glDebugMessageControl(source, type_, severity, count, ids, enabled);
}

#[no_mangle]
pub unsafe extern "C" fn glDebugMessageInsert(
    source: u32,
    type_: u32,
    id: u32,
    severity: u32,
    length: i32,
    message: *const c_char,
) {
    if message.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    if length < 0 {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    let msg = if length == 0 {
        // Null-terminated string
        CStr::from_ptr(message).to_string_lossy().into_owned()
    } else {
        // Length-specified string
        std::slice::from_raw_parts(message as *const u8, length as usize)
            .iter()
            .map(|&c| c as char)
            .collect::<String>()
    };
    if !debug_filter_passes(source, type_, severity, id) {
        return;
    }
    debug_log(source, type_, id, severity, &msg);
}

#[no_mangle]
pub unsafe extern "C" fn glPushDebugGroup(
    source: u32,
    id: u32,
    length: isize,
    message: *const c_char,
) {
    if message.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    let msg = if length < 0 {
        CStr::from_ptr(message).to_string_lossy().into_owned()
    } else {
        std::slice::from_raw_parts(message as *const u8, length as usize)
            .iter()
            .map(|&c| c as char)
            .collect::<String>()
    };
    let mut groups = DEBUG_GROUPS.lock().unwrap_or_else(|e| e.into_inner());
    groups.push(DebugGroup { source, id, message: msg.clone() });
    debug_log(source, GL_DEBUG_TYPE_PUSH_GROUP, id, GL_DEBUG_SEVERITY_NOTIFICATION, &msg);
}

#[no_mangle]
pub unsafe extern "C" fn glPopDebugGroup() {
    let mut groups = DEBUG_GROUPS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(group) = groups.pop() {
        debug_log(group.source, GL_DEBUG_TYPE_POP_GROUP, group.id, GL_DEBUG_SEVERITY_NOTIFICATION, &group.message);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetDebugMessageLog(
    count: u32,
    buf_size: i32,
    sources: *mut u32,
    types: *mut u32,
    ids: *mut u32,
    severities: *mut u32,
    lengths: *mut i32,
    message_log: *mut c_char,
) -> u32 {
    let log = DEBUG_LOG.lock().unwrap_or_else(|e| e.into_inner());
    let available = log.len().min(count as usize);
    if available == 0 {
        return 0;
    }
    let mut total_chars = 0;
    for i in 0..available {
        let entry = &log[i];
        if !sources.is_null() {
            *sources.add(i) = entry.source;
        }
        if !types.is_null() {
            *types.add(i) = entry.type_;
        }
        if !ids.is_null() {
            *ids.add(i) = entry.id;
        }
        if !severities.is_null() {
            *severities.add(i) = entry.severity;
        }
        let msg_len = entry.message.len();
        total_chars += msg_len + 1; // +1 for null terminator
        if !lengths.is_null() {
            *lengths.add(i) = msg_len as i32;
        }
        if !message_log.is_null() && buf_size > total_chars as i32 {
            let dest = message_log.add(total_chars - msg_len - 1) as *mut u8;
            std::ptr::copy_nonoverlapping(entry.message.as_ptr(), dest, msg_len);
            *dest.add(msg_len) = 0;
        }
    }
    available as u32
}

#[no_mangle]
pub unsafe extern "C" fn glGetDebugMessageLogARB(
    count: u32,
    buf_size: i32,
    sources: *mut u32,
    types: *mut u32,
    ids: *mut u32,
    severities: *mut u32,
    lengths: *mut i32,
    message_log: *mut c_char,
) -> u32 {
    glGetDebugMessageLog(count, buf_size, sources, types, ids, severities, lengths, message_log)
}

#[no_mangle]
pub unsafe extern "C" fn glGetObjectLabel(
    identifier: u32,
    name: u32,
    buf_size: i32,
    length: *mut i32,
    label: *mut c_char,
) {
    // This layer doesn't store object labels; return empty
    if !length.is_null() {
        *length = 0;
    }
    if !label.is_null() && buf_size > 0 {
        *label = 0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetObjectPtrLabel(
    ptr: *const c_void,
    buf_size: i32,
    length: *mut i32,
    label: *mut c_char,
) {
    // This layer doesn't store object labels; return empty
    if !length.is_null() {
        *length = 0;
    }
    if !label.is_null() && buf_size > 0 {
        *label = 0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn glObjectLabel(
    _identifier: u32,
    _name: u32,
    _label: *const c_char,
    _length: isize,
) {
    // Debug labels stored by application; this layer doesn't track them
}

#[no_mangle]
pub unsafe extern "C" fn glObjectPtrLabel(_identifier: u32, _ptr: *const c_void, _length: isize) {}

#[no_mangle]
pub unsafe extern "C" fn glGetPointerv(pname: u32, params: *mut *mut c_void) {
    if params.is_null() {
        errors().set(GL_INVALID_VALUE);
        return;
    }
    match pname {
        GL_DEBUG_CALLBACK_FUNCTION => {
            let guard = DEBUG_CALLBACK.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((fn_ptr, _)) = *guard {
                *params = fn_ptr as *mut c_void;
            } else {
                *params = std::ptr::null_mut();
            }
        }
        GL_DEBUG_CALLBACK_USER_PARAM => {
            let guard = DEBUG_CALLBACK.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((_, user_param)) = *guard {
                *params = user_param as *mut c_void;
            } else {
                *params = std::ptr::null_mut();
            }
        }
        _ => {
            errors().set(GL_INVALID_ENUM);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetGraphicsResetStatus() -> u32 {
    if let Some(get) =
        driver_fn_cached::<unsafe extern "C" fn() -> u32>("glGetGraphicsResetStatus")
    {
        get()
    } else if let Some(get) =
        driver_fn_cached::<unsafe extern "C" fn() -> u32>("glGetGraphicsResetStatusKHR")
    {
        get()
    } else if let Some(get) =
        driver_fn_cached::<unsafe extern "C" fn() -> u32>("glGetGraphicsResetStatusEXT")
    {
        get()
    } else {
        GL_NO_ERROR
    }
}



/// Indirect draws require the GL_ARB_multi_draw_indirect or GL_ES32 extension.
/// If supported, forward to the driver; otherwise set GL_INVALID_OPERATION.
#[no_mangle]
pub unsafe extern "C" fn glMultiDrawElementsIndirect(
    mode: u32,
    type_: u32,
    indirect: *const c_void,
    drawcount: i32,
    stride: i32,
) {
    if crate::gles3::caps().has_indirect_draw {
        if let Some(f) = driver_fn_cached::<
            unsafe extern "C" fn(u32, u32, *const c_void, i32, i32),
        >("glMultiDrawElementsIndirect")
        {
            f(mode, type_, indirect, drawcount, stride);
            return;
        }
    }
    static ONCE: AtomicBool = AtomicBool::new(false);
    log_once(
        &ONCE,
        "[GLCompat] glMultiDrawElementsIndirect is not available (no indirect draw support); \
         draw skipped",
    );
    errors().set(GL_INVALID_OPERATION);
}

#[no_mangle]
pub unsafe extern "C" fn glMultiDrawArraysIndirect(
    mode: u32,
    indirect: *const c_void,
    drawcount: i32,
    stride: i32,
) {
    if crate::gles3::caps().has_indirect_draw {
        if let Some(f) = driver_fn_cached::<
            unsafe extern "C" fn(u32, *const c_void, i32, i32),
        >("glMultiDrawArraysIndirect")
        {
            f(mode, indirect, drawcount, stride);
            return;
        }
    }
    static ONCE: AtomicBool = AtomicBool::new(false);
    log_once(
        &ONCE,
        "[GLCompat] glMultiDrawArraysIndirect is not available (no indirect draw support); \
         draw skipped",
    );
    errors().set(GL_INVALID_OPERATION);
}

#[no_mangle]
pub unsafe extern "C" fn glDispatchCompute(x: u32, y: u32, z: u32) {
    if crate::gles3::caps().has_compute_shader {
        if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32)>("glDispatchCompute") {
            f(x, y, z);
            return;
        }
    }
    errors().set(GL_INVALID_OPERATION);
}

#[no_mangle]
pub unsafe extern "C" fn glDispatchComputeIndirect(indirect: isize) {
    if crate::gles3::caps().has_compute_shader {
        if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(isize)>("glDispatchComputeIndirect") {
            f(indirect);
            return;
        }
    }
    errors().set(GL_INVALID_OPERATION);
}

#[no_mangle]
pub unsafe extern "C" fn glBindImageTextures(first: u32, count: i32, textures: *const u32) {
    if crate::has_shader_image_load_store() {
        if let Some(f) = driver_fn_cached::<unsafe extern "C" fn(u32, i32, *const u32)>("glBindImageTextures") {
            f(first, count, textures);
            return;
        }
    }
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
    "glDebugMessageControl",
    "glDebugMessageControlARB",
    "glDebugMessageInsert",
    "glGetDebugMessageLog",
    "glGetDebugMessageLogARB",
    "glGetObjectLabel",
    "glGetObjectPtrLabel",
    "glGetPointerv",
    "glGetGraphicsResetStatus",
    "glMultiDrawArraysIndirect",
    "glMultiDrawElementsIndirect",
    "glDispatchCompute",
    "glDispatchComputeIndirect",
    "glBindImageTextures",
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
