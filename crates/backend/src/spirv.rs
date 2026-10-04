//! GLSL to SPIR-V, via `naga`.
//!
//! # Why trunk, not the published crate
//!
//! `naga 30.0.1` on crates.io does not compile with its `glsl-in` feature enabled -- the
//! front-end calls `apply_default_interpolation`, which no longer exists on the interpolation
//! enum. Trunk has that fixed and renamed the front-end API (`Options { stage, defines }`
//! instead of a `Version`). The dependency is pinned to an exact revision for that reason, and
//! is **off by default**: it resolves from git, so an offline or NDK-only build should not be
//! forced to fetch it.
//!
//! # What this does and does not buy
//!
//! It turns a shader into SPIR-V words. It does **not** make the Vulkan backend able to draw:
//! that still needs shader modules, pipeline layout, descriptor sets, render passes,
//! command buffers and a swapchain, which is why `can_render()` stays `false`. Turning a
//! "no compiler linked" error into real SPIR-V is one step, not the feature.

/// Compiles one shader stage to SPIR-V words.
///
/// `stage` selects the GLSL front-end's stage. `defines` are injected as preprocessor
/// definitions, the equivalent of `#define k v`.
#[cfg(feature = "spirv")]
pub fn compile(
    source: &str,
    stage: naga::ShaderStage,
    defines: &[(String, String)],
) -> Result<Vec<u32>, String> {
    let mut options = naga::front::glsl::Options::from(stage);
    for (k, v) in defines {
        options.defines.insert(k.clone(), v.clone());
    }
    let module = naga::front::glsl::Frontend::default()
        .parse(&options, source)
        .map_err(|e| format!("GLSL front-end: {e:?}"))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| format!("SPIR-V validation: {e:?}"))?;
    naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
        .map_err(|e| format!("SPIR-V emit: {e:?}"))
}

/// Which shader stage a `glShaderType`-style enum refers to.
///
/// GL uses `GL_VERTEX_SHADER` 0x8B31 and `GL_FRAGMENT_SHADER` 0x8B30; the translator in
/// gl-compat emits GLSL ES, but naga's front-end wants a stage, so the value has to be mapped.
#[cfg(feature = "spirv")]
pub fn stage_for_gl_enum(kind: u32) -> Result<naga::ShaderStage, String> {
    match kind {
        0x8B31 => Ok(naga::ShaderStage::Vertex),
        0x8B30 => Ok(naga::ShaderStage::Fragment),
        0x8DD0 | 0x8DD1 | 0x8DD2 => Ok(naga::ShaderStage::Compute),
        // naga has no Geometry stage, so GL_GEOMETRY_SHADER cannot be compiled this way.
        0x88E4 => Err("geometry shaders are not supported by this compiler".into()),
        other => Err(format!("unknown shader type {other:#06x}")),
    }
}

#[cfg(all(test, feature = "spirv"))]
mod tests {
    use super::*;

    #[test]
    fn a_vertex_shader_compiles_to_spirv() {
        let src = "#version 450\n\
layout(location = 0) in vec3 aPos;\n\
layout(location = 0) out vec4 vCol;\n\
void main() { vCol = vec4(aPos, 1.0); gl_Position = vec4(aPos, 1.0); }\n";
        let words = compile(src, naga::ShaderStage::Vertex, &[]).expect("should compile");
        assert!(!words.is_empty(), "SPIR-V module must not be empty");
        // A SPIR-V module starts with the magic word 0x07230203.
        assert_eq!(
            words[0], 0x0723_0203,
            "expected a SPIR-V magic number, got {:#010x}",
            words[0]
        );
    }

    #[test]
    fn gl_enum_maps_to_a_stage() {
        assert_eq!(stage_for_gl_enum(0x8B31).unwrap(), naga::ShaderStage::Vertex);
        assert_eq!(stage_for_gl_enum(0x8B30).unwrap(), naga::ShaderStage::Fragment);
        assert!(stage_for_gl_enum(0xDEAD).is_err());
    }

    #[test]
    fn invalid_glsl_reports_where_it_failed() {
        let err = compile("#version 450\nvoid main() { this is not glsl }", naga::ShaderStage::Vertex, &[])
            .expect_err("should fail");
        assert!(err.starts_with("GLSL front-end"), "unexpected error: {err}");
    }
}
