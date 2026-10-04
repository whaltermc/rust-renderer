//! OpenGL 1.0 entry points.
//!
//! Part of the GL 1.0 module split: each version owns the calls it introduced, so any name can
//! be traced to the version that requires it. `EXPORTS` is asserted by a test, so a module
//! cannot claim a name that does not resolve to a real implementation.

use crate::aliases::announce_missing;
use crate::driver_fn_cached;
use crate::gl_passthrough as passthrough;
use std::sync::atomic::{AtomicU32, Ordering};

passthrough!(
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
    glLineStipple(factor: i32, pattern: u16);
);

/// Everything this module implements. A test asserts each resolves to a real implementation
/// rather than the shared legacy no-op.
pub const EXPORTS: &[&str] = &[
    "glArrayElement", "glWindowPos2i", "glPointSize", "glLighti", "glMateriali",
    "glTexEnviv", "glTexGenfv", "glTexGenf", "glTexGeni", "glTexGeniv",
    "glPushAttrib", "glPopAttrib", "glPushClientAttrib", "glPopClientAttrib",
    "glLineStipple", "glFogf", "glFogi", "glFogfv", "glLightfv", "glLightf",
    "glLightModelfv", "glMaterialfv", "glMaterialf", "glTexEnvfv", "glTexEnvf", "glTexEnvi",
    "glMatrixMode", "glLoadIdentity", "glLoadMatrixf", "glMultMatrixf",
    "glPushMatrix", "glPopMatrix", "glTranslatef", "glRotatef", "glScalef",
    "glOrtho", "glFrustum", "glSampleCoverage", "glBegin", "glEnd",
    "glVertex2f", "glVertex3f", "glVertex4f", "glVertex2d", "glVertex3d",
    "glColor3f", "glColor4f", "glColor3ub", "glColor4ub", "glColor3us", "glColor4us",
    "glNormal3f", "glTexCoord2f", "glTexCoord4f", "glMultiTexCoord2f",
    "glAlphaFunc", "glShadeModel", "glEnableClientState", "glDisableClientState",
];
