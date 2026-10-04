//! Desktop GLSL → GLSL ES translation for Minecraft / LWJGL shaders.
//!
//! Goal: never hard-fail on common vanilla/mod shaders. Prefer best-effort rewrite so
//! the driver reports a real compile error instead of an empty shader (which freezes
//! loading screens). Geometry/tessellation/compute still rejected.

const PRECISION_300: &str = "\
precision highp float;\n\
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
precision highp samplerCubeShadow;\n";

const PRECISION_100: &str = "precision highp float;\nprecision highp int;\n";


/// Rewrites GLSL 3.30 desktop qualifiers/features that do not exist in GLSL ES 3.00.
/// This deliberately stays lexical rather than using a full GLSL parser: Minecraft's
/// generated shaders are regular and preserving formatting makes driver logs useful.
fn rewrite_es300_tokens(mut s: String) -> String {
    // ES 3.00 has no interpolation qualifier named `noperspective`.
    s = s.replace("noperspective ", "");
    // `centroid` and `sample` are supported by ES3 where available; keep them.

    // Desktop layout qualifiers that are not legal in ES 3.00. Explicit attribute and
    // fragment locations remain valid and are intentionally preserved.
    let mut out = String::with_capacity(s.len());
    for line in s.lines() {
        let mut l = line.to_string();
        // Remove layout(binding = N), layout(index = N), and layout(component = N).
        // These are common in desktop shader generators but require newer GLSL ES.
        loop {
            let before = l.clone();
            l = strip_layout_key(&l, "binding");
            l = strip_layout_key(&l, "index");
            l = strip_layout_key(&l, "component");
            if l == before { break; }
        }
        // GLSL desktop permits `layout(location=0, binding=1)`; after removing binding
        // clean up a dangling comma before `)`.
        l = l.replace(", )", ")").replace("(,", "(");
        out.push_str(&l);
        out.push('\n');
    }
    s = out;

    // Desktop double types are not available in ES 3.00. Minecraft rarely needs true
    // 64-bit shader arithmetic, so map them to the closest 32-bit representation.
    for (a,b) in [
        ("dvec2", "vec2"), ("dvec3", "vec3"), ("dvec4", "vec4"),
        ("dmat2", "mat2"), ("dmat3", "mat3"), ("dmat4", "mat4"),
        ("double", "float"),
    ] { s = s.replace(a,b); }

    // Desktop-only builtins with straightforward ES equivalents.
    s
}

fn strip_layout_key(line: &str, key: &str) -> String {
    let needle = format!("{} =", key);
    let mut out = line.to_string();
    while let Some(pos) = out.find(&needle) {
        let start = out[..pos].rfind(',').map(|p| p + 1)
            .or_else(|| out[..pos].rfind('(').map(|p| p + 1));
        let Some(start) = start else { break; };
        let tail = &out[pos + needle.len()..];
        let end_rel = tail.find(',').or_else(|| tail.find(')'));
        let Some(end_rel) = end_rel else { break; };
        let end = pos + needle.len() + end_rel + if tail.as_bytes()[end_rel] == b',' { 1 } else { 0 };
        out.replace_range(start..end, "");
    }
    out
}

fn looks_like_fragment(src: &str) -> bool {
    src.contains("gl_FragColor")
        || src.contains("gl_FragData")
        || src.contains("gl_FragDepth")
        || (src.contains("out ")
            && !src.contains("gl_Position")
            && !src.contains("gl_PointSize"))
}

fn looks_like_compute(src: &str) -> bool {
    src.contains("layout(local_size")
        || src.contains("#extension GL_ARB_compute_shader")
        || src.contains("#extension GL_ES_compute")
}

fn looks_like_geometry_or_tess(src: &str) -> bool {
    // Be strict: only true geometry/tess markers, not generic `layout(points)`.
    src.contains("EmitVertex")
        || src.contains("EndPrimitive")
        || src.contains("gl_TessLevel")
        || src.contains("gl_in[")
        || src.contains("#extension GL_ARB_geometry_shader")
        || src.contains("#extension GL_EXT_geometry_shader")
        || src.contains("#extension GL_OES_geometry_shader")
        || src.contains("#extension GL_ARB_tessellation_shader")
        || src.contains("layout(triangles) in")
        || src.contains("layout(triangle_strip) out")
        || src.contains("layout(points) in;")
        || src.contains("layout(lines) in")
        || src.contains("layout(lines_adjacency)")
        || src.contains("layout(triangles_adjacency)")
}

fn rewrite_line_body(line: &str, use_300: bool, is_frag: bool, needs_frag_out: bool) -> String {
    let t = line.trim_start();
    let indent_len = line.len() - t.len();
    let indent = &line[..indent_len];

    if t.starts_with("//") {
        return line.to_string();
    }

    let mut s = line.to_string();

    if use_300 {
        if let Some(rest) = t.strip_prefix("attribute ") {
            return format!("{indent}in {rest}");
        }
        if let Some(rest) = t.strip_prefix("varying ") {
            let kw = if is_frag { "in" } else { "out" };
            return format!("{indent}{kw} {rest}");
        }

        s = s
            .replace("texture2DLod(", "textureLod(")
            .replace("texture2DProjLod(", "textureProjLod(")
            .replace("texture2DProj(", "textureProj(")
            .replace("texture2D(", "texture(")
            .replace("texture3DLod(", "textureLod(")
            .replace("texture3D(", "texture(")
            .replace("textureCubeLod(", "textureLod(")
            .replace("textureCube(", "texture(")
            .replace("texture2DArray(", "texture(")
            .replace("shadow2DProj(", "textureProj(")
            .replace("shadow2D(", "texture(")
            .replace("texture1D(", "texture(")
            .replace("texture1DLod(", "textureLod(");

        // Rare desktop helpers
        s = s
            .replace("ftransform()", "(gl_ModelViewProjectionMatrix * gl_Vertex)")
            .replace("gl_TextureMatrix[0]", "mat4(1.0)")
            .replace("gl_TextureMatrix[1]", "mat4(1.0)")
            .replace("gl_ModelViewProjectionMatrix", "mat4(1.0)")
            .replace("gl_ModelViewMatrix", "mat4(1.0)")
            .replace("gl_ProjectionMatrix", "mat4(1.0)")
            .replace("gl_NormalMatrix", "mat3(1.0)");

        if needs_frag_out {
            s = s
                .replace("gl_FragColor", "rust_FragColor")
                .replace("gl_FragData[0]", "rust_FragColor");
        }

        // Remove `shared` qualifier on non-compute (invalid in ES VS/FS)
        if t.starts_with("shared ") {
            return format!("{indent}// stripped shared: {t}");
        }
    } else {
        s = s.replace("texture(", "texture2D(");
    }

    s
}

/// Best-effort translate. Only fails hard on geometry/tess/compute.
pub fn translate(src: &str) -> Result<String, String> {
    if looks_like_geometry_or_tess(src) {
        return Err("geometry/tessellation shaders are not supported on GLES passthrough".into());
    }
    if looks_like_compute(src) {
        return Err("compute shaders are not supported on GLES passthrough".into());
    }

    let mut version: Option<(u32, bool)> = None;
    for line in src.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("#version") {
            let mut it = rest.split_whitespace();
            let num = it.next().and_then(|n| n.parse::<u32>().ok());
            let es = it.next().map_or(false, |w| w == "es");
            if let Some(n) = num {
                version = Some((n, es));
            }
            break;
        }
    }

    let (num, es) = version.unwrap_or((110, false));
    if es {
        // Already ES — still inject precision if missing (some drivers want it).
        if !src.contains("precision ") && num >= 300 {
            let mut out = String::new();
            for line in src.lines() {
                out.push_str(line);
                out.push('\n');
                if line.trim_start().starts_with("#version") {
                    out.push_str(PRECISION_300);
                }
            }
            return Ok(out);
        }
        return Ok(src.to_string());
    }

    // Map any desktop version we can into ES 300 (or 100 for very old).
    let use_300 = num >= 130;
    let (header, precision) = if use_300 {
        ("#version 300 es", PRECISION_300)
    } else {
        ("#version 100", PRECISION_100)
    };

    let is_frag = looks_like_fragment(src);
    let needs_frag_out =
        use_300 && is_frag && (src.contains("gl_FragColor") || src.contains("gl_FragData[0]"));

    // Soft-handle MRT: map gl_FragData[1..] to same output (wrong but boots).
    let has_mrt = src.contains("gl_FragData[1]")
        || src.contains("gl_FragData[2]")
        || src.contains("gl_FragData[3]");

    let mut out = String::with_capacity(src.len() + 512);
    out.push_str(header);
    out.push('\n');

    let mut inserted_precision = false;

    for line in src.lines() {
        let t = line.trim_start();

        if t.starts_with("#version") {
            continue;
        }

        if t.starts_with("#extension") {
            // Drop desktop-only; keep harmless ES / require lines stripped.
            let drop = t.contains("GL_ARB_")
                || t.contains("GL_NV_")
                || t.contains("GL_AMD_")
                || t.contains("GL_EXT_gpu_shader4")
                || t.contains("GL_EXT_geometry_shader")
                || t.contains("GL_OES_geometry_shader")
                || t.contains("GL_ARB_separate_shader_objects")
                || t.contains("GL_ARB_explicit_attrib_location")
                || t.contains("GL_ARB_explicit_uniform_location")
                || t.contains("GL_ARB_shading_language_420pack")
                || t.contains("GL_ARB_gpu_shader5")
                || t.contains("GL_ARB_shader_bit_encoding")
                || t.contains("GL_ARB_shader_storage_buffer_object")
                || t.contains("GL_ARB_compute_shader");
            if drop {
                continue;
            }
        }

        if !inserted_precision && !t.is_empty() && !t.starts_with('#') && !t.starts_with("//") {
            out.push_str(precision);
            if needs_frag_out {
                out.push_str("layout(location = 0) out vec4 rust_FragColor;\n");
            }
            if has_mrt {
                // Declare extra outs so references can be rewritten softly.
                out.push_str("layout(location = 1) out vec4 rust_FragData1;\n");
                out.push_str("layout(location = 2) out vec4 rust_FragData2;\n");
                out.push_str("layout(location = 3) out vec4 rust_FragData3;\n");
            }
            inserted_precision = true;
        }

        let mut rewritten = rewrite_line_body(line, use_300, is_frag, needs_frag_out);
        if use_300 {
            rewritten = rewrite_es300_tokens(rewritten);
        }
        if has_mrt {
            rewritten = rewritten
                .replace("gl_FragData[1]", "rust_FragData1")
                .replace("gl_FragData[2]", "rust_FragData2")
                .replace("gl_FragData[3]", "rust_FragData3");
        }
        out.push_str(&rewritten);
        out.push('\n');
    }

    if !inserted_precision {
        out.push_str(precision);
        if needs_frag_out {
            out.push_str("layout(location = 0) out vec4 rust_FragColor;\n");
        }
    }

    // Leftover gl_FragData → primary out
    if out.contains("gl_FragData") {
        out = out.replace("gl_FragData[0]", "rust_FragColor");
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_150_header() {
        let o = translate("#version 150 core\nin vec3 p;\nvoid main(){ gl_Position = vec4(p,1); }\n")
            .unwrap();
        assert!(o.starts_with("#version 300 es\n"));
        assert!(o.contains("precision highp float;"));
    }

    #[test]
    fn texture2d_and_fragcolor() {
        let o = translate(
            "#version 150\nuniform sampler2D s;\nvoid main(){ gl_FragColor = texture2D(s, vec2(0.)); }\n",
        )
        .unwrap();
        assert!(o.contains("texture(s,"));
        assert!(!o.contains("texture2D("));
        assert!(o.contains("rust_FragColor"));
    }

    #[test]
    fn high_version_allowed() {
        let o = translate("#version 410 core\nvoid main(){ gl_Position = vec4(0); }\n").unwrap();
        assert!(o.starts_with("#version 300 es\n"));
    }

    #[test]
    fn rejects_geometry() {
        assert!(translate("#version 150\nvoid main(){ EmitVertex(); }\n").is_err());
    }

    #[test]
    fn es_passthrough() {
        let s = "#version 300 es\nprecision highp float;\nvoid main(){}\n";
        assert_eq!(translate(s).unwrap(), s);
    }

    fn desktop_330_layouts_and_double_are_rewritten() {
        let o = translate("#version 330 core\nlayout(location=0, binding=2) in dvec3 p;\nlayout(location=0) out vec4 c;\nvoid main(){ c=vec4(p); }\n").unwrap();
        assert!(o.starts_with("#version 300 es\n"));
        assert!(!o.contains("binding"));
        assert!(o.contains("in vec3 p"));
    }

    #[test]
    fn noperspective_is_removed() {
        let o = translate("#version 330 core\nnoperspective in vec2 uv;\nvoid main(){gl_Position=vec4(0);}").unwrap();
        assert!(!o.contains("noperspective"));
    }
}
