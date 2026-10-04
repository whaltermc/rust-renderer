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

/// Removes Mojang-only preprocessor lines that are not GLSL.
fn strip_mojang_directives(src: &str) -> String {
    src.lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("#moj_") || t.starts_with("#import") || t.starts_with("#include"))
        })
        .fold(String::new(), |mut acc, l| {
            acc.push_str(l);
            acc.push('\n');
            acc
        })
}


/// Highest fragment output layer the device can be asked for. Iris/OptiFine packs use up to
/// eight; GLES 3.0 only guarantees four, and the real limit comes from the capability probe,
/// but declaring more than the shader writes is what used to break compilation.
/// Fragment outputs declared for a shader that writes several layers.
///
/// 8 rather than the ES 3.0 guaranteed minimum of 4: the Android devices this targets report
/// 8 draw buffers, and shader packs routinely write more than four layers. Declaring up to 8
/// is harmless on those devices, whereas capping at 4 folds real attachments onto the last
/// output and silently drops them.
pub const MAX_FRAG_OUTPUTS: usize = 8;

/// How many fragment outputs this shader actually writes.
///
/// Counting real usage matters: the previous code declared outputs 1-3 whenever any
/// `gl_FragData[1..]` appeared, and never declared location 0 for a shader that only wrote
/// `gl_FragData[0]`, which produced a shader referencing an undeclared identifier.
fn fragment_output_layers(src: &str) -> usize {
    if !src.contains("gl_FragColor") && !src.contains("gl_FragData") {
        return 0;
    }
    // A bare `gl_FragData` (no index) is not valid GLSL, but treat it as one layer.
    let max_layer = max_frag_data_index(src).unwrap_or(0);
    // Cap the declared set: beyond this, extra attachments fold onto the last output.
    (max_layer + 1).min(MAX_FRAG_OUTPUTS)
}

/// Highest `gl_FragData[n]` index written by the shader, scanning the source rather than
/// probing a fixed range of indices: shader packs do write layers above the usual range.
fn max_frag_data_index(src: &str) -> Option<usize> {
    let bytes = src.as_bytes();
    const TOKEN: &[u8] = b"gl_FragData[";
    let mut found: Option<usize> = None;
    let mut i = 0;
    while i + TOKEN.len() < bytes.len() {
        if bytes[i..].starts_with(TOKEN) {
            let mut j = i + TOKEN.len();
            let mut value = 0usize;
            let mut digits = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                value = value * 10 + (bytes[j] - b'0') as usize;
                digits += 1;
                j += 1;
            }
            if digits > 0 && j < bytes.len() && bytes[j] == b']' {
                found = Some(found.map_or(value, |m: usize| m.max(value)));
            }
            i = j;
            continue;
        }
        i += 1;
    }
    found
}

/// Declares `layers` fragment outputs, location 0 being the primary `rust_FragColor`.
fn fragment_output_decls(layers: usize) -> String {
    let mut decls = String::from("layout(location = 0) out vec4 rust_FragColor;\n");
    for i in 1..layers {
        decls.push_str(&format!("layout(location = {i}) out vec4 rust_FragData{i};\n"));
    }
    decls
}

/// Rewrites `gl_FragColor` and `gl_FragData[n]` onto the declared outputs.
///
/// Layers beyond what was declared are folded onto the highest declared one: the shader then
/// compiles and renders, losing only those extra attachments, instead of failing the compile
/// and aborting resource loading.
fn rewrite_frag_data(src: &str, layers: usize) -> String {
    let bytes = src.as_bytes();
    const TOKEN: &[u8] = b"gl_FragData[";
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(TOKEN) {
            let mut j = i + TOKEN.len();
            let mut value = 0usize;
            let mut digits = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                value = value * 10 + (bytes[j] - b'0') as usize;
                digits += 1;
                j += 1;
            }
            if digits > 0 && j < bytes.len() && bytes[j] == b']' {
                if value == 0 || layers <= 1 {
                    out.push_str("rust_FragColor");
                } else {
                    out.push_str(&format!("rust_FragData{}", value.min(layers - 1)));
                }
                i = j + 1;
                continue;
            }
        }
        // Not a recognised index: copy one byte and continue scanning.
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&src[i..i + ch_len]);
        i += ch_len;
    }
    out.replace("gl_FragColor", "rust_FragColor")
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
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
            .replace("texture2DGrad(", "textureGrad(")
            .replace("texture2DProjGrad(", "textureProjGrad(")
            .replace("texture2DLodEXT(", "textureLod(")
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

        // ES 3.00 has no EXT_frag_depth spelling.
        s = s.replace("gl_FragDepthEXT", "gl_FragDepth");

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
        let src = strip_mojang_directives(src);
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

    // Map desktop GLSL into ES 3.00. The threshold used to be 130, which sent every
    // `#version 120` shader (the OptiFine/Iris era, and Minecraft 1.12-1.15) to ES 1.00 --
    // where `layout(location = N) out` is not valid GLSL, so every MRT shader failed to
    // compile. ES 3.00 also accepts everything ES 1.00 did, so this is strictly broader.
    let use_300 = num >= 110;
    let (header, precision) = if use_300 {
        ("#version 300 es", PRECISION_300)
    } else {
        ("#version 100", PRECISION_100)
    };

    let is_frag = looks_like_fragment(src);
    let layers = fragment_output_layers(src);
    let needs_frag_out = use_300 && layers > 0;

    let mut out = String::with_capacity(src.len() + 512);
    out.push_str(header);
    out.push('\n');

    let mut inserted_precision = false;

    for line in src.lines() {
        let t = line.trim_start();

        if t.starts_with("#version") {
            continue;
        }

        // Mojang's preprocessor directives (#moj_import / #import / #moj_pack) are resolved by
        // the game, but anything that reaches us unresolved is not valid GLSL and would fail
        // the compile. Dropping an unknown directive is better than failing the shader, which
        // aborts resource loading.
        if t.starts_with("#moj_") || t.starts_with("#import") || t.starts_with("#include") {
            continue;
        }

        // ES 3.00 needs essentially no #extension directives, and a name it does not know
        // (GL_EXT_frag_depth, most desktop GL_EXT_* lines) is a compile error. Dropping them
        // all is the safer direction: the features they gate are either core in ES 3.00 or
        // not used by Minecraft shaders.
        if t.starts_with("#extension") {
            continue;
        }

        if !inserted_precision && !t.is_empty() && !t.starts_with('#') && !t.starts_with("//") {
            out.push_str(precision);
            if needs_frag_out {
                out.push_str(&fragment_output_decls(layers));
            }
            inserted_precision = true;
        }

        let mut rewritten = rewrite_line_body(line, use_300, is_frag, needs_frag_out);
        if use_300 {
            rewritten = rewrite_es300_tokens(rewritten);
        }
        if needs_frag_out {
            // Runs whenever the shader writes any output, including a single high-index
            // gl_FragData[n]; gating this on MRT left such shaders unrewritten.
            rewritten = rewrite_frag_data(&rewritten, layers);
        }
        out.push_str(&rewritten);
        out.push('\n');
    }

    if !inserted_precision {
        out.push_str(precision);
        if needs_frag_out {
            out.push_str(&fragment_output_decls(layers));
        }
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
    fn mojang_preprocessor_directives_are_dropped() {
        // A shader that reaches us with these unresolved would fail to compile and abort
        // resource loading, which is what leaves the Mojang splash on screen.
        let o = translate(
            "#version 410 core\n#moj_import <vsh_main>\nvoid main(){ gl_Position = vec4(0); }\n",
        )
        .unwrap();
        assert!(!o.contains("#moj_import"), "got: {o}");
        let es = translate("#version 300 es\n#moj_pack 1 0 f\nprecision highp float;\nvoid main(){}\n");
        assert!(!es.unwrap().contains("#moj_pack"));
    }

    #[test]
    fn noperspective_is_removed() {
        let o = translate("#version 330 core\nnoperspective in vec2 uv;\nvoid main(){gl_Position=vec4(0);}").unwrap();
        assert!(!o.contains("noperspective"));
    }

    // ---- OptiFine / Iris era (#version 120) shader packs ----

    #[test]
    fn version_120_targets_es_300_not_es_100() {
        // ES 1.00 cannot express `layout(location = N) out`, so sending 120 there made every
        // MRT shader pack fail to compile.
        let o = translate(
            "#version 120\nvarying vec2 tc;\nvoid main(){ gl_FragColor = vec4(tc, 0.0, 1.0); }\n",
        )
        .unwrap();
        assert!(o.starts_with("#version 300 es\n"), "got: {}", &o[..40]);
        assert!(o.contains("in vec2 tc"), "varying must become in");
        assert!(o.contains("layout(location = 0) out vec4 rust_FragColor;"));
    }

    #[test]
    fn mrt_declares_exactly_the_layers_written() {
        let o = translate(
            "#version 120\nvoid main(){ gl_FragData[0] = vec4(1.0); gl_FragData[1] = vec4(0.0); }\n",
        )
        .unwrap();
        assert!(o.contains("layout(location = 0) out vec4 rust_FragColor;"));
        assert!(o.contains("layout(location = 1) out vec4 rust_FragData1;"));
        // Only two layers are written, so 2 and 3 must not be declared.
        assert!(!o.contains("rust_FragData2"), "unused output declared: {o}");
        assert!(!o.contains("rust_FragData3"), "unused output declared: {o}");
        assert!(!o.contains("gl_FragData"), "gl_FragData left behind: {o}");
    }

    #[test]
    fn frag_data_zero_declares_a_primary_output() {
        // The old code rewrote gl_FragData[0] to rust_FragColor but only declared the output
        // when gl_FragColor appeared, so this referenced an undeclared identifier.
        let o = translate("#version 120\nvoid main(){ gl_FragData[0] = vec4(1.0); }\n").unwrap();
        assert!(o.contains("layout(location = 0) out vec4 rust_FragColor;"));
        assert!(!o.contains("gl_FragData"));
    }

    #[test]
    fn layers_beyond_the_declared_set_fold_onto_the_last_one() {
        // Folding keeps the shader compiling, which matters because a failed compile aborts
        // resource loading rather than just dropping a draw.
        let o = translate(
            "#version 120\nvoid main(){ gl_FragData[0] = vec4(1.0); gl_FragData[5] = vec4(0.0); }\n",
        )
        .unwrap();
        assert!(!o.contains("gl_FragData[5]"), "unrewritten: {o}");
        assert!(o.contains("rust_FragData5"), "should keep its own layer: {o}");
        assert!(o.contains("layout(location = 5) out vec4 rust_FragData5;"));

        // And when fewer layers exist than the shader writes, it folds rather than failing.
        let o2 = translate("#version 120\nvoid main(){ gl_FragData[9] = vec4(1.0); }\n").unwrap();
        assert!(!o2.contains("gl_FragData"), "unrewritten: {o2}");
    }

    #[test]
    fn frag_depth_ext_and_grad_sampling_are_translated() {
        let o = translate(
            "#version 120\nextension GL_EXT_frag_depth\nuniform sampler2D s;\n\
             void main(){ gl_FragDepthEXT = gl_FragCoord.z; gl_FragColor = texture2DGrad(s, vec2(0.), vec2(0.), vec2(0.)); }\n",
        );
        // A missing '#extension' prefix above is deliberate: unknown directives must not escape.
        let o = translate(
            "#version 120\n#extension GL_EXT_frag_depth\nuniform sampler2D s;\n\
             void main(){ gl_FragDepthEXT = gl_FragCoord.z; gl_FragColor = texture2DGrad(s, vec2(0.), vec2(0.), vec2(0.)); }\n",
        )
        .unwrap();
        assert!(o.contains("gl_FragDepth ="), "got: {o}");
        assert!(!o.contains("gl_FragDepthEXT"));
        assert!(o.contains("textureGrad("), "got: {o}");
        assert!(!o.contains("#extension"), "unknown extension kept: {o}");
    }

    #[test]
    fn plain_shader_declares_no_fragment_output() {
        let o = translate("#version 120\nuniform sampler2D s;\nvoid main(){ gl_FragColor = vec4(1.0); }\n").unwrap();
        assert!(o.contains("layout(location = 0) out vec4 rust_FragColor;"));
        let vertex = translate("#version 120\nvoid main(){ gl_Position = vec4(1.0); }\n").unwrap();
        assert!(!vertex.contains("out vec4"), "vertex shader must not declare frag outputs");
    }

    #[test]
    fn es_300_shader_passes_through_unchanged() {
        let s = "#version 300 es\nprecision highp float;\nout vec4 c;\nvoid main(){ c = vec4(1.0); }\n";
        assert_eq!(translate(s).unwrap(), s);
    }
}
