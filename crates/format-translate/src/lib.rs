//! Desktop-GL enums / pixel formats → GLES 3.0-safe equivalents.

pub const GL_UNSIGNED_BYTE: u32 = 0x1401;
pub const GL_UNSIGNED_SHORT: u32 = 0x1403;
pub const GL_UNSIGNED_INT: u32 = 0x1405;
pub const GL_FLOAT: u32 = 0x1406;
pub const GL_HALF_FLOAT: u32 = 0x140B;
pub const GL_DEPTH_COMPONENT: u32 = 0x1902;
pub const GL_RED: u32 = 0x1903;
pub const GL_RG: u32 = 0x8227;
pub const GL_RGB: u32 = 0x1907;
pub const GL_RGBA: u32 = 0x1908;
pub const GL_BGR: u32 = 0x80E0;
pub const GL_BGRA: u32 = 0x80E1;
pub const GL_UNSIGNED_INT_8_8_8_8_REV: u32 = 0x8367;
pub const GL_UNSIGNED_SHORT_5_6_5: u32 = 0x8363;
pub const GL_UNSIGNED_SHORT_4_4_4_4: u32 = 0x8033;
pub const GL_UNSIGNED_SHORT_5_5_5_1: u32 = 0x8034;
pub const GL_CLAMP: i32 = 0x2900;
pub const GL_CLAMP_TO_BORDER: i32 = 0x812D;
pub const GL_CLAMP_TO_EDGE: i32 = 0x812F;
pub const GL_DEPTH_COMPONENT16: i32 = 0x81A5;
pub const GL_DEPTH_COMPONENT24: i32 = 0x81A6;
pub const GL_DEPTH_COMPONENT32: i32 = 0x81A7;
pub const GL_DEPTH_COMPONENT32F: i32 = 0x8CAC;
pub const GL_DEPTH24_STENCIL8: i32 = 0x88F0;
pub const GL_DEPTH32F_STENCIL8: i32 = 0x8CAD;
pub const GL_RGBA32F: i32 = 0x8814;
pub const GL_RGB32F: i32 = 0x8815;
pub const GL_RGBA16F: i32 = 0x881A;
pub const GL_RGB16F: i32 = 0x881B;
pub const GL_R8: i32 = 0x8229;
pub const GL_RG8: i32 = 0x822B;
pub const GL_RGB8: i32 = 0x8051;
pub const GL_RGBA8: i32 = 0x8058;
pub const GL_R16F: i32 = 0x822D;
pub const GL_RG16F: i32 = 0x822F;
pub const GL_R32F: i32 = 0x822E;
pub const GL_RG32F: i32 = 0x8230;
pub const GL_RGB10_A2: i32 = 0x8059;
pub const GL_SRGB8_ALPHA8: i32 = 0x8C43;
pub const GL_SRGB8: i32 = 0x8C41;

/// True when the upload is 8-bit BGRA (core GLES cannot ingest directly).
pub fn is_bgra8(format: u32, ty: u32) -> bool {
    format == GL_BGRA && (ty == GL_UNSIGNED_BYTE || ty == GL_UNSIGNED_INT_8_8_8_8_REV)
}

pub fn is_bgr8(format: u32, ty: u32) -> bool {
    format == GL_BGR && ty == GL_UNSIGNED_BYTE
}

/// Desktop often uses unsized internal formats; GLES wants sized ones.
pub fn map_internal_format(internal: i32, format: u32, ty: u32) -> i32 {
    match (internal as u32, ty) {
        (GL_DEPTH_COMPONENT, GL_FLOAT) => GL_DEPTH_COMPONENT32F,
        (GL_DEPTH_COMPONENT, GL_UNSIGNED_INT) => GL_DEPTH_COMPONENT24,
        (GL_DEPTH_COMPONENT, GL_UNSIGNED_SHORT) => GL_DEPTH_COMPONENT16,
        (GL_RGBA, GL_FLOAT) | (GL_RGBA, GL_HALF_FLOAT) if ty == GL_FLOAT => GL_RGBA32F,
        (GL_RGBA, GL_HALF_FLOAT) => GL_RGBA16F,
        (GL_RGB, GL_FLOAT) => GL_RGB32F,
        (GL_RGB, GL_HALF_FLOAT) => GL_RGB16F,
        (GL_RED, GL_UNSIGNED_BYTE) => GL_R8,
        (GL_RG, GL_UNSIGNED_BYTE) => GL_RG8,
        (GL_RGB, GL_UNSIGNED_BYTE) => GL_RGB8,
        (GL_RGBA, GL_UNSIGNED_BYTE) => GL_RGBA8,
        (GL_RED, GL_FLOAT) => GL_R32F,
        (GL_RG, GL_FLOAT) => GL_RG32F,
        (GL_RED, GL_HALF_FLOAT) => GL_R16F,
        (GL_RG, GL_HALF_FLOAT) => GL_RG16F,
        // Sized internal formats from GL_EXT_texture_format_BGRA8888. GLES 3 core has no
        // BGRA storage, and glTexImage2D already swizzles BGRA uploads to RGBA, so
        // storing RGBA keeps bindings, uploads and render targets consistent.
        (GL_BGRA, _) => GL_RGBA8,
        (GL_BGR, _) => GL_RGB8,
        // unsized depth-stencil
        (0x84F9, _) => GL_DEPTH24_STENCIL8, // GL_DEPTH_STENCIL
        _ => {
            // Already-sized or unknown: pass through; also map legacy GL_DEPTH_COMPONENT32
            if internal == GL_DEPTH_COMPONENT32 {
                GL_DEPTH_COMPONENT24 // no pure 32-bit int depth in ES3 core
            } else if format == GL_BGRA && internal == GL_RGBA as i32 {
                GL_RGBA8
            } else {
                internal
            }
        }
    }
}

/// Map external format for GLES (BGR not allowed).
pub fn map_external_format(format: u32) -> u32 {
    match format {
        GL_BGRA => GL_RGBA,
        GL_BGR => GL_RGB,
        other => other,
    }
}

/// Internal format for renderbuffer targets, which take no format/type pair. Only the
/// BGRA/BGR aliases need translating; unsized depth and depth-stencil are legal in ES 3.
pub fn map_renderbuffer_internal_format(internalformat: u32) -> u32 {
    match internalformat {
        GL_BGRA => GL_RGBA8 as u32,
        GL_BGR => GL_RGB8 as u32,
        other => other,
    }
}

pub fn map_wrap(param: i32) -> i32 {
    match param {
        GL_CLAMP | GL_CLAMP_TO_BORDER => GL_CLAMP_TO_EDGE,
        other => other,
    }
}

pub fn map_access(access: u32) -> Option<u32> {
    match access {
        0x88B8 => Some(0x0001), // READ_ONLY
        0x88B9 => Some(0x0002), // WRITE_ONLY
        0x88BA => Some(0x0003), // READ_WRITE
        _ => None,
    }
}

pub fn swizzle_bgra_to_rgba(src: &[u8], pixels: usize) -> Vec<u8> {
    let n = pixels.min(src.len() / 4);
    let mut out = Vec::with_capacity(n * 4);
    for px in src[..n * 4].chunks_exact(4) {
        out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    out
}

pub fn swizzle_bgr_to_rgb(src: &[u8], pixels: usize) -> Vec<u8> {
    let n = pixels.min(src.len() / 3);
    let mut out = Vec::with_capacity(n * 3);
    for px in src[..n * 3].chunks_exact(3) {
        out.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swizzles() {
        assert_eq!(
            swizzle_bgra_to_rgba(&[1, 2, 3, 4, 5, 6, 7, 8], 2),
            vec![3, 2, 1, 4, 7, 6, 5, 8]
        );
        assert_eq!(swizzle_bgr_to_rgb(&[1, 2, 3], 1), vec![3, 2, 1]);
    }

    #[test]
    fn maps() {
        assert_eq!(
            map_internal_format(GL_DEPTH_COMPONENT as i32, 0, GL_FLOAT),
            GL_DEPTH_COMPONENT32F
        );
        assert_eq!(map_wrap(GL_CLAMP), GL_CLAMP_TO_EDGE);
        assert_eq!(map_external_format(GL_BGRA), GL_RGBA);
        assert!(is_bgra8(GL_BGRA, GL_UNSIGNED_BYTE));
    }

    #[test]
    fn bgra_internal_format_becomes_rgba() {
        // GL_EXT_texture_format_BGRA8888 sized formats are stored as RGBA so that the
        // upload swizzle, the binding and the render target all agree.
        assert_eq!(
            map_internal_format(GL_BGRA as i32, GL_BGRA, GL_UNSIGNED_BYTE),
            GL_RGBA8
        );
        assert_eq!(map_internal_format(GL_BGRA as i32, 0, 0), GL_RGBA8);
        assert_eq!(map_internal_format(GL_BGR as i32, 0, 0), GL_RGB8);
    }

    #[test]
    fn renderbuffer_internal_formats_are_translated() {
        assert_eq!(map_renderbuffer_internal_format(GL_BGRA), GL_RGBA8 as u32);
        assert_eq!(map_renderbuffer_internal_format(GL_BGR), GL_RGB8 as u32);
        // Unsized depth is legal for ES 3 renderbuffers and must pass through.
        assert_eq!(
            map_renderbuffer_internal_format(GL_DEPTH_COMPONENT),
            GL_DEPTH_COMPONENT
        );
        assert_eq!(map_renderbuffer_internal_format(GL_RGBA8 as u32), GL_RGBA8 as u32);
    }
}
