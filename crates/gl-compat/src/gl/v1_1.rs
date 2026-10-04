//! OpenGL 1.1 entry points.
//!
//! Part of the GL 1.1 module split: each version owns the calls it introduced, so any name can
//! be traced to the version that requires it. `EXPORTS` is asserted by a test, so a module
//! cannot claim a name that does not resolve to a real implementation.

use crate::aliases::announce_missing;
use crate::driver_fn_cached;
use crate::gl_passthrough as passthrough;
use std::sync::atomic::{AtomicU32, Ordering};

// ---- display lists ----------------------------------------------------------------------
// Lists are not recorded or replayed (1.12 entity models are the main user). What matters
// here is the ABI: `glGenLists` returns the first id of a range, and it used to be declared
// as a void function taking a pointer, so callers read garbage out of the return register.

static NEXT_LIST: AtomicU32 = AtomicU32::new(1);

#[no_mangle]
pub unsafe extern "C" fn glGenLists(range: i32) -> u32 {
    if range <= 0 {
        return 0;
    }
    NEXT_LIST.fetch_add(range as u32, Ordering::Relaxed)
}

#[no_mangle]
pub unsafe extern "C" fn glNewList(_list: u32, _mode: u32) {
    announce_missing("glNewList (display lists are not recorded)");
}

#[no_mangle]
pub unsafe extern "C" fn glEndList() {}

#[no_mangle]
pub unsafe extern "C" fn glCallList(_list: u32) {
    announce_missing("glCallList (display lists are not replayed)");
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteLists(_list: u32, _range: i32) {}


/// Display lists: the ABI is what matters. `glGenLists` must return the first id of a range;
/// it was once declared as a void function taking a pointer, so callers read garbage out of
/// the return register.
pub const EXPORTS: &[&str] = &["glGenLists", "glNewList", "glEndList", "glCallList", "glDeleteLists"];
