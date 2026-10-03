//! Vulkan backend: NOT IMPLEMENTED (spec phase 5). `probe` always fails so that
//! `RENDERER_BACKEND=auto` falls back to GLES and `=vulkan` fails loudly.

use renderer_core::{Backend, BackendError};

pub fn probe() -> Result<Box<dyn Backend>, BackendError> {
    Err(BackendError::Unsupported(
        "Vulkan backend is not implemented yet (spec phase 5)".into(),
    ))
}
