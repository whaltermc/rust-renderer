//! Pure functions mapping desktop-GL enums/pixel formats to what GLES 3.0 accepts.
//! No GL calls here, so everything is unit-testable.

pub const GL_UNSIGNED_BYTE: u32 = 0x1401;
pub const GL_UNSIGNED_SHORT: u32 = 0x1403;
pub const GL_UNSIGNED_INT: u32 = 0x1405;
pub const GL_FLOAT: u32 = 0x1406;
pub const GL_DEPTH_COMPONENT: u32 = 0x1902;
pub const GL_RGB: u32 = 0x1907;
pub const GL_RGBA: u32 = 0x1908;
pub const GL_BGRA: u32 = 0x80E1;
pub const GL_UNSIGNED_INT_8_8_8_8_REV: u32 = 0x8367;
pub const GL_CLAMP: i32 = 0x2900;
pub const GL_CLAMP_TO_BORDER: i32 = 0x812D;
pub const GL_CLAMP_TO_EDGE: i32 = 0x812F;
pub const GL_DEPTH_COMPONENT16: i32 = 0x81A5;
pub const GL_DEPTH_COMPONENT24: i32 = 0x81A6;
pub const GL_DEPTH_COMPONENT32F: i32 = 0x8CAC;
pub const GL_RGBA32F: i32 = 0x8814;
pub const GL_RGB32F: i32 = 0x8815;

/// True when the upload is 8-bit BGRA, which core GLES cannot ingest directly.
/// (`GL_UNSIGNED_INT_8_8_8_8_REV` on little-endian is the same byte order as BGRA bytes.)
pub fn is_bgra8(format: u32, ty: u32) -> bool {
    format == GL_BGRA && (ty == GL_UNSIGNED_BYTE || ty == GL_UNSIGNED_INT_8_8_8_8_REV)
}

/// Desktop GL accepts unsized internal formats with any type; GLES needs sized ones here.
pub fn map_internal_format(internal: i32, _format: u32, ty: u32) -> i32 {
    match (internal as u32, ty) {
        (GL_DEPTH_COMPONENT, GL_FLOAT) => GL_DEPTH_COMPONENT32F,
        (GL_DEPTH_COMPONENT, GL_UNSIGNED_INT) => GL_DEPTH_COMPONENT24,
        (GL_DEPTH_COMPONENT, GL_UNSIGNED_SHORT) => GL_DEPTH_COMPONENT16,
        (GL_RGBA, GL_FLOAT) => GL_RGBA32F,
        (GL_RGB, GL_FLOAT) => GL_RGB32F,
        _ => internal,
    }
}

/// `GL_CLAMP` does not exist in core desktop 3.x or ES; border clamp is not core ES 3.0.
/// Both fall back to clamp-to-edge (callers should log the border case).
pub fn map_wrap(param: i32) -> i32 {
    match param {
        GL_CLAMP | GL_CLAMP_TO_BORDER => GL_CLAMP_TO_EDGE,
        other => other,
    }
}

/// `glMapBuffer` access enum -> `glMapBufferRange` access bits.
pub fn map_access(access: u32) -> Option<u32> {
    match access {
        0x88B8 => Some(0x0001), // GL_READ_ONLY  -> GL_MAP_READ_BIT
        0x88B9 => Some(0x0002), // GL_WRITE_ONLY -> GL_MAP_WRITE_BIT
        0x88BA => Some(0x0003), // GL_READ_WRITE
        _ => None,
    }
}

/// Swaps B and R in tightly packed 4-byte pixels. `src.len()` must be `pixels * 4`.
pub fn swizzle_bgra_to_rgba(src: &[u8], pixels: usize) -> Vec<u8> {
    let n = pixels.min(src.len() / 4);
    let mut out = Vec::with_capacity(n * 4);
    for px in src[..n * 4].chunks_exact(4) {
        out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swizzles_pixels() {
        assert_eq!(swizzle_bgra_to_rgba(&[1, 2, 3, 4, 5, 6, 7, 8], 2), vec![3, 2, 1, 4, 7, 6, 5, 8]);
    }

    #[test]
    fn swizzle_never_reads_past_slice() {
        assert_eq!(swizzle_bgra_to_rgba(&[1, 2, 3, 4], 10).len(), 4);
    }

    #[test]
    fn detects_bgra() {
        assert!(is_bgra8(GL_BGRA, GL_UNSIGNED_BYTE));
        assert!(is_bgra8(GL_BGRA, GL_UNSIGNED_INT_8_8_8_8_REV));
        assert!(!is_bgra8(GL_RGBA, GL_UNSIGNED_BYTE));
    }

    #[test]
    fn maps_formats_and_wrap() {
        assert_eq!(map_internal_format(GL_DEPTH_COMPONENT as i32, 0, GL_FLOAT), GL_DEPTH_COMPONENT32F);
        assert_eq!(map_internal_format(0x8058, GL_RGBA, GL_UNSIGNED_BYTE), 0x8058);
        assert_eq!(map_wrap(GL_CLAMP), GL_CLAMP_TO_EDGE);
        assert_eq!(map_wrap(0x2901), 0x2901);
    }

    #[test]
    fn maps_access() {
        assert_eq!(map_access(0x88BA), Some(3));
        assert_eq!(map_access(1), None);
    }
}
