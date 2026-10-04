//! OpenGL entry points, split by the version that introduced them.
//!
//! The surface used to be one undifferentiated list in `lib.rs`, which made it impossible to
//! tell what a given name required or who owned it. Each submodule owns the calls its version
//! introduced, and each publishes an `EXPORTS` manifest that a test checks: a module cannot
//! claim a name that does not resolve to a real implementation, which is the failure that let
//! `glLightModeliv` silently become a no-op.
//!
//! Moved here so far: 1.0 (immediate mode, matrices, fog, lighting, texture environment) and
//! 1.1 (display lists). The 1.2-2.0 and 3.x/4.x moves follow.

/// Re-exported so the version modules can generate entry points without importing the macro
/// from the parent of their parent.
#[macro_export]
macro_rules! gl_passthrough {
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

pub mod v1_0;
pub mod v1_1;

/// Every name the GL 1.x modules claim, concatenated.
pub fn exports() -> Vec<&'static str> {
    v1_0::EXPORTS.iter().chain(v1_1::EXPORTS.iter()).copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;

    /// A module must not claim a name it does not actually implement. This is the invariant
    /// that would have caught `glLightModeliv`, which sat in the resolver's stub table and so
    /// resolved to a shared no-op while appearing to be handled.
    #[test]
    fn every_claimed_export_resolves_to_a_real_implementation() {
        let _lock = crate::tests::global_test_lock();
        let stub = crate::legacy_noop_fn as *const c_void;
        for name in exports() {
            let fp = crate::resolve_proc(name.as_bytes());
            assert!(!fp.is_null(), "{name} is claimed by a GL 1.x module but resolves to null");
            assert_ne!(fp, stub, "{name} is claimed but resolves to the legacy no-op stub");
        }
    }

    /// The manifests must not overlap: one name belongs to exactly one version.
    #[test]
    fn a_name_belongs_to_exactly_one_version() {
        let mut all: Vec<&str> = exports();
        let before = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), before, "a name is claimed by more than one version module");
    }
}
