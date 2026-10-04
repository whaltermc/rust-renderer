//! Exported aliases and legacy entry points.
//!
//! Two gaps closed here, both observed on device:
//!
//! 1. **OptiFine and other mods call ARB/EXT-suffixed names.** `glGenTexturesARB`,
//!    `glBufferDataARB`, `glFramebufferTexture2DEXT` and friends are the same calls under
//!    historical names. They were reachable through `resolve_proc` only, so a client that
//!    `dlsym`s the library got null -- `SIGSEGV pc=0x0`.
//! 2. **The fixed-function surface was not exported at all.** `glBegin`, `glVertex3f`,
//!    `glColor4f` and friends existed only in the resolver's stub table. ES 3.x implements
//!    every one of them, so they are exported *and* forwarded: the 1.12-1.16 path draws
//!    through exactly these calls, which is why stubbing them drew nothing.
//!
//! Nothing here is a silent no-op. Names ES genuinely lacks are exported so a `dlsym`
//! resolves them, and they announce themselves once so dependence is visible.

use super::*;
use std::sync::atomic::{AtomicU8, Ordering};

/// Announces a name that has no OpenGL ES implementation, once.
fn announce_missing(name: &str) {
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut v = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if !v.iter().any(|s| s == name) {
        if v.len() < 64 {
            v.push(name.to_string());
        }
        log(&format!(
            "[GLCompat] {name} has no OpenGL ES equivalent; the call is ignored"
        ));
    }
}

/// Exports `$name` with the given signature, forwarding to the Rust function `$target`.
macro_rules! forwards {
    ($name:ident, $target:ident, ($($a:ident : $t:ty),*) $(-> $r:ty)?) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name($($a: $t),*) $(-> $r)? {
            $target($($a),*)
        }
    };
}

/// Exports a call straight through to the identically named ES function.
macro_rules! passthrough {
    ($($name:ident($($a:ident : $t:ty),*) $(-> $r:ty)?;)*) => {
        $(
            #[no_mangle]
            pub unsafe extern "C" fn $name($($a: $t),*) $(-> $r)? {
                let fp = driver_fn_cached::<unsafe extern "C" fn($($t),*) $(-> $r)?>(
                    stringify!($name),
                );
                match fp {
                    Some(f) => f($($a),*),
                    None => announce_missing(stringify!($name)),
                }
            }
        )*
    };
}

/// Exports a call that resolves a *different* ES symbol by name.
macro_rules! passthrough_as {
    ($name:ident, $es_name:literal, ($($a:ident : $t:ty),*) $(-> $r:ty)?) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name($($a: $t),*) $(-> $r)? {
            let fp = driver_fn_cached::<unsafe extern "C" fn($($t),*) $(-> $r)?>($es_name);
            match fp {
                Some(f) => f($($a),*),
                None => announce_missing($es_name),
            }
        }
    };
}

/// Exports a stub for a name ES has no equivalent for. Returns a benign value so a caller
/// that ignores the result keeps running, and announces the name so the dependence is
/// visible rather than silent.
macro_rules! no_es_equivalent {
    ($($name:ident($($a:ident : $t:ty),*) $(-> $r:ty)?;)*) => {
        $(
            #[no_mangle]
            pub unsafe extern "C" fn $name($($a: $t),*) $(-> $r)? {
                let _ = ($($a),*);
                static ONCE: AtomicU8 = AtomicU8::new(0);
                if ONCE.swap(1, Ordering::Relaxed) == 0 {
                    announce_missing(stringify!($name));
                }
                Default::default()
            }
        )*
    };
}

// ---- immediate mode: ES implements all of this, and 1.12-1.16 draw through it -----------

passthrough!(
    glBegin(mode: u32);
    glEnd();
    glVertex2f(x: f32, y: f32);
    glVertex3f(x: f32, y: f32, z: f32);
    glVertex4f(x: f32, y: f32, z: f32, w: f32);
    glVertex2d(x: f64, y: f64);
    glVertex3d(x: f64, y: f64, z: f64);
    glColor3b(r: i8, g: i8, b: i8);
    glColor4b(r: i8, g: i8, b: i8, a: i8);
    glColor3ub(r: u8, g: u8, b: u8);
    glColor4ub(r: u8, g: u8, b: u8, a: u8);
    glColor3s(r: i16, g: i16, b: i16);
    glColor4s(r: i16, g: i16, b: i16, a: i16);
    glColor3us(r: u16, g: u16, b: u16);
    glColor4us(r: u16, g: u16, b: u16, a: u16);
    glColor3i(r: i32, g: i32, b: i32);
    glColor4i(r: i32, g: i32, b: i32, a: i32);
    glColor3ui(r: u32, g: u32, b: u32);
    glColor4ui(r: u32, g: u32, b: u32, a: u32);
    glNormal3f(x: f32, y: f32, z: f32);
    glTexCoord2f(s: f32, t: f32);
    glTexCoord4f(s: f32, t: f32, r: f32, q: f32);
    glMultiTexCoord2f(target: u32, s: f32, t: f32);
    glArrayElement(i: i32);
    glWindowPos2i(x: i32, y: i32);
    glPointSize(size: f32);
    glLighti(pname: u32, param: i32);
    glMateriali(pname: u32, param: i32);
    glTexEnviv(target: u32, pname: u32, params: *const i32);
    glTexGenfv(target: u32, pname: u32, params: *const f32);
    glTexGenf(target: u32, pname: u32, param: f32);
    glTexGeni(target: u32, pname: u32, param: i32);
    glTexGeniv(target: u32, pname: u32, params: *const i32);
    glPushAttrib(mask: u32);
    glPopAttrib();
    glPushClientAttrib(mask: u32);
    glPopClientAttrib();
    glNewList(list: u32, mode: u32);
    glEndList(list: u32);
    glCallList(list: u32);
    glGenLists(n: i32, lists: *mut i32);
    glDeleteLists(n: i32, lists: *const i32);
    glLineStipple(factor: i32, pattern: u16);
    glPolygonStipple(factor: i32, pattern: *const u16);
);

// ---- ARB / EXT historical spellings ------------------------------------------------------
// OptiFine reaches for these; each is the modern call under an older name.

forwards!(glGenTexturesARB, glGenTextures, (n: i32, out: *mut u32));
forwards!(glBindTextureARB, glBindTexture, (target: u32, texture: u32));
forwards!(glDeleteTexturesARB, glDeleteTextures, (n: i32, out: *const u32));
forwards!(glTexImage2DARB, glTexImage2D, (t: u32, l: i32, i: i32, w: i32, h: i32, b: i32, f: u32, ty: u32, d: *const c_void));
forwards!(glTexSubImage2DARB, glTexSubImage2D, (t: u32, l: i32, x: i32, y: i32, w: i32, h: i32, f: u32, ty: u32, d: *const c_void));
forwards!(glTexParameteriARB, glTexParameteri, (t: u32, p: u32, v: i32));
forwards!(glTexParameterfvARB, glTexParameterfv, (t: u32, p: u32, v: *const f32));
forwards!(glTexParameterivARB, glTexParameteriv, (t: u32, p: u32, v: *const i32));
passthrough_as!(glGetTexImageARB, "glGetTexImage", (t: u32, l: i32, f: u32, ty: u32, p: *mut c_void));
forwards!(glActiveTextureARB, glActiveTexture, (t: u32));
forwards!(glClientActiveTextureARB, glActiveTexture, (t: u32));
forwards!(glGenerateMipmapEXT, glGenerateMipmap, (t: u32));
forwards!(glDeleteTexturesEXT, glDeleteTextures, (n: i32, out: *const u32));
forwards!(glGenFramebuffersEXT, glGenFramebuffers, (n: i32, out: *mut u32));
forwards!(glDeleteFramebuffersEXT, glDeleteFramebuffers, (n: i32, out: *const u32));
forwards!(glBindFramebufferEXT, glBindFramebuffer, (t: u32, f: u32));
forwards!(glGenRenderbuffersEXT, glGenRenderbuffers, (n: i32, out: *mut u32));
forwards!(glDeleteRenderbuffersEXT, glDeleteRenderbuffers, (n: i32, out: *const u32));
forwards!(glBindRenderbufferEXT, glBindRenderbuffer, (t: u32, r: u32));
forwards!(glRenderbufferStorageEXT, glRenderbufferStorage, (t: u32, f: u32, w: i32, h: i32));
forwards!(glFramebufferRenderbufferEXT, glFramebufferRenderbuffer, (t: u32, a: u32, r: u32, at: u32));
forwards!(glFramebufferTexture2DEXT, glFramebufferTexture2D, (t: u32, a: u32, tt: u32, tex: u32, l: i32));
forwards!(glCheckFramebufferStatusEXT, glCheckFramebufferStatus, (t: u32) -> u32);
passthrough_as!(glBlitFramebufferEXT, "glBlitFramebuffer", (sx0: i32, sy0: i32, sx1: i32, sy1: i32, dx0: i32, dy0: i32, dx1: i32, dy1: i32, mask: u32, f: u32));

// ---- genuinely absent in ES ---------------------------------------------------------------
// Exported so a dlsym resolves to something callable, and announced once so a caller that
// depends on the behaviour is visible in the log rather than silently getting nothing.

no_es_equivalent!(
    glTexImage1D(t: u32, l: i32, i: i32, w: i32, b: i32, f: u32, ty: u32, d: *const c_void);
    glCopyTexSubImage1D(t: u32, l: i32, x: i32, y: i32, w: i32, h: i32);
    glCompressedTexSubImage1D(t: u32, l: i32, x: i32, w: i32, f: u32, s: i32, d: *const c_void);
    glGetMapdv(t: u32, i: i32, p: *mut f64);
    glGetMapfv(t: u32, i: i32, p: *mut f32);
    glGetMapiv(t: u32, i: i32, p: *mut i32);
    glSelectBuffer(n: u32, p: *const c_void);
    glFeedbackBuffer(kind: u32, n: i32, p: *mut f32);
    glRasterPos2f(x: f32, y: f32);
    glRasterPos3f(x: f32, y: f32, z: f32);
    glEvalCoord1f(x: f32);
    glEvalCoord2f(x: f32, y: f32);
    glEvalCoord3f(x: f32, y: f32, z: f32);
    glEvalMesh1(t: u32, u: i32, v0: i32, v1: i32);
    glEvalMesh2(t: u32, u: i32, v0: i32, v1: i32, n: i32);
    glEvalPoint1(v: i32);
    glEvalPoint2(u: i32, v: i32);
    glInitNames();
    glPushName(n: u32, name: u32);
    glPopName();
    glPassThrough(f: f32);
    glIndex(c: f32);
    glIndexd(c: f64);
    glIndexf(c: f32);
    glIndexi(c: i32);
    glIndexs(c: i16);
    glIndexub(c: u8);
    glFragmentMask(p: *const u32);
    glFogIndex(i: f32);
    glFogIndexPointer(type_: u32, p: *const c_void);
    glColorMaterialBoth(face: u32, mode: u32);
    glGetClipPlane(p: u32, c: *mut f64);
    glGetPixelMapfv(t: u32, s: f32, p: *mut f32);
    glGetPixelMapuiv(t: u32, p: *mut u32);
    glGetTexGenfv(t: u32, p: u32, params: *mut f32);
    glGetTexGeniv(t: u32, p: u32, params: *mut i32);
    glGetLightfv(p: u32, params: *mut f32);
    glGetLightiv(p: u32, params: *mut i32);
    glGetMaterialfv(p: u32, params: *mut f32);
    glGetMaterialiv(p: u32, params: *mut i32);
    glGetTexEnvfv(t: u32, p: u32, params: *mut f32);
    glGetTexEnviv(t: u32, p: u32, params: *mut i32);
    glAccum();
    glBitmap(w: i32, h: i32, x: f32, y: f32);
    glClearAccum(r: f32, g: f32, b: f32, a: f32);
    glClearIndex(c: f32);
    glIndexPointer(type_: u32, stride: i32, p: *const c_void);
    glEdgeFlagPointer(stride: i32, p: *const c_void);
);

#[cfg(test)]
mod tests {
    /// The names in this module exist because a client resolved them to null. They must stay
    /// reachable, or the SIGSEGV comes back.
    #[test]
    fn fixed_function_and_arb_aliases_are_reachable() {
        for name in [
            "glBegin",
            "glEnd",
            "glVertex3f",
            "glColor4f",
            "glTexCoord2f",
            "glFogfv",
            "glGenTexturesARB",
            "glBindTextureARB",
            "glTexImage2DARB",
            "glBufferDataARB",
            "glCreateShaderObjectARB",
            "glUniform1iARB",
            "glFramebufferTexture2DEXT",
            "glRenderbufferStorageEXT",
            "glGenerateMipmapEXT",
            "glTexImage1D",
            "glSelectBuffer",
        ] {
            assert!(
                !crate::resolve_proc(name.as_bytes()).is_null(),
                "{name} is exported but unreachable; a dlsym client would crash"
            );
        }
    }
}
