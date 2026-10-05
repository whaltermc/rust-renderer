//! Widening of integer literals for the desktop -> GLSL ES retarget.
//!
//! Desktop GLSL converts an `int` to `float` wherever a float is what the expression needs:
//! on the far side of an arithmetic operator, as a function argument, or on the right of an
//! assignment. GLSL ES has no such conversion at all -- glslang rejects `2 * pi`, `max(v, 0)`
//! and `f = 0` at every ES version, while its own desktop frontend accepts all three -- so a
//! desktop shader that leans on the conversion compiles on the desktop driver and fails the
//! moment it is retargeted at ES. Shader packs lean on it constantly: before this pass, 33 of
//! the 187 real Iris shaders in the 1.21.4 capture failed to compile for exactly this reason.
//!
//! Widening the literal to a float literal restores the conversion without needing a full type
//! checker. The conversion is value-preserving, so `2 * pi` and `2.0 * pi` are the same
//! expression, and the rewrite only fires where the surrounding expression is known to be
//! floating point. Integer contexts -- shifts, bitwise operators, subscripts, `u`-suffixed and
//! hexadecimal literals, arguments of a call that may take integer parameters, and operands
//! whose sibling is an integer variable -- are left alone.

use std::collections::{HashMap, HashSet};

/// Declared types that hold floating-point values, including the float vectors and matrices.
const FLOAT_TYPES: &[&str] = &[
    "float", "vec2", "vec3", "vec4", "mat2", "mat3", "mat4", "mat2x2", "mat2x3", "mat2x4",
    "mat3x2", "mat3x3", "mat3x4", "mat4x2", "mat4x3", "mat4x4",
];

/// Scalar integer types. `bool` is deliberately absent: a `bool` operand is not an integer
/// operand, and treating it as one would suppress rewrites inside ternaries like
/// `(isRightHanded ? 1 : -1)`, which is exactly the case this pass exists to fix.
const INT_SCALAR_TYPES: &[&str] = &["int", "uint"];

/// Integer vector types and their component counts.
const INT_VECTOR_TYPES: &[(&str, usize)] = &[
    ("ivec2", 2),
    ("ivec3", 3),
    ("ivec4", 4),
    ("uvec2", 2),
    ("uvec3", 3),
    ("uvec4", 4),
];

/// Built-in variables whose value is floating point, so `gl_Position.x * 2` widens the `2`.
const FLOAT_BUILTINS: &[&str] = &[
    "gl_Position",
    "gl_PointSize",
    "gl_FragCoord",
    "gl_FragDepth",
    "gl_Color",
    "gl_FragColor",
];

/// Multi-character operators. Longest first, because the scanner takes the longest match;
/// `<<` and `>>` matter because they are shifts rather than arithmetic and must not trigger a
/// rewrite of their operands.
const OPERATORS: &[&str] = &[
    "<<=", ">>=", "...", "++", "--", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+=", "-=",
    "*=", "/=", "%=", "&=", "|=", "^=",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// An identifier or keyword.
    Ident,
    /// An integer literal that is safe to widen: no `u` suffix, not hexadecimal.
    IntLit,
    /// Any other literal: floats, `u`-suffixed and hexadecimal integers.
    OpaqueLit,
    Punct,
}

#[derive(Clone, Copy)]
struct Tok<'a> {
    kind: Kind,
    start: usize,
    end: usize,
    text: &'a str,
}

impl<'a> Tok<'a> {
    fn is_punct(&self, c: char) -> bool {
        self.kind == Kind::Punct && self.text.len() == c.len_utf8() && self.text.starts_with(c)
    }
}

/// What the declared types in a shader tell us about its identifiers.
#[derive(Default)]
struct Declared {
    /// Names whose declared type is known and unambiguous, split by kind so the rewrites can
    /// pick the right conversion. A name that appears with two different types anywhere in the
    /// shader -- `uint pos` in one function, `vec2 pos` in another -- is deliberately absent from
    /// all three: this pass reads declarations flat and has no scope information, so the honest
    /// answer for such a name is that its type is unknown.
    floats: HashSet<String>,
    int_scalars: HashSet<String>,
    /// Integer vectors by name, with their component count, so a vector can be converted with
    /// the matching `vecN(...)` rather than `float(...)`.
    int_vectors: HashMap<String, usize>,
    /// Functions returning an integer, so `unpack(a) * 2.0` can be converted too. A call is
    /// retyped only from its own return type -- never by retyping the name in place.
    int_funcs: HashMap<String, Option<usize>>,
}

/// The declared type of a variable, as far as this pass cares about it.
#[derive(Clone, Copy, PartialEq)]
enum Ty {
    Float,
    IntScalar,
    IntVector(usize),
}

impl Declared {
    /// Whether any part of `text` is known to be a floating-point value.
    fn holds_float(&self, text: &str) -> bool {
        tokenize(text).iter().any(|t| match t.kind {
            Kind::Ident => {
                self.floats.contains(t.text) || FLOAT_BUILTINS.contains(&t.text)
            }
            // A float literal anywhere makes the expression floating point, which is what
            // `a * 2.0` means; the same expression with `a` an integer is not, and this pass
            // only ever reads the token, never rewrites it.
            Kind::OpaqueLit => t.text.contains('.'),
            _ => false,
        })
    }

    /// Whether `text` is a single identifier declared as a float.
    fn is_float_name(&self, text: &str) -> bool {
        let t = text.trim();
        self.is_single_ident(text) && (self.floats.contains(t) || FLOAT_BUILTINS.contains(&t))
    }

    /// How an integer-typed expression has to be converted to become a float.
    ///
    /// `float(...)` only accepts a *scalar*: `float(ivec2Value)` is a conversion of the first
    /// component, not a componentwise one, and glslang rejects it where a vec2 is expected. So
    /// the two cases are distinguished here and an integer vector is converted with the matching
    /// `vecN(...)`. A single component selected out of an integer vector (`uv.y`, `uv[0]`) *is*
    /// a scalar and converts with `float(...)`.
    fn retype_of(&self, text: &str) -> Option<Edit> {
        let toks = tokenize(text);
        let first = *toks.first()?;
        if first.kind != Kind::Ident {
            return None;
        }
        if self.int_scalars.contains(first.text) {
            // A bare name only. Anything chained onto a scalar -- a call, a component -- has an
            // unknown result type, and guessing at it is worse than leaving it alone.
            return (toks.len() == 1).then_some(Edit::FloatCast);
        }
        // A call: `unpack(data) * VERTEX_SCALE`. The result type is the function's return type,
        // and the conversion wraps the whole call rather than the name inside it.
        if toks.get(1).is_some_and(|t| t.is_punct('(')) {
            let close = matching_forward(&toks, 1, '(', ')')?;
            if close != toks.len() - 1 {
                return None;
            }
            return match *self.int_funcs.get(first.text)? {
                None => Some(Edit::FloatCast),
                Some(components) => Some(Edit::VectorCast(components)),
            };
        }
        let components = *self.int_vectors.get(first.text)?;
        match selection(&toks) {
            Selection::Whole => Some(Edit::VectorCast(components)),
            // `.y` / `[1]`: one component out of the vector, so a scalar.
            Selection::Width(1) => Some(Edit::FloatCast),
            // `.xy` or a longer chain: a differently-sized vector, or an operator. Either way
            // retyping it is not this pass's business.
            Selection::Width(_) | Selection::NotAChain => None,
        }
    }

    fn is_single_ident(&self, text: &str) -> bool {
        let t = text.trim();
        !t.is_empty() && !t.contains(|c: char| !(c.is_alphanumeric() || c == '_'))
    }

    /// Whether `text` is an expression made only of integer literals, integer variables, and
    /// integer operators -- e.g. `power`, `-power`, `COEFF_COUNT - 1`. Such an expression passed
    /// to a float function needs an explicit `float(...)` cast in ES.
    fn is_int_expr(&self, text: &str) -> bool {
        let toks = tokenize(text);
        if toks.is_empty() {
            return false;
        }
        for (index, t) in toks.iter().enumerate() {
            match t.kind {
                Kind::IntLit => {}
                Kind::OpaqueLit => return false,
                Kind::Ident => {
                    if self.floats.contains(t.text)
                        || FLOAT_BUILTINS.contains(&t.text)
                        || is_builtin_float_call(t.text)
                    {
                        return false;
                    }
                    if !(self.int_scalars.contains(t.text)
                        || self.int_vectors.contains_key(t.text)
                        || self.int_funcs.contains_key(t.text))
                    {
                        return false;
                    }
                }
                Kind::Punct => {
                    if !matches!(t.text, "(" | ")" | "?" | ":" | "," | "-" | "+" | "*" | "&&" | "||" | "<" | ">" | "<=" | ">=" | "==" | "!=") {
                        return false;
                    }
                    if t.text == "(" && index > 0 && toks[index - 1].kind == Kind::Ident {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Whether `text` is an expression made only of integer literals and boolean values --
    /// `1`, `-1`, `(isRightHanded ? 1 : -1)`. Widening every literal in one of these makes it
    /// floating point without changing any value, which is how a parenthesised integer operand
    /// next to a float gets fixed. Anything with a variable, a call or a mixed operator in it
    /// is rejected, because there is no way to retype it safely.
    fn is_pure_int_expr(&self, text: &str) -> bool {
        let toks = tokenize(text);
        if toks.is_empty() {
            return false;
        }
        for (index, t) in toks.iter().enumerate() {
            match t.kind {
                Kind::IntLit => {}
                Kind::OpaqueLit => return false,
                Kind::Ident => {
                    // `bool` locals are integers in the ternary sense but not in the numeric one;
                    // anything this pass declared as float or int disqualifies the expression.
                    if self.floats.contains(t.text)
                        || self.int_scalars.contains(t.text)
                        || self.int_vectors.contains_key(t.text)
                        || self.int_funcs.contains_key(t.text)
                        || FLOAT_BUILTINS.contains(&t.text)
                        || is_builtin_float_call(t.text)
                    {
                        return false;
                    }
                }
                Kind::Punct => {
                    if !matches!(t.text, "(" | ")" | "?" | ":" | "," | "-" | "+" | "*" | "&&" | "||" | "<" | ">" | "<=" | ">=" | "==" | "!=") {
                        return false;
                    }
                    // A `(` that follows an identifier is a call. Widening the literals *inside* it
                    // would retype that function's arguments, which may well be integer
                    // parameters: `GetHandItem(50)` is an int call, not an int expression whose
                    // 50 should become 50.0.
                    if t.text == "(" && index > 0 && toks[index - 1].kind == Kind::Ident {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Built-in calls that return a float, so `dot(a, b) * 2` widens the `2`.
fn is_builtin_float_call(name: &str) -> bool {
    matches!(
        name,
        "length"
            | "distance"
            | "dot"
            | "determinant"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "pow"
            | "exp"
            | "log"
            | "exp2"
            | "log2"
            | "sqrt"
            | "inversesqrt"
            | "fract"
            | "mod"
            | "min"
            | "max"
            | "clamp"
            | "mix"
            | "step"
            | "smoothstep"
            | "floor"
            | "trunc"
            | "round"
            | "ceil"
            | "reflect"
    )
}

/// Built-in calls whose every parameter must be float/vector in ES 3.00, because they have
/// no integer overloads at all. Integer arguments here always need an explicit cast.
fn is_float_only_builtin_call(name: &str) -> bool {
    matches!(
        name,
        "length"
            | "distance"
            | "dot"
            | "determinant"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "pow"
            | "exp"
            | "log"
            | "exp2"
            | "log2"
            | "sqrt"
            | "inversesqrt"
            | "fract"
            | "smoothstep"
            | "floor"
            | "trunc"
            | "round"
            | "ceil"
            | "reflect"
    )
}

fn tokenize(line: &str) -> Vec<Tok<'_>> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < line.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // Comments are opaque: an operator or identifier inside one must not look like code.
        if line[i..].starts_with("//") {
            out.push(Tok { kind: Kind::OpaqueLit, start: i, end: line.len(), text: &line[i..] });
            break;
        }
        if line[i..].starts_with("/*") {
            let end = line[i + 2..].find("*/").map_or(line.len(), |j| i + 2 + j + 2);
            out.push(Tok { kind: Kind::OpaqueLit, start: i, end, text: &line[i..end] });
            i = end;
            continue;
        }
        let start = i;
        if c.is_ascii_alphabetic() || c == '_' {
            while i < line.len() && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push(Tok { kind: Kind::Ident, start, end: i, text: &line[start..i] });
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && matches!(bytes.get(i + 1), Some(d) if d.is_ascii_digit())) {
            i = scan_literal(line, i);
            out.push(Tok { kind: literal_kind(&line[start..i]), start, end: i, text: &line[start..i] });
            continue;
        }
        let mut matched = None;
        for op in OPERATORS {
            if line[i..].starts_with(op) {
                matched = Some(op.len());
                break;
            }
        }
        let len = matched.unwrap_or(c.len_utf8());
        out.push(Tok { kind: Kind::Punct, start, end: i + len, text: &line[start..i + len] });
        i += len;
    }
    out
}

/// Scans a numeric literal, including the sign of an exponent so that `1.0E-5` is one token.
///
/// Consuming the exponent sign matters: treating it as a binary minus would make the `-5` look
/// like an integer operand, and this pass would widen it into `1.0E-5.0`.
fn scan_literal(line: &str, mut i: usize) -> usize {
    let bytes = line.as_bytes();
    // Hexadecimal: 0x1F, 0X1fu.
    if bytes[i] == b'0' && matches!(bytes.get(i + 1), Some(b'x' | b'X')) {
        i += 2;
        while i < line.len() && (bytes[i] as char).is_ascii_hexdigit() {
            i += 1;
        }
        return i + literal_suffix_len(line, i);
    }
    while i < line.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
        i += 1;
    }
    // Exponent, with its optional sign.
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        let mut after = i + 1;
        if matches!(bytes.get(after), Some(b'+' | b'-')) {
            after += 1;
        }
        if matches!(bytes.get(after), Some(d) if d.is_ascii_digit()) {
            while matches!(bytes.get(after), Some(d) if d.is_ascii_digit()) {
                after += 1;
            }
            i = after;
        }
    }
    i + literal_suffix_len(line, i)
}

/// Length of the `u`/`f`/`l` type suffix on a literal, if any.
fn literal_suffix_len(line: &str, i: usize) -> usize {
    match line.as_bytes().get(i) {
        Some(b'u' | b'U' | b'f' | b'F' | b'l' | b'L') => 1,
        _ => 0,
    }
}

/// Classifies a numeric literal, separating the integer literals this pass may widen from the
/// ones it must not touch: hexadecimal and `u`-suffixed literals are unambiguously unsigned
/// integers, and dotted, exponential and `f`-suffixed literals are already floats.
fn literal_kind(text: &str) -> Kind {
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("0x") {
        return Kind::OpaqueLit;
    }
    let mantissa = lower.split(['e', 'f', 'u', 'l']).next().unwrap_or(&lower);
    if mantissa.contains('e') || mantissa.contains('.') {
        return Kind::OpaqueLit;
    }
    if lower.ends_with('u') || lower.ends_with('l') {
        return Kind::OpaqueLit;
    }
    if lower.contains('e') || lower.ends_with('f') {
        return Kind::OpaqueLit;
    }
    Kind::IntLit
}

/// How many components a member or subscript chain selects from a vector, or `None` when the
/// chain does not select any. `uv.y` and `uv[0]` select one, `uv.xy` two, `uv` none.
/// What follows the root identifier of an integer-vector expression.
#[derive(PartialEq)]
enum Selection {
    /// Nothing, or a call: the value keeps the type it already has and converts as a whole.
    Whole,
    /// A swizzle or index selecting this many components.
    Width(usize),
    /// Not an access chain at all -- an operator, or something this pass will not model.
    NotAChain,
}

/// Reads the access chain that follows the root identifier in `toks`, which starts at the name.
fn selection(toks: &[Tok<'_>]) -> Selection {
    let mut index = 1;
    let mut selected = 0usize;
    while index < toks.len() {
        match (toks.get(index), toks.get(index + 1)) {
            (Some(dot), Some(field)) if dot.is_punct('.') && field.kind == Kind::Ident => {
                // A swizzle names one component per letter; anything else is a struct field,
                // whose type this pass does not model.
                let all_component = !field.text.is_empty()
                    && field.text.bytes().all(|b| {
                        matches!(b, b'x' | b'y' | b'z' | b'w' | b'r' | b'g' | b'b' | b'a'
                            | b's' | b't' | b'p' | b'q')
                    });
                if !all_component {
                    return Selection::NotAChain;
                }
                selected += field.text.len();
                index += 2;
            }
            (Some(open), _) if open.is_punct('[') => {
                let Some(close) = matching_forward(toks, index, '[', ']') else {
                    return Selection::NotAChain;
                };
                if !toks[index + 1..close].iter().all(|t| t.kind == Kind::IntLit) {
                    return Selection::NotAChain;
                }
                selected += 1;
                index = close + 1;
            }
            // A call: the callee's declared return type is what this expression has, so it
            // converts as a whole rather than selecting components.
            (Some(open), _) if open.is_punct('(') => {
                let Some(close) = matching_forward(toks, index, '(', ')') else {
                    return Selection::NotAChain;
                };
                index = close + 1;
            }
            _ => return Selection::NotAChain,
        }
    }
    if selected == 0 { Selection::Whole } else { Selection::Width(selected) }
}

/// Collects every identifier declared with a known numeric type, across globals, function
/// parameters and locals: `<type> <name>` and its comma-separated continuations.
fn collect_declared<'a>(src: &'a str) -> Declared {
    // Name -> type, then conflicting names dropped. Doing it this way means a name can never
    // end up believing it is both a float and an integer.
    let mut vars: HashMap<&str, Ty> = HashMap::new();
    let mut conflicted: HashSet<&str> = HashSet::new();
    let mut funcs: HashMap<&str, Option<usize>> = HashMap::new();

    let observe = |name: &'a str, ty: Ty, vars: &mut HashMap<&'a str, Ty>, conflicted: &mut HashSet<&'a str>| {
        match vars.get(name) {
            Some(previous) if *previous != ty => {
                conflicted.insert(name);
            }
            Some(_) => {}
            None => {
                vars.insert(name, ty);
            }
        }
    };

    for line in src.lines() {
        // A preprocessor line has no type keyword, and `#define A float` would otherwise look
        // like a declaration of a type.
        if line.trim_start().starts_with('#') {
            continue;
        }
        let toks = tokenize(line);
        for (index, tok) in toks.iter().enumerate() {
            if tok.kind != Kind::Ident {
                continue;
            }
            let ty = if FLOAT_TYPES.contains(&tok.text) {
                Some(Ty::Float)
            } else if INT_SCALAR_TYPES.contains(&tok.text) {
                Some(Ty::IntScalar)
            } else {
                INT_VECTOR_TYPES.iter().find(|(t, _)| *t == tok.text).map(|(_, n)| Ty::IntVector(*n))
            };
            let Some(ty) = ty else { continue };
            // The token after the type must be a name: `floatx` and a bare `vec3` are not
            // declarations. Pointer stars (`float *x`) are stepped over.
            let mut next = index + 1;
            while let Some(t) = toks.get(next) {
                if t.is_punct('*') {
                    next += 1;
                    continue;
                }
                break;
            }
            let Some(name) = toks.get(next) else { continue };
            if name.kind != Kind::Ident {
                continue;
            }
            // `uvec3 unpack(uvec2 data)` declares a function: its return type is what a call
            // needs, and recording the name as a variable would let a rewrite wrap the callee.
            if toks.get(next + 1).is_some_and(|t| t.is_punct('(')) {
                if let Some(components) = match ty {
                    Ty::IntScalar => Some(None),
                    Ty::IntVector(n) => Some(Some(n)),
                    Ty::Float => None,
                } {
                    funcs.entry(name.text).or_insert(components);
                }
                continue;
            }
            observe(name.text, ty, &mut vars, &mut conflicted);
            // `float a = f(x), b = g(y);` declares both, and a parameter list runs the same way.
            let mut scan = next;
            while let Some(t) = toks.get(scan + 1) {
                if !t.is_punct(',') {
                    break;
                }
                match toks.get(scan + 2) {
                    Some(after) if after.kind == Kind::Ident => {
                        // The next declarator's own type keyword is not a name.
                        let after_is_type = FLOAT_TYPES.contains(&after.text)
                            || INT_SCALAR_TYPES.contains(&after.text)
                            || INT_VECTOR_TYPES.iter().any(|(t, _)| *t == after.text);
                        if !after_is_type {
                            observe(after.text, ty, &mut vars, &mut conflicted);
                        }
                        scan += 2;
                    }
                    Some(after) if ty_matches(after, ty) => {
                        // `float f(vec2 a, vec3 b)` -- the list carries its own types.
                        observe(toks[scan + 3].text, ty_of(&toks[scan + 3]), &mut vars, &mut conflicted);
                        scan += 3;
                    }
                    _ => break,
                }
            }
        }
    }

    let mut declared = Declared::default();
    for (name, ty) in vars {
        if conflicted.contains(name) {
            continue;
        }
        match ty {
            Ty::Float => {
                declared.floats.insert(name.to_string());
            }
            Ty::IntScalar => {
                declared.int_scalars.insert(name.to_string());
            }
            Ty::IntVector(n) => {
                declared.int_vectors.insert(name.to_string(), n);
            }
        }
    }
    declared.int_funcs = funcs.into_iter().map(|(n, c)| (n.to_string(), c)).collect();
    declared
}

/// Whether `tok` is the type keyword matching a declarator's `ty`.
fn ty_matches(tok: &Tok<'_>, ty: Ty) -> bool {
    match ty {
        Ty::Float => FLOAT_TYPES.contains(&tok.text),
        Ty::IntScalar => INT_SCALAR_TYPES.contains(&tok.text),
        Ty::IntVector(n) => INT_VECTOR_TYPES.contains(&(tok.text, n)),
    }
}

fn ty_of(tok: &Tok<'_>) -> Ty {
    if FLOAT_TYPES.contains(&tok.text) {
        Ty::Float
    } else if INT_SCALAR_TYPES.contains(&tok.text) {
        Ty::IntScalar
    } else {
        INT_VECTOR_TYPES
            .iter()
            .find(|(t, _)| *t == tok.text)
            .map_or(Ty::Float, |(_, n)| Ty::IntVector(*n))
    }
}

/// The token range of the operand ending at `index - 1`, if there is a simple one there.
fn operand_before(toks: &[Tok<'_>], index: usize) -> Option<(usize, usize)> {
    let tok = *toks.get(index.checked_sub(1)?)?;
    match tok.kind {
        Kind::IntLit | Kind::OpaqueLit => Some((index - 1, index)),
        Kind::Ident => Some(back_over_chain(toks, index - 1)),
        Kind::Punct if tok.is_punct(')') => {
            let open = matching_back(toks, index - 1, '(', ')')?;
            let mut first = open;
            if open > 0 && toks[open - 1].kind == Kind::Ident {
                first = open - 1;
            }
            // `index - 1` is the `)`, so the operand ends there: `index` is exclusive. Returning
            // `index + 1` would swallow the operator that follows, which then read as part of
            // the call's argument list.
            Some((first, index))
        }
        Kind::Punct if tok.is_punct('!') => operand_before(toks, index - 1),
        _ => None,
    }
}

/// The token range of the operand starting at `index + 1`, if there is a simple one there.
fn operand_after(toks: &[Tok<'_>], index: usize) -> Option<(usize, usize)> {
    let tok = *toks.get(index + 1)?;
    match tok.kind {
        Kind::IntLit | Kind::OpaqueLit => Some((index + 1, index + 2)),
        Kind::Ident => Some((index + 1, forward_over_chain(toks, index + 1))),
        Kind::Punct if tok.is_punct('(') => {
            let close = matching_forward(toks, index + 1, '(', ')')?;
            let mut first = index + 1;
            if index > 0 && toks[index].kind == Kind::Ident {
                first = index;
            }
            Some((first, forward_over_chain(toks, close)))
        }
        Kind::Punct if tok.is_punct('!') => operand_after(toks, index + 1),
        _ => None,
    }
}

/// Extends an operand whose last token is at `last` over `.field` accesses and `[...]`
/// subscripts, returning the exclusive end of the operand.
fn forward_over_chain(toks: &[Tok<'_>], last: usize) -> usize {
    // `end` is the index just past the operand, so the next token to look at is `end` itself.
    let mut end = last + 1;
    loop {
        match toks.get(end) {
            Some(dot) if dot.is_punct('.') => match toks.get(end + 1) {
                Some(field) if field.kind == Kind::Ident => end += 2,
                _ => return end,
            },
            Some(open) if open.is_punct('[') => match matching_forward(toks, end, '[', ']') {
                Some(close) => end = close + 1,
                None => return end,
            },
            // `vec3(1.0)` is one operand, not the bare constructor name: without this the
            // float-ness of the expression stays invisible and nothing gets retyped.
            Some(open) if open.is_punct('(') && toks.get(end - 1).is_some_and(|t| t.kind == Kind::Ident) => {
                match matching_forward(toks, end, '(', ')') {
                    Some(close) => end = close + 1,
                    None => return end,
                }
            }
            _ => return end,
        }
    }
}

/// Extends an operand starting at `first` back over `.field` accesses and `[...]` subscripts.
fn back_over_chain(toks: &[Tok<'_>], first: usize) -> (usize, usize) {
    let mut start = first;
    let mut end = first + 1;
    loop {
        if start >= 1 && toks[start - 1].is_punct('.') && start >= 2 && toks[start - 2].kind == Kind::Ident {
            start -= 2;
            continue;
        }
        if start >= 1 && toks[start - 1].is_punct(']') {
            match matching_back(toks, start - 1, '[', ']') {
                Some(open) => {
                    start = open;
                    // `name[i]` -- the name is part of the operand.
                    if start >= 1 && toks[start - 1].kind == Kind::Ident {
                        start -= 1;
                    }
                    end = end.max(start + 1);
                    continue;
                }
                None => return (start, end),
            }
        }
        return (start, end);
    }
}

fn matching_forward(toks: &[Tok<'_>], open: usize, want: char, close: char) -> Option<usize> {
    let mut depth = 0;
    for (index, tok) in toks.iter().enumerate().skip(open) {
        if tok.is_punct(want) {
            depth += 1;
        } else if tok.is_punct(close) {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn matching_back(toks: &[Tok<'_>], close: usize, want: char, close_ch: char) -> Option<usize> {
    let mut depth = 0;
    for index in (0..=close).rev() {
        if toks[index].is_punct(close_ch) {
            depth += 1;
        } else if toks[index].is_punct(want) {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq)]
enum Edit {
    /// Replace an integer literal with its floating-point equivalent.
    Widen,
    /// Wrap a scalar integer expression in `float(...)`.
    FloatCast,
    /// Wrap an integer vector in the matching `vecN(...)`, which converts componentwise.
    VectorCast(usize),
}

/// A pending rewrite, as the inclusive token range it covers plus what to do with it.
/// Ranges rather than single tokens because `FloatCast` wraps the whole operand --
/// `float(eyeBrightnessSmooth.y)`, not `float(eyeBrightnessSmooth).y`.
type EditAt = (usize, usize, Edit);

/// Widens the integer literals in `line` that a desktop compiler would have converted to
/// float, and returns the rewritten line.
fn widen_line(line: &str, declared: &Declared) -> String {
    let toks = tokenize(line);
    if toks.is_empty() {
        return line.to_string();
    }
    // Overlapping rewrites of the same operand are de-duplicated by start index.
    let mut edits: Vec<EditAt> = Vec::new();
    let text_of = |range: (usize, usize)| {
        &line[toks[range.0].start..toks[range.1 - 1].end]
    };
    // The integer operand of an operation whose other operand is float. Desktop GLSL converted
    // it; ES does not, so it has to be converted here. A literal widens, an integer-typed
    // expression gets a cast, and a parenthesised integer expression widens literal by literal.
    let retarget = |int_side: (usize, usize), edits: &mut Vec<EditAt>| {
        if toks[int_side.0].is_punct('(') && declared.is_pure_int_expr(text_of(int_side)) {
            for offset in 0..int_side.1 - int_side.0 {
                let one = (int_side.0 + offset, int_side.0 + offset + 1);
                if toks[one.0].kind == Kind::IntLit {
                    edits.push((one.0, one.1, Edit::Widen));
                }
            }
        } else if let Some(edit) = declared.retype_of(text_of(int_side)) {
            edits.push((int_side.0, int_side.1, edit));
        } else if toks[int_side.0].kind == Kind::IntLit && int_side.1 == int_side.0 + 1 {
            edits.push((int_side.0, int_side.1, Edit::Widen));
        } else if declared.is_int_expr(text_of(int_side)) {
            edits.push((int_side.0, int_side.1, Edit::FloatCast));
        }
    };

    for index in 0..toks.len() {
        let tok = toks[index];

        // Function arguments: `max(v.x, 0)` needs the `0` widened once a sibling argument is a
        // float, because ES has no implicit conversion for arguments either. Restricted to the
        // known float builtins: their every parameter is a float, whereas `textureLod` on an
        // integer sampler and `texelFetch` both take integer parameters that must stay integers.
        if tok.kind == Kind::Ident
            && is_builtin_float_call(tok.text)
            && toks.get(index + 1).is_some_and(|t| t.is_punct('('))
        {
            if let Some(close) = matching_forward(&toks, index + 1, '(', ')') {
                let args = split_args(&toks, index + 1, close);
                let unconditional = is_float_only_builtin_call(tok.text);
                if unconditional || args.iter().any(|arg| declared.holds_float(text_of(*arg))) {
                    for arg in args {
                        retarget(arg, &mut edits);
                    }
                }
            }
        }

        if tok.kind != Kind::Punct {
            continue;
        }
        match tok.text {
            // Arithmetic and comparison operators. Desktop GLSL evaluated all of these by
            // converting the integer side to float, so `float == int` is exactly as invalid in
            // ES as `float * int` is.
            "*" | "/" | "+" | "-" | "==" | "!=" | "<" | ">" | "<=" | ">=" => {
                let (Some(left), Some(right)) =
                    (operand_before(&toks, index), operand_after(&toks, index))
                else {
                    continue;
                };
                match (
                    declared.holds_float(text_of(left)),
                    declared.holds_float(text_of(right)),
                ) {
                    // Both sides are already float, so there is no conversion to make.
                    (true, true) => {}
                    (true, false) => retarget(right, &mut edits),
                    (false, true) => retarget(left, &mut edits),
                    (false, false) => {}
                }
            }
            // Plain assignment: `float f; ... f = 0;` is an error in ES, legal on desktop.
            "=" => {
                let (Some(left), Some(right)) =
                    (operand_before(&toks, index), operand_after(&toks, index))
                else {
                    continue;
                };
                if declared.is_float_name(text_of(left)) {
                    retarget(right, &mut edits);
                }
            }
            _ => {}
        }
    }

    if edits.is_empty() {
        return line.to_string();
    }
    // Keep the widest rewrite when ranges overlap: a `float(...)` cast covers the whole
    // operand, so it supersedes any literal widening inside it.
    edits.sort_by_key(|(first, last, _)| (*first, *last));
    edits.dedup_by_key(|(first, last, _)| (*first, *last));

    let mut out = String::with_capacity(line.len() + edits.len() * 8);
    let mut cursor = 0;
    let mut skip_until = 0;
    for (first, last, edit) in edits {
        // Ranges are half-open token indices, so `last - 1` is the final token covered.
        let start = toks[first].start;
        if start < skip_until {
            continue;
        }
        let end = toks[last - 1].end;
        out.push_str(&line[cursor..start]);
        let operand = &line[start..end];
        match edit {
            Edit::Widen => {
                out.push_str(operand);
                out.push_str(".0");
            }
            Edit::FloatCast => {
                out.push_str("float(");
                out.push_str(operand);
                out.push(')');
            }
            Edit::VectorCast(n) => {
                out.push_str(&format!("vec{n}("));
                out.push_str(operand);
                out.push(')');
            }
        }
        cursor = end;
        skip_until = end;
    }
    out.push_str(&line[cursor..]);
    out
}

/// Splits a call's argument list at top-level commas.
fn split_args(toks: &[Tok<'_>], open: usize, close: usize) -> Vec<(usize, usize)> {
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = open + 1;
    for (index, tok) in toks.iter().enumerate().take(close).skip(open + 1) {
        if tok.is_punct('(') || tok.is_punct('[') {
            depth += 1;
        } else if tok.is_punct(')') || tok.is_punct(']') {
            depth = depth.saturating_sub(1);
        } else if tok.is_punct(',') && depth == 0 {
            if start < index {
                args.push((start, index));
            }
            start = index + 1;
        }
    }
    if start < close {
        args.push((start, close));
    }
    args
}

/// Widens every integer literal in `src` that desktop GLSL would have converted to float.
///
/// Operates on the whole source because the decision depends on declared types, and leaves the
/// line count and every non-rewritten byte alone so driver diagnostics still line up.
/// Name of the helper [`rewrite_int_mod`] introduces at the call sites it rewrites.
const INT_MOD_HELPER: &str = "rust_mod_int";

/// The helper body, injected into a translated shader that needs it.
///
/// GLSL ES has no integer `mod`: only `float mod(float, float)`, and ES does not convert an
/// integer to float implicitly, so `mod(blockEntityId - 10000, 0)` matches no overload. Desktop
/// GLSL 4.0 and later do provide integer `mod`, which is where shader packs pick the habit up.
/// This reproduces it exactly: GLSL defines `mod` as `x - y * floor(x / y)`, and integer division
/// truncates toward zero, so the truncated remainder needs one sign fixup to become a floored
/// one. It stays in integer arithmetic, so no precision is lost on large coordinates.
const INT_MOD_HELPER_BODY: &str = "\
int rust_mod_int(int x, int y) {
int r = x - y * (x / y);
return (r != 0 && ((r < 0) != (y < 0))) ? r + y : r;
}
";

/// Rewrites calls to `mod` whose arguments are all integers, and reports whether it did.
///
/// A float `mod` is left alone: ES provides it. The decision is made after
/// [`widen_int_literals`] has run, so a call that began with one float argument has already had
/// its integer siblings widened and is now a float call.
pub(crate) fn rewrite_int_mod(src: &str) -> (String, bool) {
    let declared = collect_declared(src);
    let mut out = String::with_capacity(src.len() + 16);
    let mut rewritten_any = false;
    for line in src.lines() {
        let toks = tokenize(line);
        // Only a `mod` that starts the line's expression can be a bare call; anything with a
        // receiver is a method-like construct this pass does not model.
        let mut cursor = 0;
        for index in 0..toks.len() {
            if toks[index].kind != Kind::Ident
                || toks[index].text != "mod"
                || !toks.get(index + 1).is_some_and(|t| t.is_punct('('))
            {
                continue;
            }
            // A `.` in front means this is a member access rather than the builtin.
            if index > 0 && toks[index - 1].is_punct('.') {
                continue;
            }
            let Some(close) = matching_forward(&toks, index + 1, '(', ')') else {
                continue;
            };
            let args = split_args(&toks, index + 1, close);
            if args.is_empty() {
                continue;
            }
            let span = |range: (usize, usize)| &line[toks[range.0].start..toks[range.1 - 1].end];
            // Any float in the argument list means ES's own float overload already applies.
            if args.iter().any(|arg| declared.holds_float(span(*arg))) {
                continue;
            }
            out.push_str(&line[cursor..toks[index].start]);
            out.push_str(INT_MOD_HELPER);
            cursor = toks[index].start + 3;
            rewritten_any = true;
        }
        out.push_str(&line[cursor..]);
        out.push('\n');
    }
    (out, rewritten_any)
}

/// The helper definition to inject into a shader that [`rewrite_int_mod`] rewrote.
pub(crate) fn int_mod_helper() -> &'static str {
    INT_MOD_HELPER_BODY
}

pub(crate) fn widen_int_literals(src: &str) -> String {
    let declared = collect_declared(src);
    let mut current = src.to_string();
    // Types propagate through an expression, and each round only sees the text as it was when
    // it started: in `f * 2 - 1` the `2` is widened first because `f` is a float, and only the
    // next round sees that the product is a float and so is the subtraction. Each round is
    // idempotent -- a widened literal is already a float, so nothing is rewritten twice -- and
    // every round that changes something strictly adds a `.0`, so this terminates.
    for _ in 0..8 {
        let mut out = String::with_capacity(current.len() + 128);
        for line in current.lines() {
            out.push_str(&widen_line(line, &declared));
            out.push('\n');
        }
        if out == current {
            break;
        }
        current = out;
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widen(src: &str) -> String {
        widen_int_literals(src)
    }

    /// A call inside the parenthesised operand may take integer parameters, so its arguments must
    /// not be widened. `GetHandItem(50)` returns an int; retargeting it to `GetHandItem(50.0)`
    /// fails with `no matching function for call to GetHandItem(float)`.
    #[test]
    fn call_arguments_are_not_widened_through_a_group() {
        let out = widen(
            "int GetHandItem(int item);\nfloat emissive = (GetHandItem(50) + GetHandItem(89));\n\
             in bool isRightHanded;\n",
        );
        assert!(out.contains("GetHandItem(50) + GetHandItem(89)"), "{out}");
        assert!(!out.contains("50.0"), "{out}");
    }

/// An integer `mod` is legal on desktop and absent from ES, but that is a missing builtin
    /// rather than a conversion: nothing in this expression is a float to convert.
    #[test]
    fn integer_mod_arguments_are_left_alone() {
        let src = "uniform int blockEntityId;\nint blockID = mod(max(blockEntityId - 10000, 0), 10000);\n";
        assert_eq!(widen(src), src);
    }

    #[test]
    fn integer_mod_call_is_retargeted_to_the_helper() {
        let (out, changed) =
            rewrite_int_mod("uniform int blockEntityId;\nint blockID = int(mod(max(blockEntityId - 10000, 0), 10000));\n");
        assert!(changed);
        assert!(out.contains("rust_mod_int(max(blockEntityId - 10000, 0), 10000)"), "{out}");
    }

    /// ES does provide `float mod(float, float)`, so a call that already takes floats must be
    /// left to the driver.
    #[test]
    fn float_mod_call_is_left_to_es() {
        let (out, changed) = rewrite_int_mod("float f = mod(1.5, 2.0);\n");
        assert!(!changed, "{out}");
        assert!(out.contains("mod(1.5, 2.0)"), "{out}");
    }

    /// `a.mod(x)` is a member access, not the builtin.
    #[test]
    fn mod_member_access_is_left_alone() {
        let (out, changed) = rewrite_int_mod("int f(){ return tex.mod(4); }\n");
        assert!(!changed, "{out}");
    }
#[test]
    fn int_literal_beside_float_constant_is_widened() {
        let out = widen("const float pi = 3.1415927;\nfloat f(){ return sin(2 * pi * 0.5); }\n");
        assert!(out.contains("2.0 * pi"), "{out}");
    }

    #[test]
    fn int_literal_beside_float_member_is_widened() {
        let out = widen("in vec4 mc_Entity;\nfloat f(){ return mc_Entity.x - 10000; }\n");
        assert!(out.contains("10000.0"), "{out}");
    }

    #[test]
    fn int_literal_beside_builtin_float_is_widened() {
        let out = widen("float f(){ return gl_Position.x * (2); }\n");
        assert!(out.contains("2.0"), "{out}");
    }

    /// The pattern that failed 28 of the Iris shaders: a parenthesised integer expression
    /// multiplying a float. Widening the literals inside it is what makes the product float.
    #[test]
    fn parenthesised_int_expression_next_to_float_is_widened() {
        let out = widen("in bool isRightHanded;\nfloat f(){ return gl_Position.x * (isRightHanded ? 1 : -1); }\n");
        assert!(out.contains("1.0"), "{out}");
        assert!(out.contains("? 1.0 : -1.0"), "{out}");
    }

    #[test]
    fn int_literal_as_float_argument_is_widened() {
        let out = widen("in vec4 mc_Entity;\nfloat f(){ return mod(max(mc_Entity.x - 10000.0, 0), 10000); }\n");
        assert!(out.contains("max(mc_Entity.x - 10000.0, 0.0)"), "{out}");
    }

    #[test]
    fn int_literal_assigned_to_float_is_widened() {
        let out = widen("out float mat;\nvoid main(){ mat = 0; }\n");
        assert!(out.contains("mat = 0.0;"), "{out}");
    }

    /// `const float S = 32.0 / POSITION_MAX_COORD;` from Sodium's chunk shader: the right side
    /// is an integer variable, so it needs a cast rather than a literal widening.
    #[test]
    fn int_constant_divided_by_float_gets_a_cast() {
        let out = widen("const uint POSITION_MAX_COORD = 1u << 20u;\nconst float S = 32.0 / POSITION_MAX_COORD;\n");
        assert!(out.contains("32.0 / float(POSITION_MAX_COORD)"), "{out}");
    }

    #[test]
    fn integer_contexts_are_untouched() {
        let src = "const uint MASK = 0xFFu;\nint f(){ return (MASK >> 2) & 0x0F | 3; }\n";
        assert_eq!(widen(src), src);
    }

    #[test]
    fn integer_arithmetic_with_an_integer_sibling_is_untouched() {
        let src = "const int N = 4;\nint f(int i){ return (i + 2) * N; }\n";
        assert_eq!(widen(src), src);
    }

    #[test]
    fn subscript_and_int_argument_are_untouched() {
        let src = "uniform int idx;\nin vec4 a[4];\nvoid main(){ a[idx] = vec4(1); }\n";
        assert_eq!(widen(src), src);
    }

    #[test]
    fn shift_and_modulo_operands_are_untouched() {
        let src = "const int SHIFT = 1;\nint f(int i){ return (i << SHIFT) % 8; }\n";
        assert_eq!(widen(src), src);
    }

    #[test]
    fn comments_are_not_rewritten() {
        let src = "// 2 * pi is written out below\nconst float pi = 3.14;\nfloat f(){ return pi; }\n";
        assert_eq!(widen(src), src);
    }

    #[test]
    fn preprocessor_lines_are_not_treated_as_declarations() {
        // `#define SCALE float` must not make SCALE a float variable, and the `1 << 3` in a
        // macro body must survive.
        let src = "#define SHIFTS 1 << 3\nfloat f(){ return 2.0; }\n";
        assert_eq!(widen(src), src);
    }

    #[test]
    fn line_count_is_preserved() {
        let src = "const float pi = 3.1415927;\nfloat f(){\n  return sin(2 * pi);\n}\n";
        let out = widen(src);
        assert_eq!(out.lines().count(), src.lines().count());
    }

    #[test]
    fn function_parameter_types_are_collected() {
        let out = widen("float g(float x, int n){ return x * 2; }\n");
        assert!(out.contains("x * 2.0"), "{out}");
    }

    #[test]
    fn user_defined_float_function_args_are_not_widened() {
        let src = "float linear_fog_value(float a, float b){ return a + b; }\nfloat f(){ return linear_fog_value(0.0, 1); }\n";
        let out = widen(src);
        assert!(out.contains("linear_fog_value(0.0, 1)"), "{out}");
    }

    #[test]
    fn comma_declared_names_are_collected() {
        let out = widen("uniform float density, start;\nfloat f(){ return density * 2; }\n");
        assert!(out.contains("density * 2.0"), "{out}");
    }
}
