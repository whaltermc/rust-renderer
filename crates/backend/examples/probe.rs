//! Prints what the Vulkan backend finds on this machine.
//!
//! Useful on its own for diagnosing a device, and as the reference for what `probe()`
//! does at renderer startup:
//!
//! ```text
//! cargo run -p vulkan-backend --example probe
//! ```
//!
//! Exits 0 when a device was found (even one that cannot render yet) and 1 when there is
//! no usable Vulkan loader, so a script can branch on it.

fn main() {
    println!("probing Vulkan...");
    match backend::vulkan::probe() {
        Ok(backend) => {
            let info = backend.device_info();
            println!("  vendor    : {}", info.vendor);
            println!("  device    : {}", info.renderer);
            println!("  api       : {}", info.api_version);
            println!("  glsl      : {}", info.glsl_version);
            let caps = backend.capabilities();
            println!("  max tex   : {}", caps.max_texture_size);
            println!("  draw bufs : {}", caps.max_draw_buffers);
            println!("  can render: {}", backend.can_render());
            if !backend.can_render() {
                println!(
                    "\nA device was found, but this backend has no SPIR-V/pipeline/present path,\n\
                     so the renderer stays on GLES for the game's GL entry points."
                );
            }
        }
        Err(e) => {
            println!("  unavailable: {e}");
            println!("\nThe renderer will fall back to GLES.");
            std::process::exit(1);
        }
    }
}
