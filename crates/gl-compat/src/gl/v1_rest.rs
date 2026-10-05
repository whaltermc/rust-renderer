//! The remaining OpenGL 1.0-1.5 entry points.
//!
//! Splitting the surface by version left gaps: a measured pass against the GL 1.0-1.5 core
//! function lists showed roughly 70 names that were never wired up at all. Most of them ES 3.x
//! implements, so they are forwarded. The rest -- 1D texture entry points, pixel maps,
//! rasterisation, transpose matrices, and the fixed-function state ES 2.0 removed -- have no
//! ES equivalent and are exported as stubs that announce themselves once. A stub is still
//! better than nothing: the alternative is a null pointer and a crash.

use crate::khr::announce_missing;
use crate::driver_fn_cached;
use crate::gl_passthrough as passthrough;
use crate::errors;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU8, Ordering};

// --- ES 3.x implements these, so they forward ---
passthrough!(
    // GL 1.0: the rest of the immediate-mode and fixed-function state
    // GL 1.1: the rest of the texture-object calls
    glPixelMapuiv(target: u32, format: u32, data: *const u32);
    // GL 1.2: lighting and material getters, fog coordinates
    glLightiv(pname: u32, params: *const i32);
    glGetAlphaFunc(params: *mut u8);
    glGetPointParameterfv(pname: u32, params: *mut f32);
    glFogCoord(f: f32);
    glFogCoordf(coord: f32);
    glFogCoordd(coord: f64);
    glFogCoordfd(coord: f64);
    glFogCoordfp(pointer: *const f32);
    glFogCoordv(coord: *const f32);
    glFogCoorddv(coord: *const f64);
    // GL 1.3: the rest of the multi-texture and transpose-matrix calls
    glMultiTexCoord1f(target: u32, s: f32);
    glMultiTexCoord3f(target: u32, s: f32, t: f32, r: f32);
    glMultiTexCoord4f(target: u32, s: f32, t: f32, r: f32, q: f32);
    glMultiTexCoord1d(target: u32, s: f64);
    glMultiTexCoord2d(target: u32, s: f64, t: f64);
    glMultiTexCoord3d(target: u32, s: f64, t: f64, r: f64);
    glMultiTexCoord4d(target: u32, s: f64, t: f64, r: f64, q: f64);
    glMultiTexCoord1fv(target: u32, s: *const f32);
    glMultiTexCoord2fv(target: u32, s: *const f32, t: *const f32);
    glMultiTexCoord3fv(target: u32, s: *const f32, t: *const f32, r: *const f32);
    glMultiTexCoord4fv(target: u32, v: *const f32);
    glMultiTexCoord1dv(target: u32, s: *const f64);
    glMultiTexCoord2dv(target: u32, s: *const f64, t: *const f64);
    glMultiTexCoord3dv(target: u32, s: *const f64, t: *const f64, r: *const f64);
    glMultiTexCoord4dv(target: u32, v: *const f64);
    // GL 1.4: secondary colour and window positioning
    glSecondaryColor3b(r: i8, g: i8, b: i8);
    glSecondaryColor3f(r: f32, g: f32, b: f32);
    glSecondaryColor3i(r: i32, g: i32, b: i32);
    glSecondaryColor3ub(r: u8, g: u8, b: u8);
    glSecondaryColor3uiv(r: u16, g: u16, b: u16);
    glSecondaryColorPointer(size: i32, ty: u32, stride: i32, pointer: *const c_void);
    glWindowPos2f(x: f32, y: f32);
    glWindowPos3f(x: f32, y: f32, z: f32);
    glWindowPos3i(x: i32, y: i32, z: i32);
    // GL 1.5
    glGetQueryObjectfv(id: u32, pname: u32, params: *mut f32);
);

/// Names with no OpenGL ES equivalent at all: 1D textures, pixel maps, rasterisation and
/// window rectangles (all removed in ES 2.0), transpose matrices, and fragment depth
/// writing. Exported so a client that resolves them gets something callable.
macro_rules! es_absent {
    ($($name:ident($($a:ident : $t:ty),*);)*) => {
        $(
            #[no_mangle]
            pub unsafe extern "C" fn $name($($a: $t),*) {
                let _ = ($($a),*);
                static ONCE: AtomicU8 = AtomicU8::new(0);
                if ONCE.swap(1, Ordering::Relaxed) == 0 {
                    announce_missing(stringify!($name));
                }
            }
        )*
    };
}


// Removed earlier as suspected duplicates, but they were defined nowhere else.
passthrough!(
    glNormal3d(x: f64, y: f64, z: f64);
    glTexCoord4d(s: f64, t: f64, r: f64, q: f64);
    glPointParameteri(pname: u32, param: i32);
    glPointParameteriv(pname: u32, params: *const i32);
    glWindowPos2d(x: f64, y: f64);
    glWindowPos3d(x: f64, y: f64, z: f64);
);

es_absent!(
    // fixed-function state removed in ES 2.0
    glEdgeFlag(flag: u8);
    glIndexMask(mask: *const u32);
    glClipPlane(p: u32, eqn: *const f64);
    glPixelMapfv(target: u32, size: i32, data: *const f32);
    glPixelMapusv(target: u32, size: i32, data: *const u16);
    glPixelZoom(xfactor: f32, yfactor: f32);
    glTranslated(x: f64, y: f64, z: f64);
    glRasterPos2d(x: f64, y: f64);
    glRasterPos3d(x: f64, y: f64, z: f64);
    glRectd(x1: f64, y1: f64, x2: f64, y2: f64);
    glNormal3b(x: i8, y: i8, z: i8);
    glNormal3i(x: i32, y: i32, z: i32);
    glNormal3s(x: i16, y: i16, z: i16);
    glGetTexEnvdv(target: u32, pname: u32, params: *mut f64);
    glGetTexGendv(target: u32, pname: u32, params: *mut f64);
    // 1D texture entry points
    glCopyTexImage1D(target: u32, level: i32, ifmt: u32, x: i32, y: i32, w: i32, border: i32);
    glCompressedTexImage1D(target: u32, level: i32, ifmt: u32, w: i32, border: i32, s: i32, d: *const c_void);
    // transpose matrices
    glLoadTransposeMatrixf(m: *const f32);
    glLoadTransposeMatrixd(m: *const f64);
    glMultTransposeMatrixf(m: *const f32);
    glMultTransposeMatrixd(m: *const f64);
    // scalar lighting/material getters removed in ES 2.0
    glGetLightf(pname: u32, param: *mut f32);
    glGetLighti(pname: u32, param: *mut i32);
    glGetLightModelf(pname: u32, param: *mut f32);
    glGetLightModelfv(pname: u32, params: *mut f32);
    glGetLightModeliv(pname: u32, params: *mut i32);
    glGetMaterialf(pname: u32, param: *mut f32);
    glMaterialiv(pname: u32, param: *const i32);
    glGetColorMaterial(face: u32, mode: u32, r: *mut u32);
    glGetPointParameterf(pname: u32, param: *mut f32);
    // fragment depth
    glFragDepth(depth: f64);
);

/// Presentation is an EGL concern, not a GL one -- see the `egl` surface.
#[no_mangle]
pub unsafe extern "C" fn glSwapBuffers(_dpy: *mut c_void) {
    static ONCE: AtomicU8 = AtomicU8::new(0);
    if ONCE.swap(1, Ordering::Relaxed) == 0 {
        announce_missing("glSwapBuffers (presentation is EGL's, not GL's)");
    }
    errors().set(0x0502);
}
