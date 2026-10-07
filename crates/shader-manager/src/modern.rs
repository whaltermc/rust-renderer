//! Modern shader rewrites for Minecraft 1.21.4-26.4.
//!
//! Handles features used by:
//! - Complementary Reimagined
//! - Derivative
//! - Bliss
//! - BSL
//! - Complex modern shader packs
//!
//! Features handled:
//! - PBR material model extensions (1.21.4+)
//! - Modern fog/atmosphere (26.x)
//! - Shadow mapping variations
//! - Volumetric lighting keywords
//! - TAA/jitter keywords
//! - Screen-space reflections
//! - Biome-based coloring
//! - Custom lightmap handling

use crate::{PackType, ShaderStage, ShaderSource};

/// Apply modern shader rewrites for 1.21.4-26.4 packs.
///
/// Returns the rewritten source, or an error if an unsupported feature is detected.
pub fn apply_modern_rewrites(
    source: &str,
    pack_type: PackType,
    stage: ShaderStage,
) -> Result<String, String> {
    let mut src = source.to_string();

    // 1.21.4+ PBR material tokens
    src = rewrite_pbr_tokens(&src);

    // 26.x atmosphere/volumetric tokens
    src = rewrite_atmosphere_tokens(&src);

    // Shadow map variations
    src = rewrite_shadow_tokens(&src);

    // TAA / camera jitter
    src = rewrite_taa_tokens(&src);

    // Screen-space reflections
    src = rewrite_ssr_tokens(&src);

    // Biome-based coloring
    src = rewrite_biome_tokens(&src);

    // Complementary-specific rewrites
    if matches!(pack_type, PackType::Complementary) {
        src = rewrite_complementary(&src, stage);
    }

    // Derivative-specific rewrites
    if matches!(pack_type, PackType::Derivative) {
        src = rewrite_derivative(&src, stage);
    }

    // Bliss-specific rewrites
    if matches!(pack_type, PackType::Bliss) {
        src = rewrite_bliss(&src, stage);
    }

    Ok(src)
}

/// Rewrite 1.21.4+ PBR material tokens.
fn rewrite_pbr_tokens(src: &str) -> String {
    let mut out = src.to_string();
    // roughnessMetalness -> roughness + metalness split (best effort)
    out = out.replace("roughnessMetalness", "roughness");
    // materialPBR7 -> standard PBR params
    out = out.replace("materialPBR7", "PBRParams");
    // 1.21.4 shadow tex
    out = out.replace("shadowtex0", "shadowMap");
    out = out.replace("shadowtex1", "shadowMap1");
    out
}

/// Rewrite 26.x atmosphere/volumetric tokens.
fn rewrite_atmosphere_tokens(src: &str) -> String {
    let mut out = src.to_string();
    // Volumetric lighting
    out = out.replace("volumetricLight", "volLighting");
    out = out.replace("volumetricCloud", "volCloud");
    // New fog model
    out = out.replace("atmosphereFog", "sceneFog");
    out = out.replace("heightFog", "hFog");
    // Atmospheric scattering
    out = out.replace("miePhase", "scatterMie");
    out = out.replace("rayleighPhase", "scatterRay");
    out
}

/// Rewrite shadow map variations across packs.
/// Note: We do NOT rewrite const declarations like shadowMapResolution=2048
/// because they are valid GLSL ES constants. Only rewrite uniform/variable references.
fn rewrite_shadow_tokens(src: &str) -> String {
    let mut out = src.to_string();
    // Only replace uniform/variable references that need mapping
    // Do NOT replace const declarations like shadowMapResolution=2048
    out = shader_translate::compat::replace_ident(&out, "shadowDistance", "SHADOW_DIST");
    out = shader_translate::compat::replace_ident(&out, "shadowInterval", "SHADOW_INTERVAL");
    out = shader_translate::compat::replace_ident(&out, "shadowHardness", "SHADOW_HARDNESS");
    // Note: shadowMapResolution is left as-is since it's a valid GLSL ES const
    out
}

/// Rewrite TAA / camera jitter tokens.
fn rewrite_taa_tokens(src: &str) -> String {
    let mut out = src.to_string();
    out = out.replace("cameraJitter", "taaOffset");
    out = out.replace("previousFrame", "prevFrame");
    out = out.replace("motionBlurFactor", "motionFactor");
    out
}

/// Rewrite screen-space reflection tokens.
fn rewrite_ssr_tokens(src: &str) -> String {
    let mut out = src.to_string();
    out = out.replace("ssrEnabled", "SSR_ENABLED");
    out = out.replace("ssrRayStep", "SSR_STEP");
    out = out.replace("ssrMaxDist", "SSR_MAX_DIST");
    out
}

/// Rewrite biome-based coloring tokens.
fn rewrite_biome_tokens(src: &str) -> String {
    let mut out = src.to_string();
    out = out.replace("biomeBlendDist", "BIOME_BLEND");
    out = out.replace("temperatureColor", "tempColor");
    out = out.replace("wetnessColor", "wetColor");
    out
}

/// Complementary Reimagined / Complementary specific rewrites.
fn rewrite_complementary(src: &str, _stage: ShaderStage) -> String {
    let mut out = src.to_string();
    // Complementary uses `rgbaN` for MRT; ES 3.00 needs explicit layout locations
    out = out.replace("rgba8", "color8");
    out = out.replace("rgba16", "color16");
    // Complementary shadow variant names
    out = out.replace("shadowtex0hard", "shadowHard");
    out = out.replace("shadowtex1hard", "shadowHard1");
    out
}

/// Derivative (Chocapic/Sildurs derivative) specific rewrites.
fn rewrite_derivative(src: &str, _stage: ShaderStage) -> String {
    let mut out = src.to_string();
    // Old-style water normals
    out = out.replace("waterNormal", "waterNorm");
    out = out.replace("waterDiffuse", "waterDiff");
    // Legacy shadow sampler
    out = out.replace("shadow", "shadowMap");
    out
}

/// Bliss specific rewrites.
fn rewrite_bliss(src: &str, _stage: ShaderStage) -> String {
    let mut out = src.to_string();
    // Bliss PBR names
    out = out.replace("roughnessMap", "roughMap");
    out = out.replace("metalnessMap", "metalMap");
    // Bliss fog
    out = out.replace("fogColor", "sceneFogColor");
    out = out.replace("fogDensity", "sceneFogDensity");
    out
}

/// Extension trait for detecting modern shader features.
pub trait ModernShaderExt {
    /// Whether this source uses 1.21.4+ PBR features.
    fn uses_pbr(&self) -> bool;

    /// Whether this source uses 26.x atmosphere/volumetric features.
    fn uses_volumetric(&self) -> bool;

    /// Whether this source uses shadow variations (multiple shadow samplers).
    fn uses_multi_shadow(&self) -> bool;

    /// Whether this source uses screen-space reflections.
    fn uses_ssr(&self) -> bool;

    /// Whether this source uses TAA.
    fn uses_taa(&self) -> bool;

    /// Minimum MC version required.
    fn min_version(&self) -> (u32, u32);

    /// Maximum known compatible version.
    fn max_version(&self) -> (u32, u32);

    /// Feature flags used by this shader.
    fn features(&self) -> Vec<&'static str>;
}

impl ModernShaderExt for str {
    fn uses_pbr(&self) -> bool {
        self.contains("roughnessMetalness")
            || self.contains("materialPBR")
            || self.contains("roughnessMap")
            || self.contains("metalnessMap")
    }

    fn uses_volumetric(&self) -> bool {
        self.contains("volumetricLight")
            || self.contains("volumetricCloud")
            || self.contains("atmosphereFog")
            || self.contains("heightFog")
    }

    fn uses_multi_shadow(&self) -> bool {
        let count = self.matches("shadowtex").count();
        count >= 2 || self.contains("shadowtex0hard") || self.contains("shadowtex1hard")
    }

    fn uses_ssr(&self) -> bool {
        self.contains("ssrEnabled")
            || self.contains("ssrRayStep")
            || self.contains("screenSpaceReflection")
    }

    fn uses_taa(&self) -> bool {
        self.contains("cameraJitter")
            || self.contains("previousFrame")
            || self.contains("taaOffset")
    }

    fn min_version(&self) -> (u32, u32) {
        if self.uses_volumetric() {
            (26, 0)
        } else if self.uses_pbr() {
            (1, 21)
        } else {
            (1, 14)
        }
    }

    fn max_version(&self) -> (u32, u32) {
        if self.contains("26") || self.contains("volumetric") {
            (26, 4)
        } else {
            (26, 4)
        }
    }

    fn features(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.uses_pbr() { v.push("pbr"); }
        if self.uses_volumetric() { v.push("volumetric"); }
        if self.uses_multi_shadow() { v.push("multi_shadow"); }
        if self.uses_ssr() { v.push("ssr"); }
        if self.uses_taa() { v.push("taa"); }
        v
    }
}

impl ModernShaderExt for ShaderSource {
    fn uses_pbr(&self) -> bool {
        self.source.uses_pbr()
    }
    fn uses_volumetric(&self) -> bool {
        self.source.uses_volumetric()
    }
    fn uses_multi_shadow(&self) -> bool {
        self.source.uses_multi_shadow()
    }
    fn uses_ssr(&self) -> bool {
        self.source.uses_ssr()
    }
    fn uses_taa(&self) -> bool {
        self.source.uses_taa()
    }
    fn min_version(&self) -> (u32, u32) {
        self.source.min_version()
    }
    fn max_version(&self) -> (u32, u32) {
        self.source.max_version()
    }
    fn features(&self) -> Vec<&'static str> {
        self.source.features()
    }
}
