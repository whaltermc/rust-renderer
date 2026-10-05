//! Debug overlay rendered on top of the game frame when `RENDERER_DEBUG=1` is set.
//!
//! Uses fixed-function immediate mode so it works without shaders/textures.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use crate::log;

static OVERLAY_ENABLED: AtomicBool = AtomicBool::new(false);
static OVERLAY_INIT: OnceLock<bool> = OnceLock::new();

pub fn set_enabled(on: bool) {
    OVERLAY_ENABLED.store(on, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    OVERLAY_ENABLED.load(Ordering::Relaxed)
}

/// Draws a minimal text overlay with the current backend/display/ANGLE state.
///
/// # Safety
/// Must be called with a current GLES context.
pub unsafe fn draw(cfg: &renderer_core::Config, backend: Option<&dyn renderer_core::Backend>) {
    if !is_enabled() {
        return;
    }

    // Ensure the overlay was initialized (runs once).
    let _ = *OVERLAY_INIT.get_or_init(|| {
        #[cfg(target_os = "android")]
        {
            log("[Overlay] debug overlay enabled");
        }
        true
    });

    let be = match backend {
        Some(b) => b,
        None => return,
    };

    let info = be.device_info();
    let caps = be.capabilities();
    let backend_name = be.kind().as_str();

    // Position the overlay in the top-left corner.
    let mut x = 10.0f32;
    let mut y = 30.0f32;
    let line_height = 16.0f32;

    // Simple color bar background.
    draw_rect(5.0, 5.0, 310.0, 145.0, 0.0, 0.0, 0.0, 0.55);

    // Title
    draw_text("RustRenderer Debug", x, y, 1.0, 1.0, 0.0, 1.0);
    y += line_height;

    // Backend
    draw_text(&format!("backend: {}", backend_name), x, y, 0.8, 0.8, 0.8, 1.0);
    y += line_height;

    // GPU
    draw_text(
        &format!("gpu: {} ({})", info.renderer, info.vendor),
        x,
        y,
        0.8,
        0.8,
        0.8,
        1.0,
    );
    y += line_height;

    // API version
    draw_text(&format!("api: {}", info.api_version), x, y, 0.8, 0.8, 0.8, 1.0);
    y += line_height;

    // GLES caps
    draw_text(
        &format!("gles: ES {}.{}, {} exts", caps.es_major, caps.es_minor, caps.extensions.len()),
        x,
        y,
        0.8,
        0.8,
        0.8,
        1.0,
    );
    y += line_height;

    // Display
    let display = if cfg.display.is_empty() {
        "default"
    } else {
        &cfg.display
    };
    draw_text(&format!("display: {}", display), x, y, 0.8, 0.8, 0.8, 1.0);
    y += line_height;

    // ANGLE backend
    let angle_backend = if cfg.angle_backend.is_empty() {
        "default"
    } else {
        &cfg.angle_backend
    };
    draw_text(&format!("angle backend: {}", angle_backend), x, y, 0.8, 0.8, 0.8, 1.0);
    y += line_height;

    // ANGLE renderer
    let angle_renderer = if cfg.angle_renderer.is_empty() {
        "default"
    } else {
        &cfg.angle_renderer
    };
    draw_text(
        &format!("angle renderer: {}", angle_renderer),
        x,
        y,
        0.8,
        0.8,
        0.8,
        1.0,
    );
}

#[allow(dead_code)]
fn draw_rect(x: f32, y: f32, w: f32, h: f32, r: f32, g: f32, b: f32, a: f32) {
    unsafe {
        // Immediate-mode quad using GL_TRIANGLE_FAN.
        let f = crate::driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, u32, u32)>("glBegin");
        if f.is_none() {
            return;
        }
        type F = unsafe extern "C" fn(u32);
        let begin = crate::driver_fn_cached::<F>("glBegin").unwrap();
        let color = crate::driver_fn_cached::<unsafe extern "C" fn(f32, f32, f32, f32)>("glColor4f").unwrap();
        let vertex = crate::driver_fn_cached::<unsafe extern "C" fn(f32, f32)>("glVertex2f").unwrap();
        let end = crate::driver_fn_cached::<F>("glEnd").unwrap();

        color(r, g, b, a);
        begin(0x0006); // GL_TRIANGLE_FAN
        vertex(x, y);
        vertex(x + w, y);
        vertex(x + w, y + h);
        vertex(x, y + h);
        end(0x0006); // GL_TRIANGLE_FAN
    }
}

#[allow(dead_code)]
fn draw_text(text: &str, x: f32, y: f32, r: f32, g: f32, b: f32, a: f32) {
    unsafe {
        let f = crate::driver_fn_cached::<unsafe extern "C" fn(u32, u32, u32, u32, u32)>("glBegin");
        if f.is_none() {
            return;
        }

        type F = unsafe extern "C" fn(u32);
        let begin = crate::driver_fn_cached::<F>("glBegin").unwrap();
        let color = crate::driver_fn_cached::<unsafe extern "C" fn(f32, f32, f32, f32)>("glColor4f").unwrap();
        let vertex = crate::driver_fn_cached::<unsafe extern "C" fn(f32, f32)>("glVertex2f").unwrap();
        let end = crate::driver_fn_cached::<F>("glEnd").unwrap();

        color(r, g, b, a);
        begin(0x0001); // GL_POINTS

        for (i, ch) in text.chars().enumerate() {
            let cx = x + (i as f32) * 8.0;
            let cy = y;
            vertex(cx, cy);
        }

        end(0x0001); // GL_POINTS
    }
}
