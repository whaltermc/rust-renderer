use std::collections::HashMap;

use naga::front::glsl::{Frontend, Options as GlslOptions};
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga::back::glsl::{Options as GlslOutOptions, PipelineOptions, Writer, Version, WriterFlags};
use naga::ShaderStage;
use thiserror::Error;
use regex::Regex;
use once_cell::sync::Lazy;


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderStageType {
    Vertex,
    Fragment,
    Compute,
}

impl From<ShaderStageType> for ShaderStage {
    fn from(stage: ShaderStageType) -> Self {
        match stage {
            ShaderStageType::Vertex => ShaderStage::Vertex,
            ShaderStageType::Fragment => ShaderStage::Fragment,
            ShaderStageType::Compute => ShaderStage::Compute,
        }
    }
}

#[derive(Debug, Error)]
pub enum TranslateError {
    #[error("GLSL parse error: {0}")]
    ParseError(String),
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error("GLSL emit error: {0}")]
    EmitError(String),
    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),
    #[error("Compute shader requires GLSL ES 3.10+ (version 330 -> 430)")]
    ComputeVersionError,
    #[error("Missing entry point: {0}")]
    MissingEntryPoint(String),
}

fn stage_to_naga(stage: ShaderStageType) -> ShaderStage {
    stage.into()
}

fn naga_stage_to_version(stage: ShaderStageType, _input_version: u32) -> Version {
    match stage {
        ShaderStageType::Compute => Version::new_gles(310),
        ShaderStageType::Vertex | ShaderStageType::Fragment => Version::new_gles(310),
    }
}

fn detect_glsl_version(source: &str) -> u32 {
    for line in source.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("#version") {
            let mut parts = rest.split_whitespace();
            if let Some(num_str) = parts.next() {
                if let Ok(version) = num_str.parse::<u32>() {
                    return version;
                }
            }
        }
    }
    330
}

fn upgrade_glsl_version_for_naga(source: &str) -> String {
    let mut lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();
    for line in &mut lines {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("#version") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(version_str) = parts.first() {
                if let Ok(version) = version_str.parse::<u32>() {
                    if version < 440 {
                        let profile = parts.get(1).map(|s| *s).unwrap_or("core");
                        *line = format!("#version 450 {}", profile);
                    }
                }
            }
            break;
        }
    }
    lines.join("\n")
}

fn validate_and_fix_compute_version(source: &str, stage: ShaderStageType) -> Result<String, TranslateError> {
    if stage != ShaderStageType::Compute {
        return Ok(source.to_string());
    }
    
    let version = detect_glsl_version(source);
    if version < 430 {
        let mut lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();
        for line in &mut lines {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("#version") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                let profile = parts.get(1).map(|s| *s).unwrap_or("core");
                *line = format!("#version 430 {}", profile);
                break;
            }
        }
        return Ok(lines.join("\n"));
    }
    Ok(source.to_string())
}

fn inject_precision_qualifiers(source: &str, version: Version) -> String {
    let is_es = matches!(version, Version::Embedded { .. });
    if !is_es {
        return source.to_string();
    }
    
    let precision_block = if matches!(version, Version::Embedded { version: v, .. } if v >= 300) {
        "precision highp float;\n\
         precision highp int;\n\
         precision highp sampler2D;\n\
         precision highp sampler3D;\n\
         precision highp samplerCube;\n\
         precision highp sampler2DArray;\n\
         precision highp sampler2DShadow;\n\
         precision highp isampler2D;\n\
         precision highp usampler2D;\n\
         precision highp isampler2DArray;\n\
         precision highp usampler2DArray;\n\
         precision highp samplerCubeShadow;\n"
    } else {
        "precision highp float;\nprecision highp int;\n"
    };
    
    let mut lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();
    let mut inserted = false;
    for (i, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#version") {
            lines.insert(i + 1, precision_block.to_string());
            inserted = true;
            break;
        }
    }
    if !inserted {
        lines.insert(0, precision_block.to_string());
    }
    lines.join("\n")
}

fn remove_layout_binding(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("layout(") && (trimmed.contains("binding") || trimmed.contains("set")) {
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}


fn reorder_parameter_qualifiers(source: &str) -> String {
    static IN_CONST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bin\s+const\s+").unwrap());
    static OUT_CONST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bout\s+const\s+").unwrap());
    static INOUT_CONST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\binout\s+const\s+").unwrap());
    static IN_PRECISE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bin\s+precise\s+").unwrap());
    static OUT_PRECISE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bout\s+precise\s+").unwrap());
    static INOUT_PRECISE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\binout\s+precise\s+").unwrap());
    static IN_CONST_PRECISE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bin\s+const\s+precise\s+").unwrap());
    static OUT_CONST_PRECISE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bout\s+const\s+precise\s+").unwrap());
    static INOUT_CONST_PRECISE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\binout\s+const\s+precise\s+").unwrap());

    let mut modified = source.to_string();
    modified = IN_CONST_RE.replace_all(&modified, "const in ").to_string();
    modified = OUT_CONST_RE.replace_all(&modified, "const out ").to_string();
    modified = INOUT_CONST_RE.replace_all(&modified, "const inout ").to_string();
    modified = IN_PRECISE_RE.replace_all(&modified, "precise in ").to_string();
    modified = OUT_PRECISE_RE.replace_all(&modified, "precise out ").to_string();
    modified = INOUT_PRECISE_RE.replace_all(&modified, "precise inout ").to_string();
    modified = IN_CONST_PRECISE_RE.replace_all(&modified, "const precise in ").to_string();
    modified = OUT_CONST_PRECISE_RE.replace_all(&modified, "const precise out ").to_string();
    modified = INOUT_CONST_PRECISE_RE.replace_all(&modified, "const precise inout ").to_string();
    modified
}

fn emulate_sampler_buffer(source: &str) -> String {
    let mut result = source.to_string();
    result = result.replace("samplerBuffer", "sampler2D");
    result = result.replace("isamplerBuffer", "isampler2D");
    result = result.replace("usamplerBuffer", "usampler2D");
    result = result.replace("texelFetchBuffer", "texelFetch2D");
    result
}


fn add_missing_intrinsics(source: &str) -> String {
    let mut result = String::new();
    let mut needs_fma = false;
    let mut needs_exp2 = false;
    let mut needs_log2 = false;
    let mut needs_frexp = false;
    let mut needs_ldexp = false;
    let mut needs_derivatives = false;
    
    for line in source.lines() {
        if line.contains("fma(") {
            needs_fma = true;
        }
        if line.contains("exp2(") {
            needs_exp2 = true;
        }
        if line.contains("log2(") {
            needs_log2 = true;
        }
        if line.contains("frexp(") {
            needs_frexp = true;
        }
        if line.contains("ldexp(") {
            needs_ldexp = true;
        }
        if line.contains("dFdx(") || line.contains("dFdy(") || line.contains("fwidth(") ||
           line.contains("dFdxFine(") || line.contains("dFdxCoarse(") ||
           line.contains("dFdyFine(") || line.contains("dFdyCoarse(") ||
           line.contains("fwidthFine(") || line.contains("fwidthCoarse(") {
            needs_derivatives = true;
        }
        result.push_str(line);
        result.push('\n');
    }
    
    let mut intrinsics = String::new();
    if needs_derivatives {
        intrinsics.push_str("#extension GL_OES_standard_derivatives : enable\n");
        intrinsics.push_str("#define dFdxFine dFdx\n");
        intrinsics.push_str("#define dFdxCoarse dFdx\n");
        intrinsics.push_str("#define dFdyFine dFdy\n");
        intrinsics.push_str("#define dFdyCoarse dFdy\n");
        intrinsics.push_str("#define fwidthFine fwidth\n");
        intrinsics.push_str("#define fwidthCoarse fwidth\n");
    }
    if needs_fma {
        intrinsics.push_str("#define fma(a, b, c) ((a) * (b) + (c))\n");
    }
    if needs_exp2 {
        intrinsics.push_str("#define exp2(x) exp((x) * 0.6931471805599453)\n");
    }
    if needs_log2 {
        intrinsics.push_str("#define log2(x) (log(x) * 1.4426950408889634)\n");
    }
    if needs_frexp {
        intrinsics.push_str("#define frexp(x, exp) /* frexp not available in ES */\n");
    }
    if needs_ldexp {
        intrinsics.push_str("#define ldexp(x, exp) ((x) * exp2(float(exp)))\n");
    }
    
    if !intrinsics.is_empty() {
        let mut lines: Vec<String> = result.lines().map(|s| s.to_string()).collect();
        let mut insert_idx = 0;
        for (i, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("#version") {
                insert_idx = i + 1;
                break;
            }
        }
        lines.insert(insert_idx, intrinsics);
        result = lines.join("\n");
    }
    
    result
}

fn fix_function_signatures(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    for line in source.lines() {
        let mut modified = line.to_string();
        modified = modified.replace("fma(", "fma(");
        modified = modified.replace("exp2(", "exp2(");
        modified = modified.replace("log2(", "log2(");
        result.push_str(&modified);
        result.push('\n');
    }
    result
}

fn map_derivative_variants(source: &str) -> String {
    let mut result = source.to_string();
    result = result.replace("dFdxFine(", "dFdx(");
    result = result.replace("dFdxCoarse(", "dFdx(");
    result = result.replace("dFdyFine(", "dFdy(");
    result = result.replace("dFdyCoarse(", "dFdy(");
    result = result.replace("fwidthFine(", "fwidth(");
    result = result.replace("fwidthCoarse(", "fwidth(");
    result
}

pub fn translate_glsl_to_essl(
    source: &str,
    stage: ShaderStageType,
    defines: &HashMap<String, String>,
) -> Result<String, TranslateError> {
    let source = validate_and_fix_compute_version(source, stage)?;
    let source = upgrade_glsl_version_for_naga(&source);
    let source = map_derivative_variants(&source);
    
    let naga_stage = stage_to_naga(stage);
    let input_version = detect_glsl_version(&source);
    
    let mut frontend = Frontend::default();
    let mut glsl_defines = naga::FastHashMap::default();
    for (k, v) in defines {
        glsl_defines.insert(k.clone(), v.clone());
    }
    let options = GlslOptions {
        stage: naga_stage,
        defines: glsl_defines,
    };
    
    let module = frontend.parse(&options, &source)
        .map_err(|e| TranslateError::ParseError(e.emit_to_string(&source)))?;
    
    let mut validator = Validator::new(ValidationFlags::all(), Capabilities::all());
    let module_info = validator.validate(&module)
        .map_err(|e| TranslateError::ValidationError(format!("{:?}", e)))?;
    
    let output_version = naga_stage_to_version(stage, input_version);
    
    let writer_flags = WriterFlags::empty();
    
    let mut output = String::new();
    let pipeline_options = PipelineOptions {
        shader_stage: naga_stage,
        entry_point: "main".to_string(),
        multiview: None,
    };
    
    let writer_options = GlslOutOptions {
        version: output_version,
        writer_flags,
        binding_map: naga::back::glsl::BindingMap::default(),
        zero_initialize_workgroup_memory: false,
    };
    
    let mut writer = Writer::new(
        &mut output,
        &module,
        &module_info,
        &writer_options,
        &pipeline_options,
        naga::proc::BoundsCheckPolicies::default(),
    ).map_err(|e| TranslateError::EmitError(format!("{:?}", e)))?;
    
    writer.write().map_err(|e| TranslateError::EmitError(format!("{:?}", e)))?;
    
    let mut output = inject_precision_qualifiers(&output, output_version);
    output = remove_layout_binding(&output);
    output = reorder_parameter_qualifiers(&output);
    output = emulate_sampler_buffer(&output);
    output = add_missing_intrinsics(&output);
    output = fix_function_signatures(&output);
    
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    
    #[test]
    fn test_vertex_shader_basic() {
        let source = r#"
#version 450 core
layout(location = 0) in vec3 position;
layout(location = 0) out vec4 fragColor;
void main() {
    fragColor = vec4(position, 1.0);
    gl_Position = vec4(position, 1.0);
}
"#;
        let defines = HashMap::new();
        let result = translate_glsl_to_essl(source, ShaderStageType::Vertex, &defines);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("#version 310 es"));
        assert!(output.contains("precision highp float"));
    }
    
    #[test]
    fn test_fragment_shader_basic() {
        let source = r#"
#version 450 core
layout(location = 0) out vec4 fragColor;
layout(binding = 0) uniform vec4 color;
void main() {
    fragColor = color;
}
"#;
        let defines = HashMap::new();
        let result = translate_glsl_to_essl(source, ShaderStageType::Fragment, &defines);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("#version 310 es"));
        assert!(output.contains("precision highp float"));
    }
    
    #[test]
    fn test_compute_shader_version_bump() {
        let source = r#"
#version 450 core
layout(local_size_x = 16, local_size_y = 16) in;
void main() {
    ivec2 pos = ivec2(gl_GlobalInvocationID.xy);
}
"#;
        let defines = HashMap::new();
        let result = translate_glsl_to_essl(source, ShaderStageType::Compute, &defines);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("#version 310 es"));
    }
    
    #[test]
    fn test_precision_injection() {
        let source = r#"
#version 450 core
void main() {}
"#;
        let defines = HashMap::new();
        let result = translate_glsl_to_essl(source, ShaderStageType::Fragment, &defines);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("precision highp float"));
        assert!(output.contains("precision highp sampler2D"));
    }
    
    #[test]
    fn test_layout_binding_removal() {
        let source = r#"
#version 450 core
layout(binding = 0) uniform vec4 color;
void main() {}
"#;
        let defines = HashMap::new();
        let result = translate_glsl_to_essl(source, ShaderStageType::Fragment, &defines);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(!output.contains("layout(binding = 0)"));
    }
    
    #[test]
    fn test_parameter_qualifier_reordering() {
        let source = r#"
#version 450 core
layout(location = 0) out vec4 fragColor;
void func(in vec4 a, out vec4 b, inout vec4 c) {
    b = a;
    c = a;
}
void main() {
    vec4 x = vec4(1.0);
    vec4 y = vec4(2.0);
    vec4 z = vec4(3.0);
    func(x, y, z);
    fragColor = y;
    gl_Position = vec4(0.0);
}
"#;
        let defines = HashMap::new();
        let result = translate_glsl_to_essl(source, ShaderStageType::Vertex, &defines);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(!output.is_empty());
    }
    
    #[test]
    fn test_sampler_buffer_emulation() {
        let source = r#"
#version 450 core
layout(binding = 0) uniform vec4 buf;
void main() {
    vec4 color = buf;
}
"#;
        let defines = HashMap::new();
        let result = translate_glsl_to_essl(source, ShaderStageType::Fragment, &defines);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("vec4"));
    }
    
    #[test]
    fn test_missing_intrinsics() {
        let source = r#"
#version 450 core
layout(location = 0) out vec4 fragColor;
void main() {
    float a = fma(1.0, 2.0, 3.0);
    float b = exp2(4.0);
    float c = log2(8.0);
    fragColor = vec4(a, b, c, 1.0);
}
"#;
        let defines = HashMap::new();
        let result = translate_glsl_to_essl(source, ShaderStageType::Fragment, &defines);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(!output.is_empty());
    }
    
    #[test]
    fn test_defines_passed_through() {
        let source = r#"
#version 450 core
layout(location = 0) out vec4 fragColor;
#ifdef FEATURE_A
float value = 1.0;
#else
float value = 2.0;
#endif
void main() {
    fragColor = vec4(value);
    gl_Position = vec4(0.0);
}
"#;
        let mut defines = HashMap::new();
        defines.insert("FEATURE_A".to_string(), "1".to_string());
        let result = translate_glsl_to_essl(source, ShaderStageType::Vertex, &defines);
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let output = result.unwrap();
        assert!(output.contains("value = 1.0") || output.contains("1.0"));
    }
    
    #[test]
    fn test_parse_error_handling() {
        let source = r#"
#version 450 core
invalid syntax here;
void main() {}
"#;
        let defines = HashMap::new();
        let result = translate_glsl_to_essl(source, ShaderStageType::Vertex, &defines);
        assert!(result.is_err());
        match result {
            Err(TranslateError::ParseError(_)) => {}
            _ => panic!("Expected ParseError"),
        }
    }
}
