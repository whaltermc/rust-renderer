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

/// Maps a desktop `glTexImage2D` format triple onto one OpenGL ES 3.0 accepts.
///
/// Returning only the internal format is not enough for depth: ES requires the *type* to
/// match a sized internal format, and desktop code routinely writes
/// `(GL_DEPTH_COMPONENT24, GL_DEPTH_COMPONENT, GL_FLOAT)`, which ES rejects. Minecraft
/// allocates its window depth attachment exactly that way, and the driver's
/// `GL_INVALID_OPERATION` surfaced as "OpenGL error 1282" during framebuffer setup.
pub fn map_upload_format(internal: i32, format: u32, ty: u32) -> (i32, u32, u32) {
    let mapped_i = map_internal_format(internal, format, ty);
    let mapped = mapped_i as u32;
    const DEPTH16: u32 = 0x81A5;
    const DEPTH24: u32 = 0x81A6;
    const DEPTH32F: u32 = 0x8CAC;
    const DEPTH24_STENCIL8: u32 = 0x88F0;
    match (mapped, format) {
        (DEPTH16, GL_DEPTH_COMPONENT) => {
            (mapped_i, GL_DEPTH_COMPONENT, GL_UNSIGNED_SHORT)
        }
        (DEPTH24, GL_DEPTH_COMPONENT) => {
            (mapped_i, GL_DEPTH_COMPONENT, GL_UNSIGNED_INT)
        }
        (DEPTH32F, GL_DEPTH_COMPONENT) => {
            (mapped_i, GL_DEPTH_COMPONENT, GL_FLOAT)
        }
        (DEPTH24_STENCIL8, GL_DEPTH_STENCIL) => (
            mapped_i,
            GL_DEPTH_STENCIL,
            0x84FA, /* GL_UNSIGNED_INT_24_8 */
        ),
        _ => (mapped_i, format, ty),
    }
}

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

// ---------------------------------------------------------------------------------------------
// ES 3.0 format conformance
//
// Desktop GL ignores the (format, type) pair of a `glTexImage2D` call that carries no pixel
// data, so shader loaders (Iris/OptiFine) allocate render targets as e.g.
// `(GL_RGBA16F, GL_RGBA, GL_UNSIGNED_BYTE, NULL)`. ES 3.0 validates the pair even for NULL
// data and raises GL_INVALID_OPERATION, so the texture never gets storage and the framebuffer
// later reports GL_FRAMEBUFFER_INCOMPLETE_ATTACHMENT (0x8CD6 / 36054).
// ---------------------------------------------------------------------------------------------

/// What the device can render to, read from the capability probe.
#[derive(Clone, Copy, Debug)]
pub struct RenderCaps {
    /// `GL_EXT_color_buffer_float` (or ES 3.2): R11F_G11F_B10F and 32F targets.
    pub float_rt: bool,
    /// 16F targets (`GL_EXT_color_buffer_half_float` or `GL_EXT_color_buffer_float`).
    pub half_float_rt: bool,
    /// `GL_EXT_texture_norm16`: R16 / RG16 / RGBA16 exist and are renderable.
    pub norm16: bool,
}

impl Default for RenderCaps {
    /// Used before a context exists: assume nothing needs downgrading.
    fn default() -> Self {
        RenderCaps { float_rt: true, half_float_rt: true, norm16: false }
    }
}

pub const GL_RED_INTEGER: u32 = 0x8D94;
pub const GL_RG_INTEGER: u32 = 0x8228;
pub const GL_RGB_INTEGER: u32 = 0x8D98;
pub const GL_RGBA_INTEGER: u32 = 0x8D99;
pub const GL_DEPTH_STENCIL: u32 = 0x84F9;
const GL_BYTE: u32 = 0x1400;
const GL_SHORT: u32 = 0x1402;
const GL_INT: u32 = 0x1404;
const GL_UNSIGNED_INT_24_8: u32 = 0x84FA;
const GL_FLOAT_32_UNSIGNED_INT_24_8_REV: u32 = 0x8DAD;
const GL_UNSIGNED_INT_2_10_10_10_REV: u32 = 0x8368;
const GL_UNSIGNED_INT_10F_11F_11F_REV: u32 = 0x8C3B;
const GL_UNSIGNED_INT_5_9_9_9_REV: u32 = 0x8C3E;

/// Closest ES-renderable sized format for a desktop sized internal format, or the input
/// unchanged when ES can already use it. Only call this for allocations that carry no pixel
/// data (render targets, `glTexStorage*`): channel-count changes would corrupt real uploads.
pub fn es_renderable_internal(internal: u32, caps: RenderCaps) -> u32 {
    let f16 = |wide: u32, narrow: u32| if caps.half_float_rt { wide } else { narrow };
    match internal {
        // RGB float: ES 3.0 can sample these but never render to them.
        0x881B /* RGB16F */ => f16(0x881A, 0x8058),
        0x8815 /* RGB32F */ => 0x8814,
        // 16-bit normalised: only exists with EXT_texture_norm16, otherwise use half float.
        0x822A /* R16 */ => if caps.norm16 { internal } else { f16(0x822D, 0x8229) },
        0x822C /* RG16 */ => if caps.norm16 { internal } else { f16(0x822F, 0x822B) },
        0x8054 /* RGB16 */ | 0x805B /* RGBA16 */ => {
            if caps.norm16 { 0x805B } else { f16(0x881A, 0x8058) }
        }
        // Float colour targets on a device without the extension would be incomplete.
        0x8C3A /* R11F_G11F_B10F */ if !caps.float_rt => f16(0x881A, 0x8058),
        0x822D /* R16F */ if !caps.half_float_rt => 0x8229,
        0x822F /* RG16F */ if !caps.half_float_rt => 0x822B,
        0x881A /* RGBA16F */ if !caps.half_float_rt => 0x8058,
        // Legacy desktop-only sized colour formats.
        0x8052 /* RGB10 */ | 0x8053 /* RGB12 */ | 0x805A /* RGBA12 */ => 0x8059,
        0x8055 /* RGBA2 */ | 0x804F /* RGB4 */ | 0x8050 /* RGB5 */ => 0x8058,
        // RGB integer formats are not renderable in ES 3.0.
        0x8D7D /* RGB8UI  */ => 0x8D7C,
        0x8D77 /* RGB16UI */ => 0x8D76,
        0x8D71 /* RGB32UI */ => 0x8D70,
        0x8D8F /* RGB8I   */ => 0x8D8E,
        0x8D89 /* RGB16I  */ => 0x8D88,
        0x8D83 /* RGB32I  */ => 0x8D82,
        other => other,
    }
}

/// The (format, type) pair OpenGL ES 3.0 accepts for a sized internal format (spec table
/// 3.2). `None` for unsized or unknown formats, which are left to the caller.
pub fn es_canonical_pair(internal: u32) -> Option<(u32, u32)> {
    let p = match internal {
        0x8229 /* R8 */ => (GL_RED, GL_UNSIGNED_BYTE),
        0x822B /* RG8 */ => (GL_RG, GL_UNSIGNED_BYTE),
        0x8051 | 0x8C41 /* RGB8, SRGB8 */ => (GL_RGB, GL_UNSIGNED_BYTE),
        0x8058 | 0x8C43 /* RGBA8, SRGB8_ALPHA8 */ => (GL_RGBA, GL_UNSIGNED_BYTE),
        0x8F94 /* R8_SNORM */ => (GL_RED, GL_BYTE),
        0x8F95 /* RG8_SNORM */ => (GL_RG, GL_BYTE),
        0x8F96 /* RGB8_SNORM */ => (GL_RGB, GL_BYTE),
        0x8F97 /* RGBA8_SNORM */ => (GL_RGBA, GL_BYTE),
        0x8D62 /* RGB565 */ => (GL_RGB, GL_UNSIGNED_SHORT_5_6_5),
        0x8056 /* RGBA4 */ => (GL_RGBA, GL_UNSIGNED_SHORT_4_4_4_4),
        0x8057 /* RGB5_A1 */ => (GL_RGBA, GL_UNSIGNED_SHORT_5_5_5_1),
        0x8059 /* RGB10_A2 */ => (GL_RGBA, GL_UNSIGNED_INT_2_10_10_10_REV),
        0x822D | 0x822E /* R16F, R32F */ => (GL_RED, GL_FLOAT),
        0x822F | 0x8230 /* RG16F, RG32F */ => (GL_RG, GL_FLOAT),
        0x881B | 0x8815 /* RGB16F, RGB32F */ => (GL_RGB, GL_FLOAT),
        0x881A | 0x8814 /* RGBA16F, RGBA32F */ => (GL_RGBA, GL_FLOAT),
        0x8C3A /* R11F_G11F_B10F */ => (GL_RGB, GL_UNSIGNED_INT_10F_11F_11F_REV),
        0x8C3D /* RGB9_E5 */ => (GL_RGB, GL_UNSIGNED_INT_5_9_9_9_REV),
        // EXT_texture_norm16
        0x822A => (GL_RED, GL_UNSIGNED_SHORT),
        0x822C => (GL_RG, GL_UNSIGNED_SHORT),
        0x8054 => (GL_RGB, GL_UNSIGNED_SHORT),
        0x805B => (GL_RGBA, GL_UNSIGNED_SHORT),
        // integer formats
        0x8232 => (GL_RED_INTEGER, GL_UNSIGNED_BYTE),
        0x8238 => (GL_RG_INTEGER, GL_UNSIGNED_BYTE),
        0x8D7D => (GL_RGB_INTEGER, GL_UNSIGNED_BYTE),
        0x8D7C => (GL_RGBA_INTEGER, GL_UNSIGNED_BYTE),
        0x8231 => (GL_RED_INTEGER, GL_BYTE),
        0x8237 => (GL_RG_INTEGER, GL_BYTE),
        0x8D8F => (GL_RGB_INTEGER, GL_BYTE),
        0x8D8E => (GL_RGBA_INTEGER, GL_BYTE),
        0x8234 => (GL_RED_INTEGER, GL_UNSIGNED_SHORT),
        0x823A => (GL_RG_INTEGER, GL_UNSIGNED_SHORT),
        0x8D77 => (GL_RGB_INTEGER, GL_UNSIGNED_SHORT),
        0x8D76 => (GL_RGBA_INTEGER, GL_UNSIGNED_SHORT),
        0x8233 => (GL_RED_INTEGER, GL_SHORT),
        0x8239 => (GL_RG_INTEGER, GL_SHORT),
        0x8D89 => (GL_RGB_INTEGER, GL_SHORT),
        0x8D88 => (GL_RGBA_INTEGER, GL_SHORT),
        0x8236 => (GL_RED_INTEGER, GL_UNSIGNED_INT),
        0x823C => (GL_RG_INTEGER, GL_UNSIGNED_INT),
        0x8D71 => (GL_RGB_INTEGER, GL_UNSIGNED_INT),
        0x8D70 => (GL_RGBA_INTEGER, GL_UNSIGNED_INT),
        0x8235 => (GL_RED_INTEGER, GL_INT),
        0x823B => (GL_RG_INTEGER, GL_INT),
        0x8D83 => (GL_RGB_INTEGER, GL_INT),
        0x8D82 => (GL_RGBA_INTEGER, GL_INT),
        0x906F /* RGB10_A2UI */ => (GL_RGBA_INTEGER, GL_UNSIGNED_INT_2_10_10_10_REV),
        // depth / depth-stencil
        0x81A5 /* DEPTH_COMPONENT16 */ => (GL_DEPTH_COMPONENT, GL_UNSIGNED_SHORT),
        0x81A6 /* DEPTH_COMPONENT24 */ => (GL_DEPTH_COMPONENT, GL_UNSIGNED_INT),
        0x8CAC /* DEPTH_COMPONENT32F */ => (GL_DEPTH_COMPONENT, GL_FLOAT),
        0x88F0 /* DEPTH24_STENCIL8 */ => (GL_DEPTH_STENCIL, GL_UNSIGNED_INT_24_8),
        0x8CAD /* DEPTH32F_STENCIL8 */ => (GL_DEPTH_STENCIL, GL_FLOAT_32_UNSIGNED_INT_24_8_REV),
        _ => return None,
    };
    Some(p)
}

/// Full conformance for a `glTexImage*` call.
///
/// With real pixel data the caller's pair is kept (after the existing BGRA/depth mapping): the
/// bytes already have a layout. With `NULL` data the pair carries no information, so it is
/// replaced by the pair ES accepts, and unrenderable internal formats are swapped for the
/// nearest renderable one.
pub fn conform_upload(
    internal: i32,
    format: u32,
    ty: u32,
    has_data: bool,
    caps: RenderCaps,
) -> (i32, u32, u32) {
    let (mapped, f, t) = map_upload_format(internal, format, ty);
    if has_data {
        return (mapped, f, t);
    }
    let sub = es_renderable_internal(mapped as u32, caps);
    match es_canonical_pair(sub) {
        Some((cf, ct)) => (sub as i32, cf, ct),
        None => (sub as i32, f, t),
    }
}

/// Internal format for storage calls with no pixel data (`glTexStorage*`, renderbuffers,
/// multisample storage).
pub fn map_storage_internal(internal: u32, caps: RenderCaps) -> u32 {
    let i = map_internal_format(internal as i32, 0, 0) as u32;
    es_renderable_internal(i, caps)
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
    fn depth_formats_are_paired_with_a_type_es_accepts() {
        // Minecraft allocates the window depth attachment as
        // (GL_DEPTH_COMPONENT24, GL_DEPTH_COMPONENT, GL_FLOAT), which ES 3.0 rejects
        // because a sized depth internal format must carry a matching type. This pairing
        // bug was the "OpenGL error 1282" during framebuffer setup.
        let (i, f, t) = map_upload_format(GL_DEPTH_COMPONENT24, GL_DEPTH_COMPONENT, GL_FLOAT);
        assert_eq!((i, f, t), (GL_DEPTH_COMPONENT24, GL_DEPTH_COMPONENT, GL_UNSIGNED_INT));

        let (i, _f, t) = map_upload_format(GL_DEPTH_COMPONENT as i32, GL_DEPTH_COMPONENT, GL_FLOAT);
        assert_eq!((i, t), (GL_DEPTH_COMPONENT32F, GL_FLOAT));

        let (i, _f, t) = map_upload_format(GL_DEPTH_COMPONENT as i32, GL_DEPTH_COMPONENT, GL_UNSIGNED_SHORT);
        assert_eq!((i, t), (GL_DEPTH_COMPONENT16, GL_UNSIGNED_SHORT));

        let (_i, _f, _t) = map_upload_format(0x84F9, 0x84F9, 0x84FA /* UNSIGNED_INT_24_8 */);
    }

    #[test]
    fn colour_formats_pass_through_unchanged() {
        let (i, f, t) = map_upload_format(GL_RGBA8, GL_RGBA, GL_UNSIGNED_BYTE);
        assert_eq!((i, f, t), (GL_RGBA8, GL_RGBA, GL_UNSIGNED_BYTE));
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

    #[test]
    fn iris_render_target_allocations_get_an_es_valid_pair() {
        let caps = RenderCaps::default();
        // Iris allocates colour targets as (internal, RGB(A), UNSIGNED_BYTE, NULL).
        let (i, f, t) = conform_upload(GL_RGBA16F, GL_RGBA, GL_UNSIGNED_BYTE, false, caps);
        assert_eq!((i, f, t), (GL_RGBA16F, GL_RGBA, GL_FLOAT));
        // RGB16F is not renderable in ES 3.0: widen to RGBA16F.
        let (i, f, t) = conform_upload(GL_RGB16F, GL_RGB, GL_UNSIGNED_BYTE, false, caps);
        assert_eq!((i, f, t), (GL_RGBA16F, GL_RGBA, GL_FLOAT));
        // RGBA16 (normalised) has no ES 3.0 equivalent without norm16.
        let (i, f, _) = conform_upload(0x805B, GL_RGBA, GL_UNSIGNED_SHORT, false, caps);
        assert_eq!((i, f), (GL_RGBA16F, GL_RGBA));
        let (i, f, t) = conform_upload(0x8C3A, GL_RGB, GL_UNSIGNED_BYTE, false, caps);
        assert_eq!((i, f, t), (0x8C3A, GL_RGB, 0x8C3B));
        let (i, f, t) = conform_upload(GL_RGBA8, GL_RGBA, GL_UNSIGNED_BYTE, false, caps);
        assert_eq!((i, f, t), (GL_RGBA8, GL_RGBA, GL_UNSIGNED_BYTE));
    }

    #[test]
    fn real_pixel_data_is_never_reinterpreted() {
        let caps = RenderCaps::default();
        let (i, f, t) = conform_upload(GL_RGB16F, GL_RGB, GL_FLOAT, true, caps);
        assert_eq!((i, f, t), (GL_RGB16F, GL_RGB, GL_FLOAT));
    }

    #[test]
    fn devices_without_float_targets_fall_back_to_rgba8() {
        let caps = RenderCaps { float_rt: false, half_float_rt: false, norm16: false };
        assert_eq!(es_renderable_internal(GL_RGBA16F as u32, caps), GL_RGBA8 as u32);
        assert_eq!(es_renderable_internal(0x8C3A, caps), GL_RGBA8 as u32);
        let (i, f, t) = conform_upload(GL_RGBA16F, GL_RGBA, GL_FLOAT, false, caps);
        assert_eq!((i, f, t), (GL_RGBA8, GL_RGBA, GL_UNSIGNED_BYTE));
    }

    #[test]
    fn depth_stencil_pairs() {
        let caps = RenderCaps::default();
        let (_, f, t) = conform_upload(GL_DEPTH24_STENCIL8, GL_DEPTH_STENCIL, GL_UNSIGNED_BYTE, false, caps);
        assert_eq!((f, t), (GL_DEPTH_STENCIL, 0x84FA));
        let (_, f, t) = conform_upload(GL_DEPTH32F_STENCIL8, GL_DEPTH_STENCIL, GL_UNSIGNED_BYTE, false, caps);
        assert_eq!((f, t), (GL_DEPTH_STENCIL, 0x8DAD));
    }
}
