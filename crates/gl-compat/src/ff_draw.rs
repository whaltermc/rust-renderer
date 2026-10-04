//! Fixed-function draw emulation for GLES 3.
//!
//! When the game draws with no shader program bound (Minecraft 1.16 and older), we render
//! the tracked client arrays with a small built-in program: MVP transform, per-vertex or
//! constant color, optional texture on unit 0, and the alpha test. `GL_QUADS` is expanded
//! to triangles. NOT emulated yet: lighting, fog, texture matrices, texture environment
//! modes, a second texture unit.
//!
//! Draws made while a program is bound (1.17+) are never touched.

use crate::fixed_func::{self, ArraySnap};
use renderer_core::{Backend, BufferId, ProgramId, VertexArrayId};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const GL_ARRAY_BUFFER: u32 = 0x8892;
const GL_ELEMENT_ARRAY_BUFFER: u32 = 0x8893;
const GL_STREAM_DRAW: u32 = 0x88E0;
const GL_CURRENT_PROGRAM: u32 = 0x8B8D;
const GL_VERTEX_ARRAY_BINDING: u32 = 0x85B5;
const GL_ARRAY_BUFFER_BINDING: u32 = 0x8894;
const GL_VERTEX_SHADER: u32 = 0x8B31;
const GL_FRAGMENT_SHADER: u32 = 0x8B30;
const GL_QUADS: u32 = 0x0007;
const GL_TRIANGLES: u32 = 0x0004;
const GL_UNSIGNED_INT: u32 = 0x1405;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_ALPHA_TEST: u32 = 0x0BC0;

const VS: &str = "#version 300 es
layout(location = 0) in vec4 aPos;
layout(location = 1) in vec4 aColor;
layout(location = 2) in vec2 aUV;
uniform mat4 uMvp;
out vec4 vColor;
out vec2 vUV;
void main() {
    gl_Position = uMvp * aPos;
    vColor = aColor;
    vUV = aUV;
}
";

const FS: &str = "#version 300 es
precision highp float;
in vec4 vColor;
in vec2 vUV;
uniform sampler2D uTex;
uniform int uUseTex;
uniform int uAlphaFunc;
uniform float uAlphaRef;
out vec4 oColor;
void main() {
    vec4 c = vColor;
    if (uUseTex != 0) c *= texture(uTex, vUV);
    if (uAlphaFunc != 0) {
        int f = uAlphaFunc - 1;
        bool ok = true;
        if (f == 0) ok = false;
        else if (f == 1) ok = c.a < uAlphaRef;
        else if (f == 2) ok = c.a == uAlphaRef;
        else if (f == 3) ok = c.a <= uAlphaRef;
        else if (f == 4) ok = c.a > uAlphaRef;
        else if (f == 5) ok = c.a != uAlphaRef;
        else if (f == 6) ok = c.a >= uAlphaRef;
        if (!ok) discard;
    }
    oColor = c;
}
";

struct Gpu {
    prog: ProgramId,
    vao: VertexArrayId,
    vbo: [BufferId; 3],
    ebo: BufferId,
    loc_mvp: i32,
    loc_tex: i32,
    loc_use_tex: i32,
    loc_afunc: i32,
    loc_aref: i32,
}

static GPU: Mutex<Option<Gpu>> = Mutex::new(None);
static DRAWS: AtomicU64 = AtomicU64::new(0);
static DIAG: AtomicU64 = AtomicU64::new(0);
static SUSPECT: AtomicU64 = AtomicU64::new(0);
static ERRS: AtomicU64 = AtomicU64::new(0);

fn build(be: &dyn Backend) -> Option<Gpu> {
    let vs = be
        .compile_shader(GL_VERTEX_SHADER, VS)
        .map_err(|e| crate::log(&format!("[FFDraw] vertex shader: {e}")))
        .ok()?;
    let fs = be
        .compile_shader(GL_FRAGMENT_SHADER, FS)
        .map_err(|e| crate::log(&format!("[FFDraw] fragment shader: {e}")))
        .ok()?;
    let prog = be
        .link_program(&[vs, fs])
        .map_err(|e| crate::log(&format!("[FFDraw] link: {e}")))
        .ok()?;
    be.delete_shader(vs);
    be.delete_shader(fs);
    let vao = be.create_vertex_array().ok()?;
    let vbo = [be.create_buffer().ok()?, be.create_buffer().ok()?, be.create_buffer().ok()?];
    let ebo = be.create_buffer().ok()?;
    let loc = |n: &str| be.uniform_location(prog, n).unwrap_or(-1);
    crate::log("[FFDraw] fixed-function emulation program ready");
    Some(Gpu {
        prog,
        vao,
        vbo,
        ebo,
        loc_mvp: loc("uMvp"),
        loc_tex: loc("uTex"),
        loc_use_tex: loc("uUseTex"),
        loc_afunc: loc("uAlphaFunc"),
        loc_aref: loc("uAlphaRef"),
    })
}

fn type_size(ty: u32) -> usize {
    match ty {
        0x1400 | 0x1401 => 1, // BYTE, UNSIGNED_BYTE
        0x1402 | 0x1403 => 2, // SHORT, UNSIGNED_SHORT
        _ => 4,               // INT, UNSIGNED_INT, FLOAT
    }
}

/// Points attribute `idx` at array `a`, rebased so vertex `first` becomes vertex 0.
/// Returns false if the array is unusable (caller then uses a constant value).
fn setup_attrib(be: &dyn Backend, idx: u32, a: &ArraySnap, vbo: BufferId, first: i32, count: i32, normalized: bool) -> bool {
    if !a.enabled || a.size <= 0 || a.size > 4 {
        return false;
    }
    let elem = a.size as usize * type_size(a.ty);
    let stride = if a.stride > 0 { a.stride as usize } else { elem };
    let skip = first as usize * stride;
    if a.buffer != 0 {
        be.bind_buffer(GL_ARRAY_BUFFER, Some(BufferId(a.buffer)));
        be.vertex_attrib_pointer(idx, a.size, a.ty, normalized, a.stride, a.ptr + skip);
    } else {
        if a.ptr == 0 {
            return false;
        }
        let len = (count as usize - 1) * stride + elem;
        // SAFETY: client-array memory is owned by the game and must stay valid until the
        // draw call returns (GL contract); we copy exactly the bytes the draw will read.
        let src = unsafe { std::slice::from_raw_parts((a.ptr + skip) as *const u8, len) };
        be.bind_buffer(GL_ARRAY_BUFFER, Some(vbo));
        if be.buffer_data(GL_ARRAY_BUFFER, src, GL_STREAM_DRAW).is_err() {
            return false;
        }
        be.vertex_attrib_pointer(idx, a.size, a.ty, normalized, a.stride, 0);
    }
    be.set_vertex_attrib_enabled(idx, true);
    true
}

fn restore(be: &dyn Backend, prev_vao: i32, prev_abuf: i32) {
    be.use_program(None);
    be.bind_vertex_array(if prev_vao > 0 { Some(VertexArrayId(prev_vao as u32)) } else { None });
    be.bind_buffer(GL_ARRAY_BUFFER, if prev_abuf > 0 { Some(BufferId(prev_abuf as u32)) } else { None });
}

/// Returns true if the draw was handled here.
///
/// # Safety
/// Reads raw client-array memory recorded by `glVertexPointer` and friends.
pub unsafe fn try_draw_arrays(mode: u32, first: i32, count: i32) -> bool {
    if count <= 0 || first < 0 {
        return false;
    }
    let be = match crate::backend() {
        Some(b) => b,
        None => return false,
    };
    type GetInt = unsafe extern "C" fn(u32, *mut i32);
    let get_int = match crate::driver_fn_cached::<GetInt>("glGetIntegerv") {
        Some(f) => f,
        None => return false,
    };
    let mut cur = 0i32;
    get_int(GL_CURRENT_PROGRAM, &mut cur);
    if cur != 0 {
        return false; // the game uses its own shaders
    }
    let (pos, col, uv) = fixed_func::arrays();
    if !pos.enabled {
        return false;
    }

    let mut guard = GPU.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = build(be);
    }
    let gpu = match guard.as_ref() {
        Some(g) => g,
        None => return false,
    };

    let n = DRAWS.fetch_add(1, Ordering::Relaxed);
    if (n == 0 || n % 10000 == 0) && std::env::var("RENDERER_DEBUG").map(|v| v == "1").unwrap_or(false) {
        crate::log(&format!(
            "[FFDraw] draw #{n}: mode=0x{mode:04X} first={first} count={count} pos(size={} ty=0x{:04X} stride={} vbo={}) color={} uv={}",
            pos.size, pos.ty, pos.stride, pos.buffer, col.enabled, uv.enabled
        ));
    }

    let mut prev_vao = 0i32;
    let mut prev_abuf = 0i32;
    get_int(GL_VERTEX_ARRAY_BINDING, &mut prev_vao);
    get_int(GL_ARRAY_BUFFER_BINDING, &mut prev_abuf);

    be.bind_vertex_array(Some(gpu.vao));
    if !setup_attrib(be, 0, &pos, gpu.vbo[0], first, count, false) {
        restore(be, prev_vao, prev_abuf);
        return false;
    }
    type Attrib4f = unsafe extern "C" fn(u32, f32, f32, f32, f32);
    let attrib4f = crate::driver_fn_cached::<Attrib4f>("glVertexAttrib4f");

    let has_color = setup_attrib(be, 1, &col, gpu.vbo[1], first, count, true);
    if !has_color {
        be.set_vertex_attrib_enabled(1, false);
        if let Some(f) = attrib4f {
            let c = fixed_func::current_color();
            f(1, c[0], c[1], c[2], c[3]);
        }
    }
    let has_uv = setup_attrib(be, 2, &uv, gpu.vbo[2], first, count, false);
    if !has_uv {
        be.set_vertex_attrib_enabled(2, false);
        if let Some(f) = attrib4f {
            f(2, 0.0, 0.0, 0.0, 1.0);
        }
    }

    // Prefer real texture binding over legacy glEnable(GL_TEXTURE_2D).
    // 1.16 often binds a 2D texture without the fixed-function enable bit.
    const GL_TEXTURE_BINDING_2D: u32 = 0x8069;
    let mut tex_binding = 0i32;
    get_int(GL_TEXTURE_BINDING_2D, &mut tex_binding);
    let use_tex = has_uv
        && (fixed_func::legacy_cap_enabled(GL_TEXTURE_2D) || tex_binding != 0);
    let (afunc, aref) = fixed_func::alpha();
    let alpha_mode = if fixed_func::legacy_cap_enabled(GL_ALPHA_TEST) && (0x200..=0x207).contains(&afunc) {
        (afunc - 0x200 + 1) as i32
    } else {
        0
    };

    let d = DIAG.fetch_add(1, Ordering::Relaxed);
    let suspect = !use_tex && !has_color;
    if std::env::var("RENDERER_DEBUG").map(|v| v == "1").unwrap_or(false)
        && (d < 20 || (suspect && SUSPECT.fetch_add(1, Ordering::Relaxed) < 10))
    {
        let c = fixed_func::current_color();
        crate::log(&format!(
            "[FFDraw] diag #{d}: mode=0x{mode:04X} count={count} tex={use_tex} texbind={tex_binding} color_array={has_color} uv_array={has_uv} alpha_mode={alpha_mode} cur_color=({:.2},{:.2},{:.2},{:.2})",
            c[0], c[1], c[2], c[3]
        ));
    }

    // Save program so we do not leave the FF shader bound for the game's next draw.
    let mut prev_prog = 0i32;
    get_int(GL_CURRENT_PROGRAM, &mut prev_prog);

    be.use_program(Some(gpu.prog));
    be.uniform_matrix_4(gpu.loc_mvp, &fixed_func::mvp_matrix(), false);
    be.uniform_1i(gpu.loc_tex, 0);
    be.uniform_1i(gpu.loc_use_tex, use_tex as i32);
    be.uniform_1i(gpu.loc_afunc, alpha_mode);
    be.uniform_1f(gpu.loc_aref, aref);

    if mode == GL_QUADS {
        let quads = (count / 4) as usize;
        let mut idx: Vec<u32> = Vec::with_capacity(quads * 6);
        for q in 0..quads {
            let b = (q * 4) as u32;
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        // SAFETY: reinterpreting a live Vec<u32> as bytes (same allocation, len * 4).
        let bytes = std::slice::from_raw_parts(idx.as_ptr() as *const u8, idx.len() * 4);
        be.bind_buffer(GL_ELEMENT_ARRAY_BUFFER, Some(gpu.ebo));
        if be.buffer_data(GL_ELEMENT_ARRAY_BUFFER, bytes, GL_STREAM_DRAW).is_ok() {
            be.draw_elements(GL_TRIANGLES, idx.len() as i32, GL_UNSIGNED_INT, 0);
        }
    } else {
        be.draw_arrays(mode, 0, count);
    }

    let err = be.get_error();
    if err != 0 && ERRS.fetch_add(1, Ordering::Relaxed) < 20 {
        crate::log(&format!("[FFDraw] GL error 0x{err:04X} after draw mode=0x{mode:04X} count={count}"));
    }

    restore(be, prev_vao, prev_abuf);
    // Restore the game's program (0 = fixed-function / none).
    if prev_prog != 0 {
        be.use_program(Some(ProgramId(prev_prog as u32)));
    } else {
        be.use_program(None);
    }
    true
}
