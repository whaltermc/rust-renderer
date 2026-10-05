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
                // Returning quietly would let the caller believe the state was applied.
                // GL_INVALID_OPERATION is the honest answer: OpenGL ES has no equivalent, so
                // the call did not happen. A caller that checks -- which Minecraft does after
                // resource setup -- then knows, instead of rendering with silent garbage.
                errors().set(0x0502 /* GL_INVALID_OPERATION */);
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

/// The names this module cannot serve, and why.
///
/// Three tiers, following MobileGL's surface discipline (its dispatch is 150 names, all ES 3.x
/// plus EXT, with no fixed-function surface at all -- it simply omits what its target ES does
/// not have). We cannot omit: LWJGL enumerates the GL 1.x names, and an unresolved symbol is
/// the `SIGSEGV pc=0x0` crash that killed 1.16.5. So:
///
/// 1. **ES-backed** -- forwarded to the real driver.
/// 2. **Layer-implemented** -- immediate mode, the matrix stack, client arrays, fog, lighting,
///    texture environment and the attribute stacks, handled by `fixed_func` / `immediate` /
///    `fixed_draw`. Needed for Minecraft 1.12-1.16, which MobileGL does not target at all.
/// 3. **ES-absent** -- the entries below. MobileGL omits them; we export them and raise
///    `GL_INVALID_OPERATION` so a caller that depends on one finds out, rather than getting a
///    silent no-op or a null pointer.
///
/// Also absent by ES's design, handled elsewhere rather than here: 1D textures and texel
/// buffers (ES never had them), and `glSwapBuffers` (presentation is EGL's, not GL's).
pub const NO_ES_EQUIVALENT: &[(&str, &str)] = &[
    ("glPixelMapfv", "pixel maps were removed in ES 2.0"),
    ("glPixelMapuiv", "pixel maps were removed in ES 2.0"),
    ("glPixelMapusv", "pixel maps were removed in ES 2.0"),
    ("glPixelZoom", "removed in ES 2.0"),
    ("glRasterPos2d", "raster position was removed in ES 2.0; no ES equivalent"),
    ("glRasterPos3d", "raster position was removed in ES 2.0; no ES equivalent"),
    ("glRectd", "window rectangles were removed in ES 2.0"),
    ("glEdgeFlag", "edge flags were removed in ES 2.0"),
    ("glIndexMask", "colour index was removed in ES 2.0"),
    ("glIndexd", "colour index was removed in ES 2.0"),
    ("glIndexf", "colour index was removed in ES 2.0"),
    ("glIndexi", "colour index was removed in ES 2.0"),
    ("glIndexs", "colour index was removed in ES 2.0"),
    ("glIndexub", "colour index was removed in ES 2.0"),
    ("glInitNames", "colour index was removed in ES 2.0"),
    ("glPassThrough", "colour index was removed in ES 2.0"),
    ("glPushName", "colour index was removed in ES 2.0"),
    ("glPopName", "colour index was removed in ES 2.0"),
    ("glSelectBuffer", "selection buffers were removed in ES 2.0"),
    ("glFeedbackBuffer", "feedback buffers were removed in ES 2.0"),
    ("glEvalCoord1f", "the evaluators were removed in ES 2.0"),
    ("glEvalCoord2f", "the evaluators were removed in ES 2.0"),
    ("glEvalCoord3f", "the evaluators were removed in ES 2.0"),
    ("glEvalMesh1", "the evaluators were removed in ES 2.0"),
    ("glEvalMesh2", "the evaluators were removed in ES 2.0"),
    ("glEvalPoint1", "the evaluators were removed in ES 2.0"),
    ("glEvalPoint2", "the evaluators were removed in ES 2.0"),
    ("glClipPlane", "clip planes were removed in ES 2.0"),
    ("glGetClipPlane", "clip planes were removed in ES 2.0"),
    ("glTranslated", "the fixed-function transform stack was removed in ES 2.0"),
    ("glLoadTransposeMatrixf", "transpose matrices are desktop GL; ES has none"),
    ("glLoadTransposeMatrixd", "transpose matrices are desktop GL; ES has none"),
    ("glMultTransposeMatrixf", "transpose matrices are desktop GL; ES has none"),
    ("glMultTransposeMatrixd", "transpose matrices are desktop GL; ES has none"),
    ("glNormal3b", "removed in ES 2.0; use glNormal3f"),
    ("glNormal3i", "removed in ES 2.0; use glNormal3f"),
    ("glNormal3s", "removed in ES 2.0; use glNormal3f"),
    ("glGetTexEnvdv", "ES exposes the float and int forms only"),
    ("glGetTexGendv", "ES exposes the float and int forms only"),
    ("glGetLightf", "ES 2.0 dropped the scalar lighting getters"),
    ("glGetLighti", "ES 2.0 dropped the scalar lighting getters"),
    ("glGetLightModelf", "ES 2.0 dropped the scalar lighting getters"),
    ("glGetLightModelfv", "ES 2.0 dropped the scalar lighting getters"),
    ("glGetLightModeliv", "ES 2.0 dropped the scalar lighting getters"),
    ("glGetMaterialf", "ES 2.0 dropped the scalar material getters"),
    ("glMaterialiv", "ES 2.0 dropped the scalar material setters"),
    ("glGetColorMaterial", "removed in ES 2.0; no ES equivalent"),
    ("glGetPointParameterf", "ES exposes the vector form only"),
    ("glFragDepth", "ES 3.x has no fragment-depth write"),
    ("glCopyTexImage1D", "ES 3.x has no 1D textures"),
    ("glCompressedTexImage1D", "ES 3.x has no 1D textures"),
    ("glCompressedTexSubImage1D", "ES 3.x has no 1D textures"),
];

/// The table as NUL-terminated C strings, so a C consumer gets a real `char*`. Returning
/// `str::as_ptr` would not do: Rust strings carry no terminator, and every reader would run
/// off the end into whatever followed.
fn es_absent_c_strings() -> &'static [std::ffi::CString] {
    use std::sync::OnceLock;
    static S: OnceLock<Vec<std::ffi::CString>> = OnceLock::new();
    S.get_or_init(|| {
        NO_ES_EQUIVALENT
            .iter()
            .filter_map(|(n, _)| std::ffi::CString::new(*n).ok())
            .collect()
    })
}

/// Number of entries in [`NO_ES_EQUIVALENT`], so a harness can walk them without duplicating
/// the table.
#[no_mangle]
pub extern "C" fn glcompat_no_es_equivalent_count() -> u32 {
    es_absent_c_strings().len() as u32
}

/// Name of entry `i` in [`NO_ES_EQUIVALENT`], NUL-terminated, or null past the end.
#[no_mangle]
pub extern "C" fn glcompat_no_es_equivalent_name(i: u32) -> *const std::ffi::c_char {
    match es_absent_c_strings().get(i as usize) {
        Some(c) => c.as_ptr(),
        None => std::ptr::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The export check cannot run here: a `cargo test` binary does not export its own
    /// symbols, so the symbol-table lookup sees nothing and every name would look missing.
    /// The harness checks it against the real cdylib instead, where the symbols are exported.
    #[test]
    fn every_es_absent_name_carries_a_reason() {
        for (name, reason) in NO_ES_EQUIVALENT {
            assert!(!reason.is_empty(), "{name} has no recorded reason");
            assert!(name.starts_with("gl"), "{name} is not a GL entry point");
        }
    }

    #[test]
    fn the_table_is_not_empty_and_has_no_duplicates() {
        assert!(NO_ES_EQUIVALENT.len() > 40, "expected a substantial table");
        let mut names: Vec<&str> = NO_ES_EQUIVALENT.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate entry in NO_ES_EQUIVALENT");
    }
}
