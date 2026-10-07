//! GLSL to GLSL ES 3.00 translation, via `naga`.
use naga::back::glsl;

/// Compiles one shader stage to GLSL ES 3.00.
#[cfg(feature = "spirv")]
pub fn translate(
    source: &str,
    stage: naga::ShaderStage,
    defines: &[(String, String)],
) -> Result<String, String> {
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
    .map_err(|e| format!("GLSL validation: {e:?}"))?;

    let mut output = String::new();
    
    let writer_options = glsl::Options {
        version: glsl::Version::Embedded { version: 300, is_webgl: false },
        writer_flags: glsl::WriterFlags::empty(),
        binding_map: Default::default(),
        zero_initialize_workgroup_memory: false,
    };
    
    let pipeline_options = glsl::PipelineOptions {
        entry_point: "main".into(),
        shader_stage: stage,
        multiview: None,
    };
    
    let mut writer = glsl::Writer::new(
        &mut output,
        &module,
        &info,
        &writer_options,
        &pipeline_options,
        naga::proc::BoundsCheckPolicies::default(),
    )
    .map_err(|e| format!("GLSL ES writer init: {e:?}"))?;

    writer.write().map_err(|e| format!("GLSL ES emit: {e:?}"))?;

    Ok(output)
}

/// Which shader stage a `glShaderType`-style enum refers to.
#[cfg(feature = "spirv")]
pub fn stage_for_gl_enum(kind: u32) -> Result<naga::ShaderStage, String> {
    match kind {
        0x8B31 => Ok(naga::ShaderStage::Vertex),
        0x8B30 => Ok(naga::ShaderStage::Fragment),
        0x8DD0 | 0x8DD1 | 0x8DD2 => Ok(naga::ShaderStage::Compute),
        0x88E4 => Err("geometry shaders are not supported by this compiler".into()),
        other => Err(format!("unknown shader type {other:#06x}")),
    }
}

#[cfg(all(test, feature = "spirv"))]
mod tests {
    use super::*;

    #[test]
    fn a_vertex_shader_translates_to_glsl_es() {
        let src = "#version 450\n\
layout(location = 0) in vec3 aPos;\n\
layout(location = 0) out vec4 vCol;\n\
void main() { vCol = vec4(aPos, 1.0); gl_Position = vec4(aPos, 1.0); }\n";
        let translated = translate(src, naga::ShaderStage::Vertex, &[]).expect("should translate");
        assert!(!translated.is_empty(), "GLSL ES module must not be empty");
        assert!(translated.contains("#version 300 es"), "expected GLSL ES 3.00 version directive");
    }

    #[test]
    fn a_fragment_shader_translates_to_glsl_es() {
        let src = "#version 450\n\
layout(location = 0) in vec4 vCol;\n\
layout(location = 0) out vec4 fragColor;\n\
void main() { fragColor = vCol; }\n";
        let translated = translate(src, naga::ShaderStage::Fragment, &[]).expect("should translate");
        assert!(!translated.is_empty(), "GLSL ES module must not be empty");
        assert!(translated.contains("#version 300 es"), "expected GLSL ES 3.00 version directive");
    }

    #[test]
    fn gl_enum_maps_to_a_stage() {
        assert_eq!(stage_for_gl_enum(0x8B31).unwrap(), naga::ShaderStage::Vertex);
        assert_eq!(stage_for_gl_enum(0x8B30).unwrap(), naga::ShaderStage::Fragment);
        assert!(stage_for_gl_enum(0xDEAD).is_err());
    }

    #[test]
    fn invalid_glsl_reports_where_it_failed() {
        let err = translate(
            "#version 450\nvoid main() { this is not glsl }",
            naga::ShaderStage::Vertex,
            &[],
        )
        .expect_err("should fail");
        assert!(err.starts_with("GLSL front-end"), "unexpected error: {err}");
    }
}
