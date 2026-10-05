//! Desktop GLSL → GLSL ES translation for Minecraft / LWJGL shaders.
//!
//! Goal: never hard-fail on common vanilla/mod shaders. Prefer best-effort rewrite so
//! the driver reports a real compile error instead of an empty shader (which freezes
//! loading screens). Geometry/tessellation/compute still rejected.

use std::borrow::Cow;
use std::collections::HashSet;

mod numeric;

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

fn strip_float_suffixes(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        let starts_number = bytes[i].is_ascii_digit()
            || (bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit));
        let boundary = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        if !starts_number || !boundary {
            let length = utf8_len(bytes[i]);
            out.push_str(&src[i..i + length]);
            i += length;
            continue;
        }

        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
        }
        if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
            let exponent = i;
            i += 1;
            if i < bytes.len() && matches!(bytes[i], b'+' | b'-') { i += 1; }
            let digits = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
            if digits == i { i = exponent; }
        }
        out.push_str(&src[start..i]);
        if i < bytes.len() && matches!(bytes[i], b'f' | b'F') {
            i += 1;
        }
    }
    out
}

fn replace_glsl_call(src: &str, call: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(index) = rest.find(call) {
        out.push_str(&rest[..index]);
        let is_identifier_byte = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
        let starts_identifier = index > 0 && is_identifier_byte(rest.as_bytes()[index - 1]);
        if starts_identifier {
            out.push_str(call);
        } else {
            out.push_str(replacement);
        }
        rest = &rest[index + call.len()..];
    }
    out.push_str(rest);
    out
}

fn declares_ftransform(src: &str) -> bool {
    src.lines().any(|line| {
        let Some(index) = line.find("ftransform()") else {
            return false;
        };
        let preceded_by_identifier = index > 0
            && (line.as_bytes()[index - 1].is_ascii_alphanumeric() || line.as_bytes()[index - 1] == b'_');
        !preceded_by_identifier && line[index + "ftransform()".len()..].trim_start().starts_with('{')
    })
}


/// Rewrites GLSL 3.30 desktop qualifiers/features that do not exist in GLSL ES 3.00.
/// This deliberately stays lexical rather than using a full GLSL parser: Minecraft's
/// generated shaders are regular and preserving formatting makes driver logs useful.
fn rewrite_es300_tokens(mut s: String) -> String {
    s = strip_float_suffixes(&s);
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

    // Mojang's shared lightmap helper divides an ivec2 by a float, and shader packs compare
    // floats against integer literals. Desktop GLSL converts implicitly; GLSL ES has no such
    // conversion, so these are fixed structurally by `numeric::widen_int_literals` earlier in
    // the pipeline rather than by naming individual expressions here.
    s = s.replace("? 0 : skip", "? 0.0 : skip");
    s = rewrite_texture_lod_integer_levels(&s);

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

/// Rewrites `#error "message"` so the message lexes as preprocessing tokens.
///
/// GLSL has no string-literal syntax, so the quotes are an illegal character. Sodium's chunk
/// shader guards its vertex-compression path with `#error "Vertex compression must be
/// enabled"` inside the `#else` of the `#ifdef` that is actually taken, and Mali's compiler
/// lexes the whole file before discarding skipped branches: it reports
/// `Unknown character '"'`, the shader fails, and Sodium aborts the frame. Dropping the
/// quotes keeps the directive and its message intact for a compiler that does evaluate it.
fn sanitize_error_directive(line: &str) -> Cow<'_, str> {
    let trimmed = line.trim_start();
    // `#errorXYZ` is a different token, not this directive.
    let Some(message) = trimmed.strip_prefix("#error") else {
        return Cow::Borrowed(line);
    };
    if !message.is_empty() && !message.starts_with([' ', '\t']) {
        return Cow::Borrowed(line);
    }
    let indent = &line[..line.len() - trimmed.len()];
    let sanitized: String = message
        .chars()
        // Everything outside identifiers and whitespace is not a preprocessing token: `"`,
        // `'` and `\` in particular. Replacing rather than deleting keeps words separated.
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { ' ' })
        .collect();
    let sanitized = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    if sanitized.is_empty() {
        return Cow::Owned(format!("{indent}#error"));
    }
    Cow::Owned(format!("{indent}#error {sanitized}"))
}

/// [`sanitize_error_directive`] over a whole source.
fn sanitize_error_directives(src: &str) -> String {
    src.lines()
        .map(sanitize_error_directive)
        .fold(String::new(), |mut acc, line| {
            acc.push_str(&line);
            acc.push('\n');
            acc
        })
}

fn contains_identifier(src: &str, identifier: &str) -> bool {
    src.match_indices(identifier).any(|(start, token)| {
        let end = start + token.len();
        let is_identifier_byte = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
        let before_is_boundary = start == 0 || !is_identifier_byte(src.as_bytes()[start - 1]);
        let after_is_boundary = end == src.len() || !is_identifier_byte(src.as_bytes()[end]);
        before_is_boundary && after_is_boundary
    })
}

fn strip_unused_iris_fog_initializer(src: &str) -> String {
    const DECLARATION: &str = "iris_FogParameters iris_Fog =";
    if !src.lines().any(|line| line.trim_start().starts_with(DECLARATION)) {
        return src.to_string();
    }

    let without_initializer = src
        .lines()
        .filter(|line| !line.trim_start().starts_with(DECLARATION))
        .fold(String::new(), |mut out, line| {
            out.push_str(line);
            out.push('\n');
            out
        });
    if contains_identifier(&without_initializer, "iris_Fog") {
        src.to_string()
    } else {
        without_initializer
    }
}

fn has_later_assignment(lines: &[&str], declaration_line: usize, name: &str) -> bool {
    for (line_number, line) in lines.iter().enumerate() {
        if line_number == declaration_line || line.trim_start().starts_with("//") {
            continue;
        }
        let bytes = line.as_bytes();
        let mut start = 0;
        while let Some(relative) = line[start..].find(name) {
            let index = start + relative;
            let end = index + name.len();
            let is_identifier_byte = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
            let whole_identifier = (index == 0 || !is_identifier_byte(bytes[index - 1]))
                && (end == bytes.len() || !is_identifier_byte(bytes[end]));
            if whole_identifier {
                let mut operator = end;
                while operator < bytes.len() && bytes[operator].is_ascii_whitespace() {
                    operator += 1;
                }
                if operator < bytes.len()
                    && ((bytes[operator] == b'=' && bytes.get(operator + 1) != Some(&b'='))
                        || matches!(bytes[operator], b'+' | b'-' | b'*' | b'/')
                            && bytes.get(operator + 1) == Some(&b'='))
                {
                    return true;
                }
            }
            start = end;
        }
    }
    false
}


fn collect_function_param_names(src: &str) -> HashSet<String> {
    const VALUE_TYPES: &[&str] = &[
        "float", "double", "int", "uint", "bool", "void", "vec2", "vec3", "vec4", "ivec2",
        "ivec3", "ivec4", "uvec2", "uvec3", "uvec4", "mat2", "mat3", "mat4",
    ];
    let mut params = HashSet::new();
    let mut in_param_list = false;

    for line in src.lines() {
        let trimmed = line.trim_start();
        if !in_param_list {
            for ty in VALUE_TYPES {
                if let Some(rest) = trimmed.strip_prefix(ty) {
                    let rest = rest.trim_start();
                    if let Some((name_part, after_paren)) = rest.split_once('(') {
                        let name = name_part.trim();
                        if name.bytes().all(|b| {
                            b == b'_' || b.is_ascii_alphabetic() || (b.is_ascii_digit() && b != b'0')
                        }) {
                            in_param_list = true;
                            for p in after_paren.split(',') {
                                let raw = p;
                                let p = raw.trim_end_matches(|c: char| c == ')' || c == '{' || c == ';' || c.is_whitespace()).trim();
                                if let Some(pname) = p.split_whitespace().last() {
                                    let pname = pname.to_string();
                                    if !pname.is_empty() {
                                        params.insert(pname);
                                    }
                                }
                                if raw.contains(')') {
                                    in_param_list = false;
                                    break;
                                }
                            }
                        }
                    }
                    break;
                }
            }
        } else if !trimmed.starts_with("//") {
            for p in line.split(',') {
                let raw = p;
                let p = raw.trim_end_matches(|c: char| c == ')' || c == '{' || c == ';' || c.is_whitespace()).trim();
                if let Some(pname) = p.split_whitespace().last() {
                    let pname = pname.to_string();
                    if !pname.is_empty() {
                        params.insert(pname);
                    }
                }
                if raw.contains(')') {
                    in_param_list = false;
                    break;
                }
            }
        }
    }

    params
}

fn collect_potential_macro_names(src: &str) -> HashSet<String> {
    const VALUE_TYPES: &[&str] = &[
        "float", "double", "int", "uint", "bool", "vec2", "vec3", "vec4", "ivec2",
        "ivec3", "ivec4", "uvec2", "uvec3", "uvec4", "mat2", "mat3", "mat4",
    ];
    let mut names = HashSet::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut brace_depth = 0isize;
    for line in lines.iter() {
        let trimmed = line.trim_start();
        if brace_depth == 0
            && !trimmed.starts_with("const ")
            && !trimmed.starts_with("//")
            && !trimmed.starts_with('#')
        {
            if let Some((left, _)) = trimmed.split_once('=') {
                let mut declaration = left.split_whitespace();
                let mut ty_opt = None;
                for tok in declaration.by_ref() {
                    if VALUE_TYPES.contains(&tok) {
                        ty_opt = Some(tok);
                        break;
                    }
                }
                let ty = match ty_opt { Some(t) => t, None => continue };
                let name = match declaration.next() { Some(n) => n, None => continue };
                if declaration.next().is_some()
                    || !name.bytes().enumerate().all(|(index, byte)| {
                        byte == b'_'
                            || byte.is_ascii_alphanumeric()
                            || index > 0 && byte.is_ascii_digit()
                    })
                {
                    continue;
                }
                names.insert(name.to_string());
            }
        }
        if !trimmed.starts_with("//") {
            brace_depth += line.chars().filter(|&ch| ch == '{').count() as isize;
            brace_depth -= line.chars().filter(|&ch| ch == '}').count() as isize;
        }
    }
    names
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn replace_whole_word_after_comment(code: &str, old: &str, new: &str) -> String {
    let mut result = String::with_capacity(code.len());
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(old.as_bytes()) {
            let start = i;
            let end = i + old.len();
            let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
            let after_ok = end == bytes.len() || !is_ident_byte(bytes[end]);
            if before_ok && after_ok {
                result.push_str(new);
                i = end;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn rename_shadowed_params(src: &str) -> String {
    let param_names = collect_function_param_names(src);
    let potential_macro_names = collect_potential_macro_names(src);
    let collisions: HashSet<_> = param_names.intersection(&potential_macro_names).cloned().collect();
    if collisions.is_empty() {
        return src.to_string();
    }

    const VALUE_TYPES: &[&str] = &[
        "float", "double", "int", "uint", "bool", "void", "vec2", "vec3", "vec4", "ivec2",
        "ivec3", "ivec4", "uvec2", "uvec3", "uvec4", "mat2", "mat3", "mat4",
    ];
    let mut out = String::with_capacity(src.len());
    let mut in_param_list = false;
    let mut in_body = false;
    let mut brace_depth = 0isize;
    let mut current_renames: Vec<(String, String)> = Vec::new();

    for line in src.lines() {
        let trimmed = line.trim_start();
        let mut renamed_line = line.to_string();

        if !in_param_list && !in_body {
            for ty in VALUE_TYPES {
                if let Some(rest) = trimmed.strip_prefix(ty) {
                    let rest = rest.trim_start();
                    if let Some((name_part, after_paren)) = rest.split_once('(') {
                        let func_name = name_part.trim();
                        if func_name.bytes().all(|b| {
                            b == b'_' || b.is_ascii_alphabetic() || (b.is_ascii_digit() && b != b'0')
                        }) {
                            in_param_list = true;
                            current_renames.clear();

                            for p in after_paren.split(',') {
                                let raw = p;
                                let p = raw.trim_end_matches(|c: char| c == ')' || c == '{' || c == ';' || c.is_whitespace()).trim();
                                if let Some(pname) = p.split_whitespace().last() {
                                    let pname = pname.to_string();
                                    if !pname.is_empty() && collisions.contains(&pname) {
                                        let new_name = format!("{pname}_");
                                        current_renames.push((pname.clone(), new_name.clone()));
                                        renamed_line = replace_whole_word_after_comment(&renamed_line, &pname, &new_name);
                                    }
                                }
                                if raw.contains(')') {
                                    in_param_list = false;
                                    break;
                                }
                            }
                            break;
                        }
                    }
                    break;
                }
            }
        } else if in_param_list {
            for p in line.split(',') {
                let raw = p;
                let p = raw.trim_end_matches(|c: char| c == ')' || c == '{' || c == ';' || c.is_whitespace()).trim();
                if let Some(pname) = p.split_whitespace().last() {
                    let pname = pname.to_string();
                    if !pname.is_empty() && collisions.contains(&pname) {
                        let new_name = format!("{pname}_");
                        current_renames.push((pname.clone(), new_name.clone()));
                        renamed_line = replace_whole_word_after_comment(&renamed_line, &pname, &new_name);
                    }
                }
                if raw.contains(')') {
                    in_param_list = false;
                    break;
                }
            }
        } else if in_body {
            for (old, new) in &current_renames {
                renamed_line = replace_whole_word_after_comment(&renamed_line, old, new);
            }
        }

        let open = line.chars().filter(|&ch| ch == '{').count();
        let close = line.chars().filter(|&ch| ch == '}').count();
        brace_depth += open as isize;
        brace_depth -= close as isize;

        if !in_param_list && !in_body && open > 0 && brace_depth > 0 {
            in_body = true;
        }

        if in_body && brace_depth <= 0 {
            in_body = false;
            current_renames.clear();
        }

        out.push_str(&renamed_line);
        out.push('\n');
    }

    out
}

fn macroize_nonconstant_globals(src: &str) -> String {
    let renamed_source = rename_shadowed_params(src);
    macroize_nonconstant_globals_with_params(&renamed_source, &collect_function_param_names(&renamed_source))
}

fn macroize_nonconstant_globals_with_params(
    src: &str,
    param_names: &HashSet<String>,
) -> String {
    const VALUE_TYPES: &[&str] = &[
        "float", "double", "int", "uint", "bool", "vec2", "vec3", "vec4", "ivec2",
        "ivec3", "ivec4", "uvec2", "uvec3", "uvec4", "mat2", "mat3", "mat4",
    ];
        let lines: Vec<&str> = src.lines().collect();
        let mut out = String::with_capacity(src.len());
        let mut brace_depth = 0isize;
        for (line_number, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            let replacement = if brace_depth == 0
                && !trimmed.starts_with("const ")
                && !trimmed.starts_with("//")
                && !trimmed.starts_with('#')
            {
                trimmed.split_once('=').and_then(|(left, right)| {
                    let mut declaration = left.split_whitespace();
                    let mut ty_opt = None;
                    for tok in declaration.by_ref() {
                        if VALUE_TYPES.contains(&tok) {
                            ty_opt = Some(tok);
                            break;
                        }
                    }
                    let ty = ty_opt?;
                    let name = declaration.next()?;
                    if declaration.next().is_some()
                        || !name.bytes().enumerate().all(|(index, byte)| {
                            byte == b'_'
                                || byte.is_ascii_alphabetic()
                                || index > 0 && byte.is_ascii_digit()
                        })
                    {
                        return None;
                    }
                    let expression = right.trim().strip_suffix(';')?.trim();
                    if expression.is_empty() || has_later_assignment(&lines, line_number, name) {
                        return None;
                    }
                    Some(format!("{indent}#define {name} ({expression})"))
                })
            } else {
                None
            };
            out.push_str(replacement.as_deref().unwrap_or(line));
            out.push('\n');
            if !trimmed.starts_with("//") {
                brace_depth += line.chars().filter(|&ch| ch == '{').count() as isize;
                brace_depth -= line.chars().filter(|&ch| ch == '}').count() as isize;
            }
        }
        out
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

fn rewrite_texture_lod_integer_levels(src: &str) -> String {
    const CALL: &str = "textureLod(";
    let mut out = String::with_capacity(src.len());
    let mut copied_until = 0;
    let mut search_from = 0;
    while let Some(relative) = src[search_from..].find(CALL) {
        let open = search_from + relative + CALL.len() - 1;
        let mut depth = 1usize;
        let mut commas = Vec::with_capacity(2);
        let mut close = None;
        for (offset, byte) in src.as_bytes()[open + 1..].iter().enumerate() {
            let index = open + 1 + offset;
            match byte {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(index);
                        break;
                    }
                }
                b',' if depth == 1 => commas.push(index),
                _ => {}
            }
        }
        let Some(close) = close else { break };
        if commas.len() == 2 {
            let level_start = commas[1] + 1;
            let raw_level = &src[level_start..close];
            let level = raw_level.trim();
            let digits = level.strip_prefix('-').or_else(|| level.strip_prefix('+')).unwrap_or(level);
            if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) {
                out.push_str(&src[copied_until..level_start]);
                let leading = raw_level.len() - raw_level.trim_start().len();
                let trailing = raw_level.trim_end().len();
                out.push_str(&raw_level[..leading]);
                out.push_str("float(");
                out.push_str(level);
                out.push(')');
                out.push_str(&raw_level[trailing..]);
                copied_until = close;
            }
        }
        search_from = close + 1;
    }
    out.push_str(&src[copied_until..]);
    out
}

fn rewrite_line_body(
    line: &str,
    use_300: bool,
    is_frag: bool,
    needs_frag_out: bool,
    rewrite_ftransform: bool,
) -> String {
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
        if rewrite_ftransform {
            s = replace_glsl_call(&s, "ftransform()", "(gl_ModelViewProjectionMatrix * gl_Vertex)");
        }
        s = s
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
        let src = sanitize_error_directives(&src);
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

    let fog_sanitized_source = strip_unused_iris_fog_initializer(src);
    let suffix_sanitized_source = strip_float_suffixes(&fog_sanitized_source);
    // Before macroize_nonconstant_globals, which folds `float f = ...;` globals into #defines and
    // so hides the declarations this pass types against. ES 1.00 has no implicit conversion at
    // all and ES 3.00 kept only a few, so both targets need the literals widened.
    let widened_source = numeric::widen_int_literals(&suffix_sanitized_source);
    // After the widening, so a `mod` that started with one float argument has already become a
    // float call and is left for ES's own overload; what is left taking integers is retargeted
    // to the helper, which is injected only when something actually needed it.
    let (mod_rewritten_source, needs_int_mod) = numeric::rewrite_int_mod(&widened_source);
    let sanitized_source = macroize_nonconstant_globals(&mod_rewritten_source);
    let src = sanitized_source.as_str();
    let is_frag = looks_like_fragment(src);
    let layers = fragment_output_layers(src);
    let needs_frag_out = use_300 && layers > 0;
    let rewrite_ftransform = !declares_ftransform(src);

    let mut out = String::with_capacity(src.len() + 512);
    out.push_str(header);
    out.push('\n');

    let mut inserted_precision = false;

    for line in src.lines() {
        let line = sanitize_error_directive(line);
        let line = line.as_ref();
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
            if needs_int_mod {
                out.push_str(numeric::int_mod_helper());
                out.push('\n');
            }
            if needs_frag_out {
                out.push_str(&fragment_output_decls(layers));
            }
            inserted_precision = true;
        }

        let mut rewritten = rewrite_line_body(line, use_300, is_frag, needs_frag_out, rewrite_ftransform);
        if use_300 {
            rewritten = rewrite_es300_tokens(rewritten);
        }
        if needs_frag_out {
            // Runs whenever the shader writes any output, including a single high-index
            // gl_FragData[n]; gating this on MRT left such shaders unrewritten.
            rewritten = rewrite_frag_data(&rewritten, layers);
        }
        out.push_str(rewritten.trim_end_matches('\n'));
        // The per-line rewriters rebuild their input a line at a time and so return a trailing
        // newline of their own; without trimming it here every source line would be followed by
        // a blank one, which both bloats the shader and shifts every line number a driver
        // reports away from the source it came from.
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

    /// The exact line from Sodium's chunk vertex shader. GLSL has no string-literal syntax, so
    /// the quoted message is an illegal character; Mali lexes the whole file before discarding
    /// the `#else` branch this sits in, rejected it with `Unknown character '"'`, and the game
    /// crashed on the resulting failed compile.
    #[test]
    fn quoted_error_message_is_unquoted() {
        let sodium = "#version 300 es\n#define USE_VERTEX_COMPRESSION\nprecision highp float;\n\
            #ifdef USE_VERTEX_COMPRESSION\nvoid main(){ gl_Position = vec4(0); }\n\
            #else\n#error \"Vertex compression must be enabled\"\n#endif\n";
        let o = translate(sodium).unwrap();
        assert!(!o.contains('"'), "quoted #error message survived: {o}");
        assert!(o.contains("#error Vertex compression must be enabled"), "{o}");
        // The directive still fires where it is reached, so a genuinely unsupported
        // configuration is not silently compiled.
        assert!(o.contains("#ifdef USE_VERTEX_COMPRESSION"));
        assert!(o.contains("#else"));
        assert!(o.contains("#endif"));
    }

    #[test]
    fn error_message_without_quotes_is_untouched() {
        let s = "#version 300 es\n#error needs GLSL 420\nvoid main(){}\n";
        assert!(translate(s).unwrap().contains("#error needs GLSL 420"));
    }

    /// `#errorXYZ` is a different token and must not be rewritten into a directive.
    #[test]
    fn directive_prefix_that_is_not_error_is_untouched() {
        let s = "#version 300 es\n#errorish foo \"bar\"\nvoid main(){}\n";
        assert!(translate(s).unwrap().contains("#errorish foo \"bar\""));
    }

    /// Desktop shaders hit the same illegal character, so the rewrite is not ES-only.
    #[test]
    fn desktop_shader_error_message_is_unquoted() {
        let o = translate(
            "#version 150 core\n#error \"GL_ARB_gpu_shader5 is required\"\n\
             void main(){ gl_Position = vec4(0); }\n",
        )
        .unwrap();
        assert!(!o.contains('"'), "quoted #error message survived: {o}");
        assert!(o.contains("#error GL_ARB_gpu_shader5 is required"), "{o}");
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

    #[test]
    fn minecraft_lightmap_integer_uvs_convert_before_float_division() {
        let o = translate(
            "#version 150\nin ivec2 UV2;\nout vec2 texCoord2;\nin vec2 texCoord;\nvoid main() { texCoord2 = UV2; float level = floor(texCoord.x * 16) / 15; }\nvec2 lightmap(ivec2 uv) { return uv / 256.0; }\n",
        )
        .unwrap();
        // The int-to-float conversions are now made structurally rather than by naming these
        // three expressions. An integer vector has to convert with `vecN(...)`: `float(uv)` on
        // an ivec2 is a conversion of the first component, not componentwise, and glslang
        // rejects it here.
        assert!(o.contains("vec2(uv) / 256.0"), "{o}");
        assert!(o.contains("texCoord2 = vec2(UV2)"), "{o}");
        assert!(o.contains("floor(texCoord.x * 16.0) / 15.0"), "{o}");
    }

    #[test]
    fn unused_iris_fog_global_initializer_is_removed() {
        let source = "#version 150\nstruct iris_FogParameters { float density; };\nuniform float iris_FogDensity;\niris_FogParameters iris_Fog = iris_FogParameters(iris_FogDensity);\nvoid main() {}\n";
        let translated = translate(source).unwrap();
        assert!(!translated.contains("iris_FogParameters iris_Fog ="));

        let referenced = source.replace("void main() {}", "void main() { float d = iris_Fog.density; }");
        assert!(translate(&referenced).unwrap().contains("iris_FogParameters iris_Fog ="));
    }

    #[test]
    fn desktop_float_suffixes_are_removed_without_changing_identifiers() {
        let stripped = strip_float_suffixes("float f = 0.25f + 1e-5F; float index4f = 1.0f;");
        assert!(stripped.contains("0.25 + 1e-5"));
        assert!(stripped.contains("index4f = 1.0"));
        let o = translate("#version 150\nfloat f = 0.25f + 1e-5F;\nfloat index4f = 1.0f;\n").unwrap();
        assert!(o.contains("0.25 + 1e-5"));
        assert!(!o.contains("1.0f"));
    }

    #[test]
    fn ftransform_rewrite_does_not_corrupt_iris_helper_names() {
        let o = translate(
            "#version 150\nvec4 iris_ftransform() { return vec4(0.0); }\nvoid main() { gl_Position = ftransform(); }\n",
        )
        .unwrap();
        assert!(o.contains("vec4 iris_ftransform()"));
        assert!(o.contains("gl_Position = (mat4(1.0) * gl_Vertex)"));
    }

    #[test]
    fn user_defined_ftransform_is_preserved() {
        let o = translate(
            "#version 330 core\nvec4 ftransform() { return vec4(1.0); }\nvoid main() { gl_Position = ftransform(); }\n",
        )
        .unwrap();
        assert!(o.contains("vec4 ftransform()"));
        assert!(o.contains("gl_Position = ftransform()"));
    }

    #[test]
    fn bsl_integer_lod_and_uniform_globals_are_es_compatible() {
        let source = "#version 330 core\nuniform int frameCounter;\nuniform ivec2 eyeBrightnessSmooth;\nuniform float viewHeight;\nuniform float viewWidth;\nuniform float aspectRatio;\nuniform float frameTimeCounter;\nuniform float timeAngle;\nuniform vec3 sunVec;\nuniform vec4 weatherRain;\nuniform vec4 weatherCold;\nuniform float isCold;\nuniform float weatherWeight;\nuniform sampler2D tex;\nin vec2 uv;\nfloat eBS = eyeBrightnessSmooth.y / 240.0;\nfloat ph = 0.8 / min(720.0, viewHeight);\nfloat pw = ph / aspectRatio;\nfloat frametime = frameTimeCounter * 1.0;\nfloat sunVisibility = clamp(frameTimeCounter, 0.0, 1.0);\nvec2 view = vec2(1.0 / viewWidth, 1.0 / viewHeight);\nfloat pi2wt = frameTimeCounter * 6.28;\nvec3 lightVec = sunVec * ((timeAngle < 0.5325) ? 1.0 : -1.0);\nvec4 weatherCol = mix(weatherRain, weatherCold * isCold / max(weatherWeight, 1e-4), weatherWeight);\nvoid main() { float d = fract(frameCounter * 0.618); float sampleSkip = 0.0; bool skip = sampleSkip == 0; vec4 c = textureLod(tex, uv, 0); }\n";
        let o = translate(source).unwrap();
        assert!(o.contains("#define eBS (float(eyeBrightnessSmooth.y) / 240.0)"));
        assert!(o.contains("#define ph (0.8 / min(720.0, viewHeight))"));
        assert!(o.contains("#define pw (ph / aspectRatio)"));
        assert!(o.contains("#define frametime (frameTimeCounter * 1.0)"));
        assert!(o.contains("#define sunVisibility (clamp(frameTimeCounter, 0.0, 1.0))"));
        assert!(o.contains("#define view (vec2(1.0 / viewWidth, 1.0 / viewHeight))"));
        assert!(o.contains("#define pi2wt (frameTimeCounter * 6.28)"));
        assert!(o.contains("#define lightVec (sunVec * ((timeAngle < 0.5325) ? 1.0 : -1.0))"));
        assert!(o.contains("#define weatherCol (mix(weatherRain, weatherCold * isCold / max(weatherWeight, 1e-4), weatherWeight))"));
        assert!(o.contains("float(frameCounter) * 0.618"));
        assert!(o.contains("sampleSkip == 0.0"));
        assert!(o.contains("textureLod(tex, uv, float(0))"), "{o}");
    }

    #[test]
    fn bsl_moon_weather_and_float_ternary_are_es_compatible() {
        let source = "#version 330 core\nuniform float frameTimeCounter;\nuniform vec3 sunVec;\nuniform vec3 upVec;\nuniform float isCold;\nuniform float isDesert;\nfloat moonVisibility = clamp(dot(-sunVec, upVec) + 0.05, 0.0, 1.0);\nfloat weatherWeight = isCold + isDesert;\nvoid main() { float skip = 1.0; float sampleDepth = 1.0; float skipDepth = 1.0; skip = (sampleDepth < skipDepth) ? 0 : skip; }\n";
        let o = translate(source).unwrap();
        assert!(o.contains("#define moonVisibility (clamp(dot(-sunVec, upVec) + 0.05, 0.0, 1.0))"));
        assert!(o.contains("#define weatherWeight (isCold + isDesert)"));
        assert!(o.contains("? 0.0 : skip"));
    }

    #[test]
    fn iris_dependent_global_initializers_become_macros() {
        let source = "#version 330 core\nuniform float timeAngle;\nuniform float timeBrightness;\nvec3 blocklightColSqrt = vec3(1.0);\nfloat mefade = 1.0 - clamp(abs(timeAngle - 0.5) * 8.0 - 1.5, 0.0, 1.0);\nfloat dfade = 1.0 - pow(1.0 - timeBrightness, 1.5);\nvec3 blocklightCol = blocklightColSqrt * blocklightColSqrt;\nvoid main() {}\n";
        let o = translate(source).unwrap();
        assert!(o.contains("#define mefade (1.0 - clamp(abs(timeAngle - 0.5) * 8.0 - 1.5, 0.0, 1.0))"));
        assert!(o.contains("#define dfade (1.0 - pow(1.0 - timeBrightness, 1.5))"));
        assert!(o.contains("#define blocklightCol (blocklightColSqrt * blocklightColSqrt)"));
    }

    #[test]
    fn iris_sun_color_globals_become_macros() {
        let source = "#version 330 core\nvec3 lightMorning;\nvec3 lightEvening;\nvec3 lightDay;\nfloat mefade;\nfloat dfade;\nvec3 lightSun = mix(mix(lightMorning, lightEvening, mefade), lightDay, dfade);\nvec3 ambientMorning;\nvec3 ambientEvening;\nvec3 ambientDay;\nvec3 ambientSun = mix(mix(ambientMorning, ambientEvening, mefade), ambientDay, dfade);\nvoid main() {}\n";
        let o = translate(source).unwrap();
        assert!(o.contains("#define lightSun (mix(mix(lightMorning, lightEvening, mefade), lightDay, dfade))"));
        assert!(o.contains("#define ambientSun (mix(mix(ambientMorning, ambientEvening, mefade), ambientDay, dfade))"));
    }

    #[test]
    fn desktop_ftransform_builtin_is_rewritten_without_a_definition() {
        let o = translate("#version 150\nvoid main() { gl_Position = ftransform(); }\n").unwrap();
        assert!(o.contains("gl_Position = (mat4(1.0) * gl_Vertex)"));
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
