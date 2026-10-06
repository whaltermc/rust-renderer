//! Desktop-GLSL names and features that GLSL ES 3.00 spells differently or lacks.
//!
//! Everything here is a whole-identifier rewrite on a single source line, in the same lexical
//! style as the rest of the translator: no GLSL parser, formatting (and so driver line numbers)
//! preserved.

/// Replaces `old` with `new` wherever it appears as a complete identifier. Operates on
/// characters, so non-ASCII text in comments survives unchanged.
pub(crate) fn replace_ident(src: &str, old: &str, new: &str) -> String {
    if old.is_empty() || !src.contains(old) {
        return src.to_string();
    }
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(src.len() + 8);
    let mut rest = src;
    let mut prev: Option<char> = None;
    while let Some(pos) = rest.find(old) {
        let before = rest[..pos].chars().next_back().or(prev);
        let after = rest[pos + old.len()..].chars().next();
        let starts_ok = !before.is_some_and(is_ident);
        let ends_ok = !after.is_some_and(is_ident);
        out.push_str(&rest[..pos]);
        if starts_ok && ends_ok {
            out.push_str(new);
        } else {
            out.push_str(old);
        }
        prev = old.chars().next_back();
        rest = &rest[pos + old.len()..];
    }
    out.push_str(rest);
    out
}

/// Screen-space derivatives.
///
/// `dFdx`, `dFdy` and `fwidth` are core in ES 3.00 fragment shaders, but `GL_ARB_derivative_control`
/// (GLSL 4.50) adds `*Fine` / `*Coarse` variants that ES does not have at all. Packs that
/// were written against desktop GL use them for normal reconstruction, anti-aliased edges and
/// parallax. ES lets the implementation pick the precision of `dFdx`, which is what both
/// variants amount to when the hardware has no choice, so they map onto the plain forms.
pub(crate) fn rewrite_derivative_names(line: &str) -> String {
    if !line.contains("dFd") && !line.contains("fwidth") {
        return line.to_string();
    }
    let mut s = line.to_string();
    for (from, to) in [
        ("dFdxFine", "dFdx"),
        ("dFdyFine", "dFdy"),
        ("dFdxCoarse", "dFdx"),
        ("dFdyCoarse", "dFdy"),
        ("fwidthFine", "fwidth"),
        ("fwidthCoarse", "fwidth"),
        // OES_standard_derivatives never renamed the functions, but some packs wrap them.
        ("dFdxOES", "dFdx"),
        ("dFdyOES", "dFdy"),
        ("fwidthOES", "fwidth"),
    ] {
        s = replace_ident(&s, from, to);
    }
    s
}

/// Whether the source calls a screen-space derivative at all.
pub(crate) fn uses_derivatives(src: &str) -> bool {
    ["dFdx", "dFdy", "fwidth"].iter().any(|n| src.contains(n))
}

/// `GL_ARB_shader_texture_lod` / `GL_EXT_shader_texture_lod` spell the explicit-LOD and
/// gradient lookups with `ARB` / `EXT` suffixes. ES 3.00 has them as overloads of `textureLod`
/// and `textureGrad`.
pub(crate) fn rewrite_extension_texture_names(line: &str) -> String {
    if !line.contains("ARB(") && !line.contains("EXT(") && !line.contains("OES(") {
        return line.to_string();
    }
    let mut s = line.to_string();
    for (from, to) in [
        ("texture2DLodARB", "textureLod"),
        ("texture2DLodEXT", "textureLod"),
        ("texture2DProjLodARB", "textureProjLod"),
        ("texture2DProjLodEXT", "textureProjLod"),
        ("texture2DGradARB", "textureGrad"),
        ("texture2DGradEXT", "textureGrad"),
        ("texture2DProjGradARB", "textureProjGrad"),
        ("texture2DProjGradEXT", "textureProjGrad"),
        ("texture3DLodARB", "textureLod"),
        ("texture3DLodEXT", "textureLod"),
        ("texture3DGradARB", "textureGrad"),
        ("textureCubeLodARB", "textureLod"),
        ("textureCubeLodEXT", "textureLod"),
        ("textureCubeGradARB", "textureGrad"),
        ("textureCubeGradEXT", "textureGrad"),
        ("shadow2DLodARB", "textureLod"),
        ("shadow2DARB", "texture"),
        ("texture2DARB", "texture"),
    ] {
        s = replace_ident(&s, from, to);
    }
    s
}

/// Words reserved by GLSL ES 3.00 that desktop drivers (NVIDIA in particular) accept as
/// ordinary identifiers. Shader packs name variables `input`, `filter`, `half`, `common`...
/// and compile everywhere except on ES.
const ES_RESERVED: &[&str] = &[
    "input", "output", "filter", "half", "fixed", "long", "short", "unsigned", "common",
    "partition", "active", "external", "interface", "public", "static", "this", "template",
    "cast", "namespace", "using", "resource", "inline", "noinline", "goto", "class", "enum",
    "union", "typedef", "sizeof", "superp", "asm", "extern",
];

/// Prefix for renamed identifiers. Not `gl_` / `GL_` (reserved) and no double underscore.
const RENAME_PREFIX: &str = "rs_id_";

/// Renames [`ES_RESERVED`] words used as identifiers. Applied to every stage, so a renamed
/// varying still matches across vertex and fragment shaders.
///
/// Directives that carry no GLSL code (`#extension`, `#version`, `#pragma`, `#error`,
/// `#line`) are left alone; `#define` and `#if` bodies are rewritten because macros often
/// expand to the renamed names.
pub(crate) fn rename_reserved_identifiers(line: &str) -> String {
    let t = line.trim_start();
    if t.starts_with('#') {
        let d = t[1..].trim_start();
        let code_directive = d.starts_with("define")
            || d.starts_with("if")
            || d.starts_with("elif")
            || d.starts_with("undef");
        if !code_directive {
            return line.to_string();
        }
    }
    let mut s: Option<String> = None;
    for word in ES_RESERVED {
        let cur = s.as_deref().unwrap_or(line);
        if cur.contains(word) {
            let replaced = replace_ident(cur, word, &format!("{RENAME_PREFIX}{word}"));
            s = Some(replaced);
        }
    }
    s.unwrap_or_else(|| line.to_string())
}

/// `fma` arrived in GLSL 4.00 and ES 3.20. A macro keeps it type-generic.
pub(crate) fn needs_fma_macro(src: &str) -> bool {
    if !src.contains("fma(") {
        return false;
    }
    let user_defined = ["float", "vec2", "vec3", "vec4"]
        .iter()
        .any(|t| src.contains(&format!("{t} fma(")) || src.contains(&format!("{t} fma (")));
    !user_defined
}

pub(crate) const FMA_MACRO: &str = "#define fma(a, b, c) ((a) * (b) + (c))\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivative_control_variants_map_to_plain_derivatives() {
        assert_eq!(
            rewrite_derivative_names("vec3 n = cross(dFdxFine(p), dFdyCoarse(p));"),
            "vec3 n = cross(dFdx(p), dFdy(p));"
        );
        assert_eq!(rewrite_derivative_names("float w = fwidthFine(x);"), "float w = fwidth(x);");
        // Plain forms and similarly named user functions are untouched.
        assert_eq!(rewrite_derivative_names("a = dFdx(b);"), "a = dFdx(b);");
        assert_eq!(rewrite_derivative_names("a = myDFdxFine(b);"), "a = myDFdxFine(b);");
    }

    #[test]
    fn extension_texture_lookups_map_to_es_overloads() {
        assert_eq!(
            rewrite_extension_texture_names("c = texture2DGradARB(s, uv, dx, dy);"),
            "c = textureGrad(s, uv, dx, dy);"
        );
        assert_eq!(
            rewrite_extension_texture_names("c = texture2DLodEXT(s, uv, 0.0);"),
            "c = textureLod(s, uv, 0.0);"
        );
    }

    #[test]
    fn reserved_words_are_renamed_only_as_whole_identifiers() {
        assert_eq!(
            rename_reserved_identifiers("float filter = input * 2.0;"),
            "float rs_id_filter = rs_id_input * 2.0;"
        );
        assert_eq!(rename_reserved_identifiers("float filtered = 1.0;"), "float filtered = 1.0;");
        assert_eq!(rename_reserved_identifiers("#extension GL_x : enable"), "#extension GL_x : enable");
        assert_eq!(
            rename_reserved_identifiers("#define HALF half"),
            "#define HALF rs_id_half"
        );
        assert_eq!(rename_reserved_identifiers("// the input"), "// the rs_id_input");
    }

    #[test]
    fn replace_ident_preserves_non_ascii() {
        assert_eq!(replace_ident("é input é", "input", "x"), "é x é");
    }

    #[test]
    fn fma_macro_only_when_needed() {
        assert!(needs_fma_macro("float a = fma(x, y, z);"));
        assert!(!needs_fma_macro("float fma(float a, float b, float c) { return a*b+c; }\nfloat q = fma(1.,2.,3.);"));
        assert!(!needs_fma_macro("float a = 1.0;"));
    }
}
