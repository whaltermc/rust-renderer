//! Desktop GLSL → GLSL ES translation for Minecraft / LWJGL shaders.
//!
//! Handles the rewrites that vanilla 1.12–1.20 and common mods need most often.
//! Not a full GLSL compiler — geometry/tessellation/compute are rejected.

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
precision mediump sampler2DShadow;\n";

const PRECISION_100: &str = "precision highp float;\nprecision highp int;\n";

fn looks_like_fragment(src: &str) -> bool {
    src.contains("gl_FragColor")
        || src.contains("gl_FragData")
        || src.contains("gl_FragDepth")
        || (src.contains("out ")
            && !src.contains("gl_Position")
            && !src.contains("gl_PointSize"))
}

fn looks_like_geometry_or_tess(src: &str) -> bool {
    let lower = src.to_ascii_lowercase();
    lower.contains("#extension gl_ext_geometry_shader")
        || lower.contains("#extension gl_arb_geometry_shader")
        || lower.contains("#extension gl_arb_tessellation_shader")
        || lower.contains("layout(triangles)")
        || lower.contains("layout(points)")
        || lower.contains("layout(lines")
        || lower.contains("gl_in[")
        || src.contains("EmitVertex")
        || src.contains("EndPrimitive")
        || src.contains("gl_TessLevel")
}

/// Apply token-level desktop→ES rewrites on a single line (not comments).
fn rewrite_line_body(line: &str, use_300: bool, is_frag: bool, needs_frag_out: bool) -> String {
    let t = line.trim_start();
    let indent_len = line.len() - t.len();
    let indent = &line[..indent_len];

    // Skip pure comments / preprocessor (handled by caller for #version/#extension).
    if t.starts_with("//") {
        return line.to_string();
    }

    let mut s = line.to_string();

    if use_300 {
        // attribute / varying
        if let Some(rest) = t.strip_prefix("attribute ") {
            return format!("{indent}in {rest}");
        }
        if let Some(rest) = t.strip_prefix("varying ") {
            let kw = if is_frag { "in" } else { "out" };
            return format!("{indent}{kw} {rest}");
        }

        // texture builtins
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
            .replace("textureGrad(", "textureGrad("); // already ES3

        // matrix helpers removed in core / ES
        s = s
            .replace("ftransform()", "(gl_ModelViewProjectionMatrix * gl_Vertex)")
            .replace("gl_TextureMatrix[0]", "mat4(1.0)"); // weak fallback

        if needs_frag_out {
            s = s
                .replace("gl_FragColor", "rust_FragColor")
                .replace("gl_FragData[0]", "rust_FragColor");
        }

        // legacy fixed-function varyings — map common ones if present
        // (most MC shaders don't use these on 1.16+)
    } else {
        // GLSL 100 path: keep attribute/varying, rewrite texture only if needed
        s = s
            .replace("texture(", "texture2D("); // 100 uses texture2D
    }

    s
}

pub fn translate(src: &str) -> Result<String, String> {
    if looks_like_geometry_or_tess(src) {
        return Err(
            "geometry/tessellation shaders are not supported on GLES passthrough".into(),
        );
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
        return Ok(src.to_string());
    }
    if num > 330 {
        return Err(format!("GLSL {num} is not supported (maximum 330)"));
    }
    if num >= 400 {
        return Err(format!("GLSL {num} requires desktop features unavailable on GLES"));
    }

    let use_300 = num >= 130;
    let (header, precision) = if use_300 {
        ("#version 300 es", PRECISION_300)
    } else {
        ("#version 100", PRECISION_100)
    };

    let is_frag = looks_like_fragment(src);
    let needs_frag_out =
        use_300 && is_frag && (src.contains("gl_FragColor") || src.contains("gl_FragData[0]"));

    if src.contains("gl_FragData[") && !src.contains("gl_FragData[0]") {
        // any non-zero index → MRT
        if src.contains("gl_FragData[1]")
            || src.contains("gl_FragData[2]")
            || src.contains("gl_FragData[3]")
        {
            return Err("MRT gl_FragData[N>0] is not translated yet".into());
        }
    }

    let mut out = String::with_capacity(src.len() + 512);
    out.push_str(header);
    out.push('\n');

    let mut inserted_precision = false;
    let mut inserted_frag_out = false;

    for line in src.lines() {
        let t = line.trim_start();

        if t.starts_with("#version") {
            continue;
        }

        // Drop desktop-only extensions; keep ES ones if any.
        if t.starts_with("#extension") {
            let drop = t.contains("GL_ARB_")
                || t.contains("GL_NV_")
                || t.contains("GL_EXT_gpu_shader4")
                || t.contains("GL_EXT_geometry_shader")
                || t.contains("GL_ARB_separate_shader_objects")
                || t.contains("GL_ARB_explicit_attrib_location")
                || t.contains("GL_ARB_shading_language_420pack")
                || t.contains("GL_ARB_gpu_shader5");
            if drop {
                continue;
            }
        }

        // Insert precision (+ optional frag out) before first non-preprocessor statement.
        if !inserted_precision && !t.is_empty() && !t.starts_with('#') && !t.starts_with("//") {
            out.push_str(precision);
            if needs_frag_out && !inserted_frag_out {
                out.push_str("layout(location = 0) out vec4 rust_FragColor;\n");
                inserted_frag_out = true;
            }
            inserted_precision = true;
        }

        let rewritten = rewrite_line_body(line, use_300, is_frag, needs_frag_out);
        out.push_str(&rewritten);
        out.push('\n');
    }

    if !inserted_precision {
        out.push_str(precision);
        if needs_frag_out {
            out.push_str("layout(location = 0) out vec4 rust_FragColor;\n");
        }
    }

    // Final safety: leftover gl_FragData
    if out.contains("gl_FragData[") {
        return Err("unhandled gl_FragData reference after translation".into());
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
        assert!(o.contains("in vec3 p;"));
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
        assert!(o.contains("layout(location = 0) out vec4 rust_FragColor;"));
    }

    #[test]
    fn attribute_varying() {
        let vs = translate(
            "#version 120\nattribute vec3 pos;\nvarying vec2 uv;\nvoid main(){ gl_Position = vec4(pos,1.0); }\n",
        )
        .unwrap();
        // 120 → 100 keeps attribute/varying
        assert!(vs.starts_with("#version 100\n"));
        assert!(vs.contains("attribute vec3 pos;") || vs.contains("varying vec2 uv;"));

        let vs2 = translate(
            "#version 150\nattribute vec3 pos;\nvarying vec2 uv;\nvoid main(){ gl_Position = vec4(pos,1.0); }\n",
        )
        .unwrap();
        assert!(vs2.contains("in vec3 pos;"));
        assert!(vs2.contains("out vec2 uv;"));
    }

    #[test]
    fn rejects_geometry() {
        assert!(translate("#version 150\nlayout(triangles) in;\nvoid main(){ EmitVertex(); }\n").is_err());
    }

    #[test]
    fn rejects_high_version() {
        assert!(translate("#version 400\nvoid main(){}\n").is_err());
    }

    #[test]
    fn es_passthrough() {
        let s = "#version 300 es\nprecision highp float;\nvoid main(){}\n";
        assert_eq!(translate(s).unwrap(), s);
    }

    #[test]
    fn drops_arb_extension() {
        let o = translate(
            "#version 150\n#extension GL_ARB_explicit_attrib_location : enable\nvoid main(){}\n",
        )
        .unwrap();
        assert!(!o.contains("GL_ARB_"));
    }
}
