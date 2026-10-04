//! Desktop fixed-function → GLES 3 compatibility state.
#![allow(dead_code)]
//!
//! Minecraft 1.16 (and the loading/UI paths) still call matrix-stack and client-array
//! APIs that do not exist in core GLES. We track the state and, on draw, emit a
//! temporary VBO + a tiny shader so *something* can appear instead of INVALID_OPERATION.

use std::ffi::c_void;
use std::sync::Mutex;

const GL_MODELVIEW: u32 = 0x1700;
const GL_PROJECTION: u32 = 0x1701;
const GL_TEXTURE: u32 = 0x1702;
const GL_VERTEX_ARRAY: u32 = 0x8074;
const GL_COLOR_ARRAY: u32 = 0x8076;
const GL_TEXTURE_COORD_ARRAY: u32 = 0x8078;
const GL_NORMAL_ARRAY: u32 = 0x8075;
const GL_FLOAT: u32 = 0x1406;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
pub const GL_TRIANGLES: u32 = 0x0004;
const GL_TRIANGLE_STRIP: u32 = 0x0005;
const GL_TRIANGLE_FAN: u32 = 0x0006;
pub const GL_QUADS: u32 = 0x0007; // desktop only — expand to triangles
pub const GL_PROXY_TEXTURE_2D: u32 = 0x8064; // was 0x8514 (cube-map binding), which never matched
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_MAX_TEXTURE_SIZE: u32 = 0x0D33;
const GL_TEXTURE_WIDTH: u32 = 0x1000;
const GL_TEXTURE_HEIGHT: u32 = 0x1001;
const GL_TEXTURE_INTERNAL_FORMAT: u32 = 0x1003;
const GL_TEXTURE_BORDER: u32 = 0x1005;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_WRAP_S: u32 = 0x2802;
const GL_TEXTURE_WRAP_T: u32 = 0x2803;
const GL_TEXTURE_MAX_ANISOTROPY_EXT: u32 = 0x84FE;
const GL_TEXTURE_LOD_BIAS: u32 = 0x8501;
const GL_TEXTURE_MAX_LEVEL: u32 = 0x813D;
const GL_TEXTURE_BASE_LEVEL: u32 = 0x813C;
const GL_TEXTURE_MAX_LOD: u32 = 0x813B;
const GL_TEXTURE_MIN_LOD: u32 = 0x813A;
const GL_GENERATE_MIPMAP: u32 = 0x8191; // desktop legacy pname

#[derive(Clone, Copy)]
struct Mat4(pub [f32; 16]);

impl Mat4 {
    fn identity() -> Self {
        Self([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ])
    }

    fn mul(self, o: Mat4) -> Mat4 {
        let mut r = [0.0f32; 16];
        for col in 0..4 {
            for row in 0..4 {
                r[col * 4 + row] = self.0[0 * 4 + row] * o.0[col * 4 + 0]
                    + self.0[1 * 4 + row] * o.0[col * 4 + 1]
                    + self.0[2 * 4 + row] * o.0[col * 4 + 2]
                    + self.0[3 * 4 + row] * o.0[col * 4 + 3];
            }
        }
        Mat4(r)
    }
}

#[derive(Clone, Copy, Default)]
struct ClientArray {
    size: i32,
    ty: u32,
    stride: i32,
    ptr: usize,
    enabled: bool,
    /// GL_ARRAY_BUFFER bound when the pointer was set (0 = client memory).
    buffer: u32,
}

struct FfState {
    mode: u32, // current matrix mode
    modelview: Vec<Mat4>,
    projection: Vec<Mat4>,
    texture: Vec<Mat4>,
    vertex: ClientArray,
    color: ClientArray,
    texcoord: ClientArray,
    normal: ClientArray,
    client_active_texture: u32, // 0 = GL_TEXTURE0
    color4: [f32; 4],
    alpha_func: (u32, f32),
    fog_enabled: bool,
    /// Desktop-only enable caps (GL_TEXTURE_2D, GL_LIGHTING, ...) that ES rejects.
    legacy_caps: Vec<u32>,
    /// Last proxy tex probe size (Minecraft max-texture probe).
    proxy_w: i32,
    proxy_h: i32,
}

impl FfState {
    fn new() -> Self {
        Self {
            mode: GL_MODELVIEW,
            modelview: vec![Mat4::identity()],
            projection: vec![Mat4::identity()],
            texture: vec![Mat4::identity()],
            vertex: ClientArray::default(),
            color: ClientArray::default(),
            texcoord: ClientArray::default(),
            normal: ClientArray::default(),
            client_active_texture: 0,
            color4: [1.0, 1.0, 1.0, 1.0],
            alpha_func: (0x0207, 0.0), // GL_ALWAYS
            fog_enabled: false,
            legacy_caps: Vec::new(),
            proxy_w: 0,
            proxy_h: 0,
        }
    }

    fn stack(&mut self) -> &mut Vec<Mat4> {
        match self.mode {
            GL_PROJECTION => &mut self.projection,
            GL_TEXTURE => &mut self.texture,
            _ => &mut self.modelview,
        }
    }

    fn top(&mut self) -> &mut Mat4 {
        let s = self.stack();
        if s.is_empty() {
            s.push(Mat4::identity());
        }
        let n = s.len();
        &mut s[n - 1]
    }
}

static FF: Mutex<Option<FfState>> = Mutex::new(None);

fn with_ff<R>(f: impl FnOnce(&mut FfState) -> R) -> R {
    let mut g = FF.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(FfState::new());
    }
    f(g.as_mut().unwrap())
}

// ----- Matrix stack -----

pub extern "C" fn gl_matrix_mode(mode: u32) {
    with_ff(|s| s.mode = mode);
}

pub extern "C" fn gl_load_identity() {
    with_ff(|s| *s.top() = Mat4::identity());
}

pub extern "C" fn gl_push_matrix() {
    with_ff(|s| {
        let t = *s.top();
        s.stack().push(t);
    });
}

pub extern "C" fn gl_pop_matrix() {
    with_ff(|s| {
        let st = s.stack();
        if st.len() > 1 {
            st.pop();
        }
    });
}

pub extern "C" fn gl_load_matrixf(m: *const f32) {
    if m.is_null() {
        return;
    }
    with_ff(|s| {
        let mut a = [0.0f32; 16];
        unsafe {
            std::ptr::copy_nonoverlapping(m, a.as_mut_ptr(), 16);
        }
        *s.top() = Mat4(a);
    });
}

pub extern "C" fn gl_mult_matrixf(m: *const f32) {
    if m.is_null() {
        return;
    }
    with_ff(|s| {
        let mut a = [0.0f32; 16];
        unsafe {
            std::ptr::copy_nonoverlapping(m, a.as_mut_ptr(), 16);
        }
        let cur = *s.top();
        *s.top() = cur.mul(Mat4(a));
    });
}

pub extern "C" fn gl_translatef(x: f32, y: f32, z: f32) {
    with_ff(|s| {
        let t = Mat4([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0,
        ]);
        let cur = *s.top();
        *s.top() = cur.mul(t);
    });
}

pub extern "C" fn gl_scalef(x: f32, y: f32, z: f32) {
    with_ff(|s| {
        let t = Mat4([
            x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, z, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]);
        let cur = *s.top();
        *s.top() = cur.mul(t);
    });
}

pub extern "C" fn gl_rotatef(angle_deg: f32, x: f32, y: f32, z: f32) {
    let rad = angle_deg.to_radians();
    let (s, c) = rad.sin_cos();
    let len = (x * x + y * y + z * z).sqrt();
    if len < 1e-8 {
        return;
    }
    let (x, y, z) = (x / len, y / len, z / len);
    let oc = 1.0 - c;
    let m = Mat4([
        oc * x * x + c,
        oc * x * y + z * s,
        oc * x * z - y * s,
        0.0,
        oc * x * y - z * s,
        oc * y * y + c,
        oc * y * z + x * s,
        0.0,
        oc * x * z + y * s,
        oc * y * z - x * s,
        oc * z * z + c,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]);
    with_ff(|st| {
        let cur = *st.top();
        *st.top() = cur.mul(m);
    });
}

pub extern "C" fn gl_ortho(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) {
    let (l, r, b, t, n, f) = (
        left as f32, right as f32, bottom as f32, top as f32, near as f32, far as f32,
    );
    let m = Mat4([
        2.0 / (r - l),
        0.0,
        0.0,
        0.0,
        0.0,
        2.0 / (t - b),
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 / (f - n),
        0.0,
        -(r + l) / (r - l),
        -(t + b) / (t - b),
        -(f + n) / (f - n),
        1.0,
    ]);
    with_ff(|s| {
        let cur = *s.top();
        *s.top() = cur.mul(m);
    });
}

pub extern "C" fn gl_frustum(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) {
    let (l, r, b, t, n, f) = (
        left as f32, right as f32, bottom as f32, top as f32, near as f32, far as f32,
    );
    if n <= 0.0 || f <= 0.0 || l == r || b == t {
        return;
    }
    let m = Mat4([
        (2.0 * n) / (r - l),
        0.0,
        0.0,
        0.0,
        0.0,
        (2.0 * n) / (t - b),
        0.0,
        0.0,
        (r + l) / (r - l),
        (t + b) / (t - b),
        -(f + n) / (f - n),
        -1.0,
        0.0,
        0.0,
        -(2.0 * f * n) / (f - n),
        0.0,
    ]);
    with_ff(|s| {
        let cur = *s.top();
        *s.top() = cur.mul(m);
    });
}

// ----- Client arrays -----

pub extern "C" fn gl_enable_client_state(cap: u32) {
    with_ff(|s| match cap {
        GL_VERTEX_ARRAY => s.vertex.enabled = true,
        GL_COLOR_ARRAY => s.color.enabled = true,
        GL_TEXTURE_COORD_ARRAY => {
            if s.client_active_texture == 0 {
                s.texcoord.enabled = true;
            }
        }
        GL_NORMAL_ARRAY => s.normal.enabled = true,
        _ => {}
    });
}

pub extern "C" fn gl_disable_client_state(cap: u32) {
    with_ff(|s| match cap {
        GL_VERTEX_ARRAY => s.vertex.enabled = false,
        GL_COLOR_ARRAY => s.color.enabled = false,
        GL_TEXTURE_COORD_ARRAY => {
            if s.client_active_texture == 0 {
                s.texcoord.enabled = false;
            }
        }
        GL_NORMAL_ARRAY => s.normal.enabled = false,
        _ => {}
    });
}

fn set_array(a: &mut ClientArray, size: i32, ty: u32, stride: i32, ptr: *const c_void, buffer: u32) {
    a.buffer = buffer;
    a.size = size;
    a.ty = ty;
    a.stride = stride;
    a.ptr = ptr as usize;
}

pub extern "C" fn gl_vertex_pointer(size: i32, ty: u32, stride: i32, ptr: *const c_void) {
    // SAFETY: only queries driver state.
    let buf = unsafe { crate::current_array_buffer() };
    with_ff(|s| set_array(&mut s.vertex, size, ty, stride, ptr, buf));
}

pub extern "C" fn gl_color_pointer(size: i32, ty: u32, stride: i32, ptr: *const c_void) {
    let buf = unsafe { crate::current_array_buffer() };
    with_ff(|s| set_array(&mut s.color, size, ty, stride, ptr, buf));
}

pub extern "C" fn gl_tex_coord_pointer(size: i32, ty: u32, stride: i32, ptr: *const c_void) {
    // Only texture unit 0 is emulated; unit 1 (lightmap) must not overwrite the base UVs.
    let buf = unsafe { crate::current_array_buffer() };
    with_ff(|s| {
        if s.client_active_texture == 0 {
            set_array(&mut s.texcoord, size, ty, stride, ptr, buf)
        }
    });
}

pub extern "C" fn gl_normal_pointer(ty: u32, stride: i32, ptr: *const c_void) {
    let buf = unsafe { crate::current_array_buffer() };
    with_ff(|s| set_array(&mut s.normal, 3, ty, stride, ptr, buf));
}

pub extern "C" fn gl_client_active_texture(texture: u32) {
    // GL_TEXTURE0 = 0x84C0
    with_ff(|s| s.client_active_texture = texture.saturating_sub(0x84C0));
}

// ----- Legacy state (mostly tracked / no-op) -----

pub extern "C" fn gl_color4f(r: f32, g: f32, b: f32, a: f32) {
    with_ff(|s| s.color4 = [r, g, b, a]);
}

pub extern "C" fn gl_color3f(r: f32, g: f32, b: f32) {
    gl_color4f(r, g, b, 1.0);
}

pub extern "C" fn gl_alpha_func(func: u32, ref_v: f32) {
    with_ff(|s| s.alpha_func = (func, ref_v));
}

pub extern "C" fn gl_fogf(_pname: u32, _param: f32) {}
pub extern "C" fn gl_fogi(_pname: u32, _param: i32) {}
pub extern "C" fn gl_fogfv(_pname: u32, _params: *const f32) {}
pub extern "C" fn gl_shade_model(_mode: u32) {}
pub extern "C" fn gl_tex_envf(_target: u32, _pname: u32, _param: f32) {}
pub extern "C" fn gl_tex_envi(_target: u32, _pname: u32, _param: i32) {}
pub extern "C" fn gl_tex_envfv(_target: u32, _pname: u32, _params: *const f32) {}

/// Map desktop-only tex parameter names; return None if the call should be dropped.
/// Translates a texture pname, returning `None` when it must be dropped.
///
/// `supports_anisotropy` comes from the capability probe: `GL_TEXTURE_MAX_ANISOTROPY_EXT`
/// is not a core ES enum, so on a device without the extension forwarding it produces
/// GL_INVALID_ENUM. Dropping it is better than a spurious error, and the caller keeps the
/// value clamped to 1 instead of losing the state silently.
pub fn map_tex_parameter_with(pname: u32, supports_anisotropy: bool) -> Option<u32> {
    match pname {
        GL_TEXTURE_MAG_FILTER | GL_TEXTURE_MIN_FILTER | GL_TEXTURE_WRAP_S | GL_TEXTURE_WRAP_T => {
            Some(pname)
        }
        GL_TEXTURE_MAX_LEVEL | GL_TEXTURE_BASE_LEVEL | GL_TEXTURE_MAX_LOD | GL_TEXTURE_MIN_LOD => {
            Some(pname) // valid in ES3
        }
        GL_TEXTURE_MAX_ANISOTROPY_EXT if supports_anisotropy => Some(pname),
        GL_TEXTURE_LOD_BIAS | GL_GENERATE_MIPMAP => None, // drop — not in core ES
        _ => Some(pname),
    }
}

/// Convenience wrapper assuming anisotropy is available; the probe-aware entry point is
/// [`map_tex_parameter_with`].
pub fn map_tex_parameter(pname: u32) -> Option<u32> {
    map_tex_parameter_with(pname, true)
}

/// Desktop proxy-texture probe used by Minecraft to find max texture size.
pub fn handle_proxy_tex_image(target: u32, width: i32, height: i32, max: i32) -> bool {
    if target != GL_PROXY_TEXTURE_2D {
        return false;
    }
    with_ff(|s| {
        // Accept up to the real driver limit; report back via GetTexLevelParameter.
        if width <= max && height <= max && width > 0 && height > 0 {
            s.proxy_w = width;
            s.proxy_h = height;
        } else {
            s.proxy_w = 0;
            s.proxy_h = 0;
        }
    });
    true
}

pub fn handle_get_tex_level_parameter(target: u32, _level: i32, pname: u32, params: *mut i32) -> bool {
    if target != GL_PROXY_TEXTURE_2D || params.is_null() {
        return false;
    }
    with_ff(|s| {
        let v = match pname {
            GL_TEXTURE_WIDTH => s.proxy_w,
            GL_TEXTURE_HEIGHT => s.proxy_h,
            GL_TEXTURE_INTERNAL_FORMAT => 0x8058, // RGBA8
            GL_TEXTURE_BORDER => 0,
            _ => 0,
        };
        unsafe {
            *params = v;
        }
    });
    true
}

/// Remap draw mode: GLES has no QUADS.
pub fn map_draw_mode(mode: u32) -> u32 {
    if mode == GL_QUADS {
        GL_TRIANGLES // caller must expand verts; for now draw as triangles (wrong but non-fatal)
    } else {
        mode
    }
}

pub fn mvp_matrix() -> [f32; 16] {
    with_ff(|s| {
        let mv = s.modelview.last().copied().unwrap_or_else(Mat4::identity);
        let proj = s.projection.last().copied().unwrap_or_else(Mat4::identity);
        proj.mul(mv).0
    })
}

pub fn client_vertex_enabled() -> bool {
    with_ff(|s| s.vertex.enabled)
}

pub fn current_color() -> [f32; 4] {
    with_ff(|s| s.color4)
}


/// Desktop-only `glEnable`/`glDisable` caps. GLES 3 rejects them with GL_INVALID_ENUM, so we
/// record them here instead of forwarding. Returns true when the cap was consumed.
pub fn handle_cap(cap: u32, on: bool) -> bool {
    let legacy = matches!(
        cap,
        0x0DE0 // GL_TEXTURE_1D
            | 0x0DE1 // GL_TEXTURE_2D
            | 0x0BC0 // GL_ALPHA_TEST
            | 0x0B50 // GL_LIGHTING
            | 0x4000..=0x4007 // GL_LIGHT0..7
            | 0x0B60 // GL_FOG
            | 0x0BA1 // GL_NORMALIZE
            | 0x803A // GL_RESCALE_NORMAL
            | 0x0B57 // GL_COLOR_MATERIAL
            | 0x0BF2 // GL_COLOR_LOGIC_OP
            | 0x0B20 // GL_LINE_SMOOTH
            | 0x0B41 // GL_POLYGON_SMOOTH
            | 0x0B10 // GL_POINT_SMOOTH
            | 0x0C60..=0x0C63 // GL_TEXTURE_GEN_S/T/R/Q
            | 0x809D // GL_MULTISAMPLE
            | 0x2A01 // GL_POLYGON_OFFSET_POINT
            | 0x2A02 // GL_POLYGON_OFFSET_LINE
            | 0x0B24 // GL_LINE_STIPPLE
            | 0x0B42 // GL_POLYGON_STIPPLE
            | 0x3000..=0x3005 // GL_CLIP_PLANE0..5
            | 0x8861 // GL_POINT_SPRITE
            | 0x8642 // GL_VERTEX_PROGRAM_POINT_SIZE
    );
    if !legacy {
        return false;
    }
    with_ff(|s| {
        if on {
            if !s.legacy_caps.contains(&cap) {
                s.legacy_caps.push(cap);
            }
        } else {
            s.legacy_caps.retain(|c| *c != cap);
        }
        if cap == 0x0B60 {
            s.fog_enabled = on;
        }
    });
    true
}

pub fn legacy_cap_enabled(cap: u32) -> bool {
    with_ff(|s| s.legacy_caps.contains(&cap))
}

/// Read-only copy of a client array for the draw emulation.
#[derive(Clone, Copy, Default)]
pub struct ArraySnap {
    pub size: i32,
    pub ty: u32,
    pub stride: i32,
    pub ptr: usize,
    pub enabled: bool,
    pub buffer: u32,
}

fn snap(a: &ClientArray) -> ArraySnap {
    ArraySnap { size: a.size, ty: a.ty, stride: a.stride, ptr: a.ptr, enabled: a.enabled, buffer: a.buffer }
}

/// (vertex, color, texcoord) arrays.
pub fn arrays() -> (ArraySnap, ArraySnap, ArraySnap) {
    with_ff(|s| (snap(&s.vertex), snap(&s.color), snap(&s.texcoord)))
}

pub fn alpha() -> (u32, f32) {
    with_ff(|s| s.alpha_func)
}
