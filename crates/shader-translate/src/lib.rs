//! Text-level GLSL rewriting from desktop GLSL (1.10–3.30) to GLSL ES (1.00 / 3.00).
//!
//! Not a full compiler. Handles the transformations Minecraft / LWJGL shaders need most:
//! version header, precision, texture2D→texture, gl_FragColor, attribute/varying, ARB
//! extensions. Anything beyond that is reported rather than silently passed through.

const PRECISION_300: &str = "precision highp float;\nprecision highp int;\n\
precision highp sampler2D;\nprecision highp sampler3D;\nprecision highp samplerCube;\n\
precision highp sampler2DArray;\nprecision highp isampler2D;\nprecision highp usampler2D;\n\
precision highp sampler2DShadow;\n";
const PRECISION_100: &str = "precision highp float;\nprecision highp int;\n";

/// Heuristic: vertex shaders usually write `gl_Position`; fragment shaders write
/// `gl_FragColor` / `gl_FragData` / declare `out` colour.
fn looks_like_fragment(src: &str) -> bool {
    src.contains("gl_FragColor")
        || src.contains("gl_FragData")
        || src.contains("gl_FragDepth")
        || (src.contains("out ") && !src.contains("gl_Position"))
}

pub fn translate(src: &str) -> Result<String, String> {
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
    // No #version means GLSL 1.10 by the spec.
    let (num, es) = version.unwrap_or((110, false));
    if es {
        return Ok(src.to_string());
    }
    if num > 330 {
        return Err(format!("GLSL {num} is not supported (maximum 330)"));
    }

    let use_300 = num >= 130;
    let (header, precision) = if use_300 {
        ("#version 300 es", PRECISION_300)
    } else {
        ("#version 100", PRECISION_100)
    };

    let is_frag = looks_like_fragment(src);
    let needs_frag_out = use_300 && is_frag && (src.contains("gl_FragColor") || src.contains("gl_FragData"));

    let mut out = String::with_capacity(src.len() + 512);
    out.push_str(header);
    out.push('\n');
    let mut inserted = false;
    let mut frag_out_inserted = false;

    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("#version") {
            continue;
        }
        // Desktop-only extensions are invalid in ES.
        if t.starts_with("#extension")
            && (t.contains("GL_ARB_")
                || t.contains("GL_EXT_gpu_shader4")
                || t.contains("GL_NV_"))
        {
            continue;
        }

        if !inserted && !t.is_empty() && !t.starts_with('#') && !t.starts_with("//") {
            out.push_str(precision);
            if needs_frag_out && !frag_out_inserted {
                out.push_str("out vec4 rust_FragColor;\n");
                frag_out_inserted = true;
            }
            inserted = true;
        }

        let mut rewritten = line.to_string();

        if use_300 {
            // Legacy texture lookups → modern texture().
            rewritten = rewritten
                .replace("texture2DLod(", "textureLod(")
                .replace("texture2DProj(", "textureProj(")
                .replace("texture2D(", "texture(")
                .replace("textureCube(", "texture(")
                .replace("texture3D(", "texture(")
                .replace("texture2DArray(", "texture(")
                .replace("shadow2D(", "texture(")
                .replace("shadow2DProj(", "textureProj(");

            // attribute / varying → in / out (stage-dependent for varying).
            rewritten = rewrite_attr_varying(&rewritten, is_frag);

            if needs_frag_out {
                rewritten = rewritten
                    .replace("gl_FragColor", "rust_FragColor")
                    .replace("gl_FragData[0]", "rust_FragColor");
            }

            // gl_FragData[N] for N>0 is multi-render-target; leave a clear comment marker
            // rather than silently breaking.
            if rewritten.contains("gl_FragData[") {
                return Err(
                    "gl_FragData[N] (MRT) is not fully translated yet; use single colour output"
                        .into(),
                );
            }
        }

        out.push_str(&rewritten);
        out.push('\n');
    }

    if !inserted {
        out.push_str(precision);
        if needs_frag_out {
            out.push_str("out vec4 rust_FragColor;\n");
        }
    }

    Ok(out)
}

/// Rewrite a single line's `attribute` / `varying` keywords for GLSL ES 3.00.
fn rewrite_attr_varying(line: &str, is_frag: bool) -> String {
    let t = line.trim_start();
    // Preserve leading whitespace.
    let indent_len = line.len() - t.len();
    let indent = &line[..indent_len];

    if let Some(rest) = t.strip_prefix("attribute ") {
        return format!("{indent}in {rest}");
    }
    if let Some(rest) = t.strip_prefix("varying ") {
        // Vertex: out, fragment: in.
        let kw = if is_frag { "in" } else { "out" };
        return format!("{indent}{kw} {rest}");
    }
    line.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_core_150() {
        let o = translate("#version 150 core\nin vec3 p;\nvoid main(){ gl_Position = vec4(p,1); }\n").unwrap();
        assert!(o.starts_with("#version 300 es\n"));
        assert!(o.find("precision highp float;").unwrap() < o.find("in vec3 p;").unwrap());
    }

    #[test]
    fn legacy_120_becomes_100() {
        let o = translate("#version 120\nvarying vec2 uv;\nvoid main(){}\n").unwrap();
        assert!(o.starts_with("#version 100\n"));
        assert!(o.contains("varying vec2 uv;"));
    }

    #[test]
    fn no_version_defaults_to_100() {
        assert!(translate("void main(){}\n").unwrap().starts_with("#version 100\n"));
    }

    #[test]
    fn es_source_untouched() {
        let s = "#version 300 es\nvoid main(){}\n";
        assert_eq!(translate(s).unwrap(), s);
    }

    #[test]
    fn drops_arb_extension_and_keeps_defines_first() {
        let o = translate(
            "#version 150\n#extension GL_ARB_foo : enable\n#define X 1\nvoid main(){}\n",
        )
        .unwrap();
        assert!(!o.contains("GL_ARB_foo"));
        assert!(o.find("#define X").unwrap() < o.find("precision").unwrap());
    }

    #[test]
    fn rejects_glsl_400() {
        assert!(translate("#version 400\nvoid main(){}\n").is_err());
    }

    #[test]
    fn texture2d_becomes_texture() {
        let o = translate(
            "#version 150\nout vec4 c;\nuniform sampler2D s;\nvoid main(){ c = texture2D(s, vec2(0)); }\n",
        )
        .unwrap();
        assert!(o.contains("texture(s,"));
        assert!(!o.contains("texture2D("));
    }

    #[test]
    fn frag_color_rewritten() {
        let o = translate(
            "#version 150\nvoid main(){ gl_FragColor = vec4(1.0); }\n",
        )
        .unwrap();
        assert!(o.contains("out vec4 rust_FragColor;"));
        assert!(o.contains("rust_FragColor = vec4(1.0);"));
        assert!(!o.contains("gl_FragColor"));
    }

    #[test]
    fn attribute_varying_to_in_out() {
        let vs = translate(
            "#version 150\nattribute vec3 pos;\nvarying vec2 uv;\nvoid main(){ gl_Position = vec4(pos,1); }\n",
        )
        .unwrap();
        assert!(vs.contains("in vec3 pos;"));
        assert!(vs.contains("out vec2 uv;"));

        let fs = translate(
            "#version 150\nvarying vec2 uv;\nvoid main(){ gl_FragColor = vec4(uv,0,1); }\n",
        )
        .unwrap();
        assert!(fs.contains("in vec2 uv;"));
        assert!(fs.contains("rust_FragColor"));
    }
}
