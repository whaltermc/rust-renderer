//! Fixed-function draw emulation for GLES 3.
//!
//! When the game draws with no shader program bound (Minecraft 1.16 and older), we render
//! the tracked client arrays with a small built-in program: MVP transform, per-vertex or
//! constant color, optional texture on unit 0, and the alpha test. `GL_QUADS` is expanded
//! to triangles. NOT emulated yet: lighting, fog, texture matrices, texture environment
//! modes. Fog, the lightmap (texture unit 1 on 1.12-1.14, unit 2 on 1.15/1.16) and texture
//! matrices are handled by the extended program; if that program fails to compile the base
//! program (no fog, lightmap or texture matrices) is used instead, so a typo in the extended
//! shader costs those features rather than the whole fixed-function path.
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

const VS_BASE: &str = "#version 300 es
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

const FS_BASE: &str = "#version 300 es
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

const VS_EXT: &str = "#version 300 es
layout(location = 0) in vec4 aPos;
layout(location = 1) in vec4 aColor;
layout(location = 2) in vec2 aUV;
layout(location = 3) in vec2 aLightUV;
uniform mat4 uMvp;
uniform mat4 uMv;
uniform mat4 uTexMat0;
uniform mat4 uTexMatLight;
out vec4 vColor;
out vec2 vUV;
out vec2 vLightUV;
out float vFogDist;
void main() {
    gl_Position = uMvp * aPos;
    vColor = aColor;
    vUV = (uTexMat0 * vec4(aUV, 0.0, 1.0)).xy;
    vLightUV = (uTexMatLight * vec4(aLightUV, 0.0, 1.0)).xy;
    vFogDist = abs((uMv * aPos).z);
}
";

const FS_EXT: &str = "#version 300 es
precision highp float;
in vec4 vColor;
in vec2 vUV;
in vec2 vLightUV;
in float vFogDist;
uniform sampler2D uTex;
uniform sampler2D uTexLight;
uniform int uUseTex;
uniform int uUseLight;
uniform int uAlphaFunc;
uniform float uAlphaRef;
uniform int uFogMode;
uniform vec4 uFogColor;
uniform vec4 uFogParams;
out vec4 oColor;
void main() {
    vec4 c = vColor;
    if (uUseTex != 0) c *= texture(uTex, vUV);
    if (uUseLight != 0) c.rgb *= texture(uTexLight, vLightUV).rgb;
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
    if (uFogMode != 0) {
        float k = 1.0;
        if (uFogMode == 1) {
            k = (uFogParams.y - vFogDist) / max(uFogParams.y - uFogParams.x, 0.0001);
        } else if (uFogMode == 2) {
            k = exp(-uFogParams.z * vFogDist);
        } else {
            float d = uFogParams.z * vFogDist;
            k = exp(-d * d);
        }
        c.rgb = mix(uFogColor.rgb, c.rgb, clamp(k, 0.0, 1.0));
    }
    oColor = c;
}
";

struct Gpu {
    prog: ProgramId,
    vao: VertexArrayId,
    vbo: [BufferId; 4],
    ebo: BufferId,
    /// True for the extended program (fog, lightmap, texture matrices).
    extended: bool,
    loc_mv: i32,
    loc_tm0: i32,
    loc_tm_light: i32,
    loc_tex_light: i32,
    loc_use_light: i32,
    loc_fog_mode: i32,
    loc_fog_color: i32,
    loc_fog_params: i32,
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

/// Compiles and links one fixed-function program, logging (not panicking) on failure.
fn make_program(be: &dyn Backend, vs_src: &str, fs_src: &str, tag: &str) -> Option<ProgramId> {
    let vs = be
        .compile_shader(GL_VERTEX_SHADER, vs_src)
        .map_err(|e| crate::log(&format!("[FFDraw] {tag} vertex shader: {e}")))
        .ok()?;
    let fs = match be.compile_shader(GL_FRAGMENT_SHADER, fs_src) {
        Ok(f) => f,
        Err(e) => {
            crate::log(&format!("[FFDraw] {tag} fragment shader: {e}"));
            be.delete_shader(vs);
            return None;
        }
    };
    let prog = match be.link_program(&[vs, fs]) {
        Ok(p) => p,
        Err(e) => {
            crate::log(&format!("[FFDraw] {tag} link: {e}"));
            be.delete_shader(vs);
            be.delete_shader(fs);
            return None;
        }
    };
    be.delete_shader(vs);
    be.delete_shader(fs);
    Some(prog)
}

fn build(be: &dyn Backend) -> Option<Gpu> {
    let (prog, extended) = match make_program(be, VS_EXT, FS_EXT, "extended") {
        Some(p) => (p, true),
        None => {
            crate::log("[FFDraw] extended program failed; falling back to the base program (no fog, lightmap or texture matrices)");
            (make_program(be, VS_BASE, FS_BASE, "base")?, false)
        }
    };
    let vao = be.create_vertex_array().ok()?;
    let vbo = [
        be.create_buffer().ok()?,
        be.create_buffer().ok()?,
        be.create_buffer().ok()?,
        be.create_buffer().ok()?,
    ];
    let ebo = be.create_buffer().ok()?;
    let loc = |n: &str| be.uniform_location(prog, n).unwrap_or(-1);
    crate::log(&format!(
        "[FFDraw] fixed-function emulation program ready ({})",
        if extended { "extended: fog, lightmap, texture matrices" } else { "base" }
    ));
    Some(Gpu {
        prog,
        vao,
        vbo,
        ebo,
        extended,
        loc_mv: loc("uMv"),
        loc_tm0: loc("uTexMat0"),
        loc_tm_light: loc("uTexMatLight"),
        loc_tex_light: loc("uTexLight"),
        loc_use_light: loc("uUseLight"),
        loc_fog_mode: loc("uFogMode"),
        loc_fog_color: loc("uFogColor"),
        loc_fog_params: loc("uFogParams"),
        loc_mvp: loc("uMvp"),
        loc_tex: loc("uTex"),
        loc_use_tex: loc("uUseTex"),
        loc_afunc: loc("uAlphaFunc"),
        loc_aref: loc("uAlphaRef"),
    })
}

/// Picks the unit that carries the lightmap: unit 2 when it has both a UV array and texturing
/// enabled (1.15/1.16, where unit 1 is the entity overlay), otherwise unit 1 (1.12-1.14).
fn lightmap_unit(arrays: impl Fn(usize) -> ArraySnap, enabled: impl Fn(usize) -> bool) -> Option<usize> {
    [2usize, 1usize]
        .into_iter()
        .find(|&u| arrays(u).enabled && enabled(u))
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

    // Lightmap: a second UV array on unit 1 or 2 plus a texture enabled on that unit.
    let mut light_unit: Option<usize> = None;
    if gpu.extended {
        light_unit = lightmap_unit(fixed_func::texcoord_unit, fixed_func::unit_texture_enabled);
        if let Some(u) = light_unit {
            let a = fixed_func::texcoord_unit(u);
            if !setup_attrib(be, 3, &a, gpu.vbo[3], first, count, false) {
                light_unit = None;
            }
        }
        if light_unit.is_none() {
            be.set_vertex_attrib_enabled(3, false);
            if let Some(f) = attrib4f {
                f(3, 0.0, 0.0, 0.0, 1.0);
            }
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
    if gpu.extended {
        be.uniform_matrix_4(gpu.loc_mv, &fixed_func::modelview_matrix(), false);
        be.uniform_matrix_4(gpu.loc_tm0, &fixed_func::texture_matrix(0), false);
        match light_unit {
            Some(u) => {
                be.uniform_matrix_4(gpu.loc_tm_light, &fixed_func::texture_matrix(u), false);
                be.uniform_1i(gpu.loc_tex_light, u as i32);
                be.uniform_1i(gpu.loc_use_light, 1);
            }
            None => {
                be.uniform_matrix_4(gpu.loc_tm_light, &fixed_func::texture_matrix(0), false);
                be.uniform_1i(gpu.loc_tex_light, 1);
                be.uniform_1i(gpu.loc_use_light, 0);
            }
        }
        match fixed_func::fog() {
            Some(f) => {
                be.uniform_1i(gpu.loc_fog_mode, f.mode);
                be.uniform_4f(gpu.loc_fog_color, f.color[0], f.color[1], f.color[2], f.color[3]);
                be.uniform_4f(gpu.loc_fog_params, f.start, f.end, f.density, 0.0);
            }
            None => be.uniform_1i(gpu.loc_fog_mode, 0),
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn arr(enabled: bool) -> ArraySnap {
        ArraySnap { enabled, ..ArraySnap::default() }
    }

    #[test]
    fn lightmap_prefers_unit_two_then_one() {
        // 1.16: overlay on unit 1, lightmap on unit 2.
        assert_eq!(lightmap_unit(|_| arr(true), |_| true), Some(2));
        // 1.12-1.14: lightmap on unit 1 only.
        assert_eq!(lightmap_unit(|u| arr(u == 1), |u| u == 1), Some(1));
        // A UV array without an enabled texture is not a lightmap.
        assert_eq!(lightmap_unit(|_| arr(true), |_| false), None);
        assert_eq!(lightmap_unit(|_| arr(false), |_| true), None);
    }

    #[test]
    fn extended_shaders_declare_every_uniform_the_draw_sets() {
        for u in ["uMvp", "uMv", "uTexMat0", "uTexMatLight"] {
            assert!(VS_EXT.contains(u), "{u}");
        }
        for u in ["uTex", "uTexLight", "uUseTex", "uUseLight", "uAlphaFunc", "uAlphaRef",
                  "uFogMode", "uFogColor", "uFogParams"] {
            assert!(FS_EXT.contains(u), "{u}");
        }
        // The attribute locations the draw path binds.
        assert!(VS_EXT.contains("location = 3) in vec2 aLightUV"));
    }
}
