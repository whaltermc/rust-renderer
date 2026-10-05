//! Immediate mode (`glBegin` / `glVertex*` / `glEnd`) on top of the fixed-function emulation.
//!
//! OpenGL ES has no immediate mode. `aliases.rs` used to forward these names to the driver,
//! which does not export them, so every call was dropped (after a single log line) and
//! anything the game drew this way was simply missing. Here the vertices are collected on the
//! CPU and `glEnd` draws them through the same path as client-array draws
//! ([`crate::fixed_draw::try_draw_arrays`]), so quads, texturing, the alpha test, fog and the
//! matrix stack all behave the same way.
//!
//! Limits: a draw made while a shader program is bound is not emulated (the same limit as
//! every other fixed-function draw), and per-vertex normals are collected but unused because
//! the emulation has no lighting yet.

use crate::fixed_func;
use std::sync::Mutex;

const GL_QUAD_STRIP: u32 = 0x0008;
const GL_POLYGON: u32 = 0x0009;
const GL_TRIANGLE_STRIP: u32 = 0x0005;
const GL_TRIANGLE_FAN: u32 = 0x0006;
const GL_TEXTURE0: u32 = 0x84C0;

#[derive(Default)]
struct ImmediateState {
    active: bool,
    mode: u32,
    /// Current texture coordinate (`glTexCoord*`), applied to each following vertex.
    uv: [f32; 2],
    /// Set once the application supplies any texture coordinate, so a vertex stream that never
    /// does is drawn without a UV array instead of sampling texel (0, 0).
    uv_used: bool,
    pos: Vec<f32>, // x, y, z, w per vertex
    col: Vec<f32>, // r, g, b, a per vertex
    uvs: Vec<f32>, // s, t per vertex
}

static IMM: Mutex<Option<ImmediateState>> = Mutex::new(None);

fn with_imm<R>(f: impl FnOnce(&mut ImmediateState) -> R) -> R {
    let mut g = IMM.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(ImmediateState::default))
}

/// ES has no `GL_QUAD_STRIP` or `GL_POLYGON`; both have an exact triangle equivalent.
fn es_mode(mode: u32) -> u32 {
    match mode {
        GL_QUAD_STRIP => GL_TRIANGLE_STRIP,
        GL_POLYGON => GL_TRIANGLE_FAN,
        m => m,
    }
}

/// Vertices a draw of `mode` needs to be well formed; trailing extras are dropped.
fn usable_vertex_count(mode: u32, n: usize) -> usize {
    match mode {
        0x0007 => n - n % 4,                  // GL_QUADS
        0x0004 => n - n % 3,                  // GL_TRIANGLES
        0x0001 => n - n % 2,                  // GL_LINES
        0x0002 | 0x0003 => if n >= 2 { n } else { 0 }, // LINE_LOOP, LINE_STRIP
        GL_TRIANGLE_STRIP | GL_TRIANGLE_FAN | GL_POLYGON => if n >= 3 { n } else { 0 },
        GL_QUAD_STRIP => if n >= 4 { n - n % 2 } else { 0 },
        _ => n, // GL_POINTS
    }
}

pub unsafe extern "C" fn begin(mode: u32) {
    if !crate::gl::v1_1::record_command(crate::gl::v1_1::ListCommand::Begin(mode)) {
        return;
    }
    with_imm(|i| {
        i.active = true;
        i.mode = mode;
        i.pos.clear();
        i.col.clear();
        i.uvs.clear();
        // The texture coordinate persists across Begin/End in GL, but `uv_used` only describes
        // this primitive.
        i.uv_used = false;
    });
}

pub unsafe extern "C" fn end() {
    if !crate::gl::v1_1::record_command(crate::gl::v1_1::ListCommand::End) {
        return;
    }
    let batch = with_imm(|i| {
        if !i.active {
            return None;
        }
        i.active = false;
        Some((
            i.mode,
            std::mem::take(&mut i.pos),
            std::mem::take(&mut i.col),
            std::mem::take(&mut i.uvs),
            i.uv_used,
        ))
    });
    let Some((mode, pos, col, uvs, uv_used)) = batch else { return };
    let n = usable_vertex_count(mode, pos.len() / 4);
    if n == 0 {
        return;
    }
    let uv_ptr = if uv_used { Some(uvs.as_ptr()) } else { None };
    let saved = fixed_func::push_arrays(pos.as_ptr(), col.as_ptr(), uv_ptr);
    // SAFETY: `pos`, `col` and `uvs` outlive the draw; `n` never exceeds what they hold.
    let handled = crate::fixed_draw::try_draw_arrays(es_mode(mode), 0, n as i32);
    fixed_func::pop_arrays(saved);
    if !handled {
        crate::log("[Immediate] glEnd: draw not emulated (a shader program is bound or the FF path is unavailable)");
    }
}

fn push_vertex(x: f32, y: f32, z: f32, w: f32) {
    if !crate::gl::v1_1::record_command(crate::gl::v1_1::ListCommand::Vertex([x, y, z, w])) {
        return;
    }
    let color = fixed_func::current_color();
    with_imm(|i| {
        if !i.active {
            return;
        }
        i.pos.extend_from_slice(&[x, y, z, w]);
        i.col.extend_from_slice(&color);
        let uv = i.uv;
        i.uvs.extend_from_slice(&uv);
    });
}

fn set_uv(s: f32, t: f32) {
    if !crate::gl::v1_1::record_command(crate::gl::v1_1::ListCommand::TexCoord([s, t])) {
        return;
    }
    with_imm(|i| {
        i.uv = [s, t];
        i.uv_used = true;
    });
}

pub(crate) fn set_color(r: f32, g: f32, b: f32, a: f32) {
    if crate::gl::v1_1::record_command(crate::gl::v1_1::ListCommand::Color([r, g, b, a])) {
        fixed_func::gl_color4f(r, g, b, a);
    }
}

pub(crate) fn replay_list_command(command: crate::gl::v1_1::ListCommand) {
    use crate::gl::v1_1::ListCommand;
    match command {
        ListCommand::Begin(mode) => unsafe { begin(mode) },
        ListCommand::End => unsafe { end() },
        ListCommand::Vertex([x, y, z, w]) => push_vertex(x, y, z, w),
        ListCommand::Color([r, g, b, a]) => set_color(r, g, b, a),
        ListCommand::TexCoord([s, t]) => set_uv(s, t),
        ListCommand::CallList(_) => unreachable!("nested lists are replayed by the list manager"),
    }
}

// ---- colour conversion: GL maps the full integer range onto [0, 1] -----------------------

fn from_u8(v: u8) -> f32 { v as f32 / 255.0 }
fn from_i8(v: i8) -> f32 { (2.0 * v as f32 + 1.0) / 255.0 }
fn from_u16(v: u16) -> f32 { v as f32 / 65535.0 }
fn from_i16(v: i16) -> f32 { (2.0 * v as f32 + 1.0) / 65535.0 }
fn from_u32(v: u32) -> f32 { (v as f64 / 4294967295.0) as f32 }
fn from_i32(v: i32) -> f32 { ((2.0 * v as f64 + 1.0) / 4294967295.0) as f32 }

macro_rules! export {
    ($($name:ident($($a:ident : $t:ty),*) $body:block)*) => {
        $(
            #[no_mangle]
            pub unsafe extern "C" fn $name($($a: $t),*) $body
        )*
    };
}

export! {
    glBegin(mode: u32) { begin(mode) }
    glEnd() { end() }

    glVertex2f(x: f32, y: f32) { push_vertex(x, y, 0.0, 1.0) }
    glVertex3f(x: f32, y: f32, z: f32) { push_vertex(x, y, z, 1.0) }
    glVertex4f(x: f32, y: f32, z: f32, w: f32) { push_vertex(x, y, z, w) }
    glVertex2d(x: f64, y: f64) { push_vertex(x as f32, y as f32, 0.0, 1.0) }
    glVertex3d(x: f64, y: f64, z: f64) { push_vertex(x as f32, y as f32, z as f32, 1.0) }
    glVertex4d(x: f64, y: f64, z: f64, w: f64) { push_vertex(x as f32, y as f32, z as f32, w as f32) }
    glVertex2i(x: i32, y: i32) { push_vertex(x as f32, y as f32, 0.0, 1.0) }
    glVertex3i(x: i32, y: i32, z: i32) { push_vertex(x as f32, y as f32, z as f32, 1.0) }
    glVertex2fv(v: *const f32) { if !v.is_null() { push_vertex(*v, *v.add(1), 0.0, 1.0) } }
    glVertex3fv(v: *const f32) { if !v.is_null() { push_vertex(*v, *v.add(1), *v.add(2), 1.0) } }
    glVertex4fv(v: *const f32) { if !v.is_null() { push_vertex(*v, *v.add(1), *v.add(2), *v.add(3)) } }

    glTexCoord1f(s: f32) { set_uv(s, 0.0) }
    glTexCoord2f(s: f32, t: f32) { set_uv(s, t) }
    glTexCoord2d(s: f64, t: f64) { set_uv(s as f32, t as f32) }
    glTexCoord2fv(v: *const f32) { if !v.is_null() { set_uv(*v, *v.add(1)) } }
    // Only s and t are used; r and q are dropped (the emulation has 2D textures only).
    glTexCoord3f(s: f32, t: f32, _r: f32) { set_uv(s, t) }
    glTexCoord4f(s: f32, t: f32, _r: f32, _q: f32) { set_uv(s, t) }
    // Only unit 0 has a current coordinate; other units are ignored rather than clobbering it.
    glMultiTexCoord2f(target: u32, s: f32, t: f32) { if target == GL_TEXTURE0 { set_uv(s, t) } }

    // Normals are accepted so callers keep running; there is no lighting to use them.
    glNormal3f(_x: f32, _y: f32, _z: f32) {}
    glNormal3fv(_v: *const f32) {}

    glColor3b(r: i8, g: i8, b: i8) { set_color(from_i8(r), from_i8(g), from_i8(b), 1.0) }
    glColor4b(r: i8, g: i8, b: i8, a: i8) { set_color(from_i8(r), from_i8(g), from_i8(b), from_i8(a)) }
    glColor3ub(r: u8, g: u8, b: u8) { set_color(from_u8(r), from_u8(g), from_u8(b), 1.0) }
    glColor4ub(r: u8, g: u8, b: u8, a: u8) { set_color(from_u8(r), from_u8(g), from_u8(b), from_u8(a)) }
    glColor3s(r: i16, g: i16, b: i16) { set_color(from_i16(r), from_i16(g), from_i16(b), 1.0) }
    glColor4s(r: i16, g: i16, b: i16, a: i16) { set_color(from_i16(r), from_i16(g), from_i16(b), from_i16(a)) }
    glColor3us(r: u16, g: u16, b: u16) { set_color(from_u16(r), from_u16(g), from_u16(b), 1.0) }
    glColor4us(r: u16, g: u16, b: u16, a: u16) { set_color(from_u16(r), from_u16(g), from_u16(b), from_u16(a)) }
    glColor3i(r: i32, g: i32, b: i32) { set_color(from_i32(r), from_i32(g), from_i32(b), 1.0) }
    glColor4i(r: i32, g: i32, b: i32, a: i32) { set_color(from_i32(r), from_i32(g), from_i32(b), from_i32(a)) }
    glColor3ui(r: u32, g: u32, b: u32) { set_color(from_u32(r), from_u32(g), from_u32(b), 1.0) }
    glColor4ui(r: u32, g: u32, b: u32, a: u32) { set_color(from_u32(r), from_u32(g), from_u32(b), from_u32(a)) }
    glColor3d(r: f64, g: f64, b: f64) { set_color(r as f32, g as f32, b as f32, 1.0) }
    glColor4d(r: f64, g: f64, b: f64, a: f64) { set_color(r as f32, g as f32, b as f32, a as f32) }
    glColor3fv(v: *const f32) { if !v.is_null() { set_color(*v, *v.add(1), *v.add(2), 1.0) } }
    glColor4fv(v: *const f32) { if !v.is_null() { set_color(*v, *v.add(1), *v.add(2), *v.add(3)) } }
    glColor3ubv(v: *const u8) { if !v.is_null() { set_color(from_u8(*v), from_u8(*v.add(1)), from_u8(*v.add(2)), 1.0) } }
    glColor4ubv(v: *const u8) { if !v.is_null() { set_color(from_u8(*v), from_u8(*v.add(1)), from_u8(*v.add(2)), from_u8(*v.add(3))) } }
}

/// Every name this module exports, for the resolver.
pub fn resolve(n: &[u8]) -> *const std::ffi::c_void {
    macro_rules! table {
        ($($name:ident),* $(,)?) => {
            $( if n == stringify!($name).as_bytes() { return $name as *const std::ffi::c_void; } )*
        };
    }
    table!(
        glBegin, glEnd,
        glVertex2f, glVertex3f, glVertex4f, glVertex2d, glVertex3d, glVertex4d,
        glVertex2i, glVertex3i, glVertex2fv, glVertex3fv, glVertex4fv,
        glTexCoord1f, glTexCoord2f, glTexCoord2d, glTexCoord2fv, glTexCoord3f, glTexCoord4f,
        glMultiTexCoord2f, glNormal3f, glNormal3fv,
        glColor3b, glColor4b, glColor3ub, glColor4ub, glColor3s, glColor4s, glColor3us,
        glColor4us, glColor3i, glColor4i, glColor3ui, glColor4ui, glColor3d, glColor4d,
        glColor3fv, glColor4fv, glColor3ubv, glColor4ubv,
    );
    std::ptr::null()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quad_strip_and_polygon_map_to_es_modes() {
        assert_eq!(es_mode(GL_QUAD_STRIP), GL_TRIANGLE_STRIP);
        assert_eq!(es_mode(GL_POLYGON), GL_TRIANGLE_FAN);
        assert_eq!(es_mode(0x0004), 0x0004);
    }

    #[test]
    fn incomplete_primitives_drop_their_tail() {
        assert_eq!(usable_vertex_count(0x0007, 9), 8); // quads
        assert_eq!(usable_vertex_count(0x0004, 7), 6); // triangles
        assert_eq!(usable_vertex_count(0x0001, 5), 4); // lines
        assert_eq!(usable_vertex_count(0x0005, 2), 0); // strip needs 3
        assert_eq!(usable_vertex_count(GL_QUAD_STRIP, 5), 4);
        assert_eq!(usable_vertex_count(0x0000, 3), 3); // points
    }

    #[test]
    fn integer_colours_span_the_unit_range() {
        assert_eq!(from_u8(255), 1.0);
        assert_eq!(from_u8(0), 0.0);
        assert!((from_i8(127) - 1.0).abs() < 1e-6);
        assert!((from_u16(65535) - 1.0).abs() < 1e-6);
        assert!((from_i32(i32::MAX) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn immediate_resolver_covers_the_core_calls() {
        for n in ["glBegin", "glEnd", "glVertex3f", "glTexCoord2f", "glColor4ub", "glMultiTexCoord2f"] {
            assert!(!resolve(n.as_bytes()).is_null(), "{n}");
        }
        assert!(resolve(b"glDrawArrays").is_null());
    }
}
