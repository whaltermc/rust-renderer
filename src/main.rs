use std::collections::HashSet;
fn collect_function_param_names(src: &str) -> HashSet<String> {
    const VALUE_TYPES: &[&str] = &[
        "float", "double", "int", "uint", "bool", "vec2", "vec3", "vec4", "ivec2",
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
                                let p = p.trim();
                                if let Some(pname) = p.split_whitespace().last() {
                                    let pname = pname.trim_end_matches(')').to_string();
                                    if !pname.is_empty() {
                                        params.insert(pname);
                                    }
                                }
                                if p.contains(')') {
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
                let p = p.trim();
                if let Some(pname) = p.split_whitespace().last() {
                    let pname = pname.trim_end_matches(')').to_string();
                    if !pname.is_empty() {
                        params.insert(pname);
                    }
                }
                if p.contains(')') {
                    in_param_list = false;
                    break;
                }
            }
        }
    }
    params
}
fn main() {
    let src = std::fs::read_to_string("/tmp/kilo/out2/262.out").unwrap();
    let params = collect_function_param_names(&src);
    println!("Params in 262.out:");
    for p in params.iter().sorted() { println!("  {p}"); }
}
