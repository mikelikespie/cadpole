//! `format(value, spec)`: Python's format-spec mini-language for the numbers
//! and strings a drawing writes.
//!
//! Starlark's `%` operator takes `%s %d %f %e %g` and nothing between the
//! `%` and the letter, so `"%.1f" % area` is an error and a table of areas
//! came out as `26.24671916010498`. `math.round(x, 1)` rounds the number
//! but not what `str()` writes (`12.0` stays `12.0`, `12.30` is `12.3`).
//! `format(x, ".1f")` is the missing piece, spelled the way Python spells
//! it, so the reader already knows it: `[[fill]align][sign][z][#][0][width]
//! [,|_][.precision][type]`, with the types `f F e E g G %` for numbers,
//! `d b o x X` for integers and `s` for strings. Rounding is Rust's, which
//! like Python's rounds the exact binary value to nearest, ties to even:
//! `format(2.675, ".2f")` is `"2.67"` in both, because 2.675 is stored as
//! 2.67499999... Pure: no feature, nothing recorded.

use starlark::environment::GlobalsBuilder;
use starlark::starlark_module;
use starlark::values::{UnpackValue, Value};

/// A parsed format spec.
#[derive(Clone, Debug, PartialEq)]
struct Spec {
    fill: char,
    align: Option<char>,
    sign: char,
    z: bool,
    alt: bool,
    width: usize,
    grouping: Option<char>,
    precision: Option<usize>,
    ty: Option<char>,
}

fn parse(spec: &str) -> Result<Spec, String> {
    let c: Vec<char> = spec.chars().collect();
    let n = c.len();
    let mut i = 0;
    let mut sp = Spec { fill: ' ', align: None, sign: '-', z: false, alt: false, width: 0, grouping: None, precision: None, ty: None };
    let is_align = |ch: char| matches!(ch, '<' | '>' | '^' | '=');
    let mut fill_given = false;
    if n >= 2 && is_align(c[1]) {
        fill_given = true;
        sp.fill = c[0];
        sp.align = Some(c[1]);
        i = 2;
    } else if n >= 1 && is_align(c[0]) {
        sp.align = Some(c[0]);
        i = 1;
    }
    if i < n && matches!(c[i], '+' | '-' | ' ') {
        sp.sign = c[i];
        i += 1;
    }
    if i < n && c[i] == 'z' {
        sp.z = true;
        i += 1;
    }
    if i < n && c[i] == '#' {
        sp.alt = true;
        i += 1;
    }
    // Python's `0` flag: zero fill unless a fill was given, and sign-aware
    // (`=`) unless an alignment was (`format(1234, "<08,d")` is `1,234000`).
    if i < n && c[i] == '0' {
        if !fill_given {
            sp.fill = '0';
        }
        if sp.align.is_none() {
            sp.align = Some('=');
        }
        i += 1;
    }
    let start = i;
    while i < n && c[i].is_ascii_digit() {
        i += 1;
    }
    if i > start {
        sp.width = c[start..i].iter().collect::<String>().parse().map_err(|_| format!("width in {spec:?} is too large"))?;
        if sp.width > 1000 {
            return Err(format!("width {} in {spec:?} is over 1000", sp.width));
        }
    }
    if i < n && matches!(c[i], ',' | '_') {
        sp.grouping = Some(c[i]);
        i += 1;
    }
    if i < n && c[i] == '.' {
        i += 1;
        let start = i;
        while i < n && c[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return Err(format!("{spec:?}: a '.' needs a precision after it"));
        }
        let p: usize = c[start..i].iter().collect::<String>().parse().map_err(|_| format!("precision in {spec:?} is too large"))?;
        if p > 100 {
            return Err(format!("precision {p} in {spec:?} is over 100"));
        }
        sp.precision = Some(p);
    }
    if i < n {
        if !matches!(c[i], 'f' | 'F' | 'e' | 'E' | 'g' | 'G' | '%' | 'd' | 'b' | 'o' | 'x' | 'X' | 's') {
            return Err(format!("{spec:?}: unknown format type {:?} (f F e E g G % d b o x X s)", c[i]));
        }
        sp.ty = Some(c[i]);
        i += 1;
    }
    if i != n {
        return Err(format!("{spec:?} is not a format spec: [[fill]align][sign][z][#][0][width][,][.precision][type]"));
    }
    Ok(sp)
}

/// What a value is, for formatting.
enum Arg {
    /// An integer, as its decimal digits and sign (Starlark ints are
    /// unbounded, so this never goes through an i64).
    Int { negative: bool, digits: String },
    Float(f64),
    Bool(bool),
    Str(String),
}

fn arg_of(v: Value<'_>) -> Result<Arg, String> {
    if let Some(b) = v.unpack_bool() {
        return Ok(Arg::Bool(b));
    }
    if let Some(s) = v.unpack_str() {
        return Ok(Arg::Str(s.to_owned()));
    }
    match v.get_type() {
        "int" => {
            let s = v.to_str();
            let (negative, digits) = match s.strip_prefix('-') {
                Some(d) => (true, d.to_owned()),
                None => (false, s),
            };
            Ok(Arg::Int { negative, digits })
        }
        "float" => starlark::values::float::UnpackFloat::unpack_value(v)
            .ok()
            .flatten()
            .map(|f| Arg::Float(f.0))
            .ok_or_else(|| "format: cannot read the float".to_owned()),
        other => Err(format!("format: a number or a string, got {other}")),
    }
}

/// The digits `int` with `sep` every `every` of them from the right, and
/// zero-extended on the left (separators included) to at least `min_len`
/// characters: Python's zero padding of a grouped number, where the
/// separators carry on into the zeros (`0,001,234`). A separator is never
/// the first character: one that would be is followed by a zero
/// (`format(5, "04,d")` is `0,005`). `min_len` 0 is plain grouping.
fn group(int: &str, sep: char, every: usize, min_len: usize) -> String {
    let digits: Vec<char> = int.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(min_len.max(digits.len() * 2));
    let (mut left, mut run) = (digits.len(), 0);
    while left > 0 || out.len() < min_len {
        if run == every {
            out.push(sep);
            run = 0;
        }
        let d = if left > 0 {
            left -= 1;
            digits[left]
        } else {
            '0'
        };
        out.push(d);
        run += 1;
    }
    out.iter().rev().collect()
}

/// `x` (finite, non-negative) in scientific notation, Python's way:
/// mantissa with `prec` decimals, `e`, a sign and at least two digits.
fn sci(x: f64, prec: usize, upper: bool) -> String {
    let s = format!("{x:.prec$e}");
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let e: i32 = e.parse().unwrap_or(0);
    let mark = if upper { 'E' } else { 'e' };
    format!("{m}{mark}{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

/// The decimal exponent of `x` once rounded to `p` significant digits.
fn exponent(x: f64, p: usize) -> i32 {
    if x == 0.0 {
        return 0;
    }
    let s = format!("{x:.prec$e}", prec = p.saturating_sub(1));
    s.split_once('e').and_then(|(_, e)| e.parse().ok()).unwrap_or(0)
}

/// Strip trailing zeros after a decimal point, and the point if nothing is
/// left after it, in the mantissa only.
fn strip_zeros(s: &str) -> String {
    let (m, e) = match s.find(['e', 'E']) {
        Some(i) => s.split_at(i),
        None => (s, ""),
    };
    let m = if m.contains('.') { m.trim_end_matches('0').trim_end_matches('.') } else { m };
    format!("{m}{e}")
}

/// Python's `g`: fixed or scientific by the exponent, trailing zeros
/// dropped unless `alt`. `keep_point` is the no-type float form, which
/// keeps one digit past the point when it is fixed.
fn general(x: f64, prec: Option<usize>, upper: bool, alt: bool, keep_point: bool) -> String {
    let p = prec.unwrap_or(6).max(1);
    let e = exponent(x, p);
    let s = if -4 <= e && (e as i64) < p as i64 {
        let decimals = (p as i64 - 1 - e as i64).max(0) as usize;
        let s = format!("{x:.decimals$}");
        let mut s = if alt { s } else { strip_zeros(&s) };
        if keep_point && !s.contains('.') {
            s.push_str(".0");
        }
        s
    } else {
        let s = sci(x, p - 1, upper);
        if alt { s } else { strip_zeros(&s) }
    };
    s
}

/// Python's `repr` of a finite, non-negative float.
fn shortest(a: f64) -> String {
    let e = format!("{a:e}");
    let (m, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if (-4..16).contains(&exp) {
        let s = format!("{a}");
        if s.contains('.') { s } else { format!("{s}.0") }
    } else {
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

/// Apply `spec` to `value`: the whole of what `format()` does.
pub(crate) fn format_value(value: Value<'_>, spec: &str) -> Result<String, String> {
    let sp = parse(spec)?;
    let arg = arg_of(value)?;
    let is_number = !matches!(arg, Arg::Str(_));
    // Sign and body, before padding.
    let (negative, body): (bool, String) = match (&arg, sp.ty) {
        (Arg::Str(s), None | Some('s')) => {
            let s = match sp.precision {
                Some(p) => s.chars().take(p).collect(),
                None => s.clone(),
            };
            (false, s)
        }
        (Arg::Str(_), Some(t)) => return Err(format!("format: {t:?} is a number type and the value is a string")),
        (Arg::Bool(b), None) => (false, if *b { "True".to_owned() } else { "False".to_owned() }),
        (Arg::Bool(_) | Arg::Int { .. } | Arg::Float(_), Some('s')) => {
            return Err("format: 's' is for strings; use str() or a number type".to_owned())
        }
        (Arg::Bool(b), Some(t)) => {
            let digits = if *b { "1" } else { "0" }.to_owned();
            return format_value_int(false, digits, t, &sp);
        }
        (Arg::Int { negative, digits }, None | Some('d' | 'b' | 'o' | 'x' | 'X')) => {
            return format_value_int(*negative, digits.clone(), sp.ty.unwrap_or('d'), &sp);
        }
        (Arg::Int { negative, digits }, Some(t)) => {
            let x: f64 = digits.parse::<f64>().unwrap_or(f64::INFINITY) * if *negative { -1.0 } else { 1.0 };
            float_body(x, Some(t), &sp)
        }
        (Arg::Float(_), Some(t @ ('d' | 'b' | 'o' | 'x' | 'X'))) => {
            return Err(format!("format: {t:?} is for integers; use \".0f\" for a float with no decimals"))
        }
        (Arg::Float(x), t) => float_body(*x, t, &sp),
    };
    // A finite float's integer part is the run of digits it starts with;
    // what follows (`.5`, `e+20`, `%`) is never grouped.
    let int_len = if is_number && body.starts_with(|c: char| c.is_ascii_digit()) {
        body.find(|c: char| !c.is_ascii_digit()).unwrap_or(body.len())
    } else {
        0
    };
    let (int, rest) = body.split_at(int_len);
    Ok(lay_out(negative, "", int, rest, 3, is_number, &sp))
}

/// The sign and the digits of a float.
fn float_body(x: f64, ty: Option<char>, sp: &Spec) -> (bool, String) {
    let negative = x.is_sign_negative() && !x.is_nan();
    let a = x.abs();
    let upper = matches!(ty, Some('F' | 'E' | 'G'));
    let body = if !a.is_finite() {
        let s = if a.is_nan() { "nan" } else { "inf" };
        if upper { s.to_ascii_uppercase() } else { s.to_owned() }
    } else {
        match ty {
            Some('f' | 'F') => format!("{a:.prec$}", prec = sp.precision.unwrap_or(6)),
            Some('e' | 'E') => sci(a, sp.precision.unwrap_or(6), upper),
            Some('g' | 'G') => general(a, sp.precision, upper, sp.alt, false),
            Some('%') => format!("{:.prec$}%", a * 100.0, prec = sp.precision.unwrap_or(6)),
            _ => match sp.precision {
                Some(p) => general(a, Some(p), false, sp.alt, true),
                // No type, no precision: Python's `repr`, the shortest
                // string that reads back as the same float, in positional
                // form unless the exponent is under -4 or 16 or over.
                // (Starlark's own `str()` goes scientific from a million.)
                None => shortest(a),
            },
        }
    };
    // `z`: a value that rounds to zero is written without its sign.
    let zero = body.chars().all(|c| matches!(c, '0' | '.' | '%' | ',' | '_'));
    (negative && !(sp.z && zero), body)
}

fn format_value_int(negative: bool, digits: String, ty: char, sp: &Spec) -> Result<String, String> {
    if sp.precision.is_some() {
        return Err("format: an integer takes no precision; use \".2f\" to write it with decimals".to_owned());
    }
    let (prefix, body, every) = match ty {
        'd' => ("", digits, 3),
        'b' | 'o' | 'x' | 'X' => {
            let v: u128 = digits.parse().map_err(|_| format!("format: {digits} is too large for {ty:?}"))?;
            let (s, prefix) = match ty {
                'b' => (format!("{v:b}"), "0b"),
                'o' => (format!("{v:o}"), "0o"),
                'x' => (format!("{v:x}"), "0x"),
                _ => (format!("{v:X}"), "0X"),
            };
            if sp.grouping == Some(',') {
                return Err("format: ',' groups decimal digits; use '_' for binary, octal and hex".to_owned());
            }
            (if sp.alt { prefix } else { "" }, s, 4)
        }
        other => return Err(format!("format: {other:?} is not an integer type")),
    };
    // Every character of an integer's body is a digit, hex `e` included.
    Ok(lay_out(negative, prefix, &body, "", every, true, sp))
}

/// A number's sign, `#` prefix, integer digits (grouped every `every` when
/// the spec groups) and the rest, filled and aligned. Under `=` with a zero
/// fill the integer part itself is extended, so grouping runs through the
/// padding as in Python; under `=` with any other fill, the fill goes
/// between the sign and prefix and the digits.
fn lay_out(negative: bool, prefix: &str, int: &str, rest: &str, every: usize, is_number: bool, sp: &Spec) -> String {
    let Some(g) = sp.grouping.filter(|_| is_number && !int.is_empty()) else {
        return pad(negative, prefix, format!("{int}{rest}"), is_number, sp);
    };
    let min_len = if sp.align == Some('=') && sp.fill == '0' {
        let sign = usize::from(negative || matches!(sp.sign, '+' | ' '));
        sp.width.saturating_sub(sign + prefix.chars().count() + rest.chars().count())
    } else {
        0
    };
    pad(negative, prefix, format!("{}{rest}", group(int, g, every, min_len)), is_number, sp)
}

/// Sign, fill and alignment. `prefix` (`0x` under `#`) stays with the
/// digits: `=` fills between it and the sign's side, so `#08x` is
/// `0x0000ff`.
fn pad(negative: bool, prefix: &str, body: String, is_number: bool, sp: &Spec) -> String {
    let sign = if negative {
        "-"
    } else if is_number {
        match sp.sign {
            '+' => "+",
            ' ' => " ",
            _ => "",
        }
    } else {
        ""
    };
    let len = sign.chars().count() + prefix.chars().count() + body.chars().count();
    let fill_n = sp.width.saturating_sub(len);
    let fill: String = std::iter::repeat_n(sp.fill, fill_n).collect();
    match sp.align.unwrap_or(if is_number { '>' } else { '<' }) {
        '<' => format!("{sign}{prefix}{body}{fill}"),
        '^' => {
            let left: String = std::iter::repeat_n(sp.fill, fill_n / 2).collect();
            let right: String = std::iter::repeat_n(sp.fill, fill_n - fill_n / 2).collect();
            format!("{left}{sign}{prefix}{body}{right}")
        }
        '=' => format!("{sign}{prefix}{fill}{body}"),
        _ => format!("{fill}{sign}{prefix}{body}"),
    }
}

#[starlark_module]
pub(crate) fn format_globals(builder: &mut GlobalsBuilder) {
    /// `value` written by the format spec `spec`, Python's mini-language:
    /// `[[fill]align][sign][z][#][0][width][,|_][.precision][type]`.
    /// `format(3.14159, ".2f")` is `"3.14"`, `format(1234567.8,
    /// ",.0f")` is `"1,234,568"`, `format(0.256, ".1%")` is `"25.6%"`,
    /// `format(7, "03d")` is `"007"`, `format("A2.0", ">8")` right-aligns
    /// in eight characters. Types: `f` fixed, `e` scientific, `g` general,
    /// `%` percent (upper-case `F E G` too) for numbers; `d b o x X` for
    /// integers; `s` for strings. No spec writes a string or an integer as
    /// `str()` does and a float as Python's `str()` does (`1234567.891`,
    /// where Starlark's goes to `1.234568e+06`).
    /// Starlark's own `"%.1f" % x` takes no precision, which is why this
    /// exists: a cover sheet's table of areas.
    fn format<'v>(
        #[starlark(require = pos)] value: Value<'v>,
        #[starlark(require = pos, default = "")] spec: &str,
    ) -> anyhow::Result<String> {
        format_value(value, spec).map_err(|e| anyhow::anyhow!("{}", if e.starts_with("format") { e } else { format!("format: {e}") }))
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{errors, run as eval};

    fn run(src: &str) -> Vec<String> {
        errors(&eval(src))
    }

    fn check(expr: &str, want: &str) {
        let src = format!("assert_eq({expr}, {want:?})\n");
        let errs = run(&src);
        assert!(errs.is_empty(), "{expr} -> {errs:?}");
    }

    #[test]
    fn numbers_format_the_way_python_writes_them() {
        check("format(3.14159, \".2f\")", "3.14");
        check("format(2.675, \".2f\")", "2.67");
        check("format(1234567.891, \",.1f\")", "1,234,567.9");
        check("format(1234567.8, \",.0f\")", "1,234,568");
        check("format(0.256, \".1%\")", "25.6%");
        check("format(42, \"05d\")", "00042");
        check("format(42, \">6\")", "    42");
        check("format(-42, \"=+6\")", "-   42");
        check("format(1234567, \",\")", "1,234,567");
        check("format(1234.5, \"e\")", "1.234500e+03");
        check("format(0.00001234, \"g\")", "1.234e-05");
        check("format(123456789.0, \"g\")", "1.23457e+08");
        check("format(2.0, \"g\")", "2");
        check("format(2.0, \".3\")", "2.0");
        check("format(2.5)", "2.5");
        check("format(2.0)", "2.0");
        check("format(1.5, \"+.1f\")", "+1.5");
        check("format(-1.5, \"+.1f\")", "-1.5");
        check("format(-0.04, \".1f\")", "-0.0");
        check("format(-0.04, \"z.1f\")", "0.0");
        check("format(12, \".2f\")", "12.00");
        check("format(255, \"#x\")", "0xff");
        check("format(True, \"d\")", "1");
        check("format(True)", "True");
        check("format(7)", "7");
        check("format(1234567.891)", "1234567.891");
        check("format(1e20)", "1e+20");
        check("format(0.0001)", "0.0001");
        check("format(0.00001)", "1e-05");
        check("format(1234567.891, \",\")", "1,234,567.891");
    }

    /// Zero padding with grouping carries the separators into the padding,
    /// as Python does (`format(1234, "08,d")` is `0,001,234`, not
    /// `0001,234`); `_` groups every hex digit, `e` included; the `0` flag
    /// zero-fills under an explicit alignment; and `#` puts its prefix in
    /// front of the zeros. Every expectation is CPython's output.
    #[test]
    fn grouping_and_zero_padding_match_python() {
        check("format(1234, \"08,d\")", "0,001,234");
        check("format(1234.5, \"010,.1f\")", "0,001,234.5");
        check("format(1234, \"08_d\")", "0_001_234");
        check("format(1234, \"0=8,d\")", "0,001,234");
        check("format(1234, \"=08,d\")", "0,001,234");
        check("format(1234, \"09,d\")", "0,001,234");
        check("format(1234, \"06,d\")", "01,234");
        check("format(1234, \"05,d\")", "1,234");
        check("format(5, \"04,d\")", "0,005");
        check("format(5, \"03,d\")", "005");
        check("format(-5, \"05,d\")", "-0,005");
        check("format(-1234, \"08,d\")", "-001,234");
        check("format(1234, \"+09,d\")", "+0,001,234");
        check("format(-1234.5, \"+012,.1f\")", "-0,001,234.5");
        check("format(0.5, \"08,.2%\")", "0,050.00%");
        check("format(1e20, \"030,\")", "00,000,000,000,000,000,001e+20");
        check("format(255, \"010_b\")", "0_1111_1111");
        check("format(123456, \"_x\")", "1_e240");
        check("format(4887303800, \"_x\")", "1_234e_5678");
        check("format(123456, \"#012_x\")", "0x0_0001_e240");
        check("format(255, \"#09_x\")", "0x00_00ff");
        check("format(255, \"#08x\")", "0x0000ff");
        check("format(-255, \"#08x\")", "-0x000ff");
        check("format(255, \"#8x\")", "    0xff");
        // Only `=` alignment pads through the grouping; other fills do not.
        check("format(1234, \"0>8,d\")", "0001,234");
        check("format(1234, \"0<8,d\")", "1,234000");
        check("format(1234, \"<08,d\")", "1,234000");
        check("format(1234, \"*>8,d\")", "***1,234");
        check("format(1234, \"*=8,d\")", "***1,234");
        check("format(1234.5, \"012,e\")", "1.234500e+03");
        check("format(float(\"inf\"), \"010,f\")", "0000000inf");
    }

    #[test]
    fn strings_align_and_truncate() {
        check("format(\"ab\", \"^6\")", "  ab  ");
        check("format(\"ab\", \"*<5\")", "ab***");
        check("format(\"abcdef\", \".3\")", "abc");
        check("format(\"A2.0\", \">6\")", "  A2.0");
    }

    #[test]
    fn a_wrong_spec_says_what_a_spec_is() {
        let e = run("format(1.5, \"d\")\n");
        assert!(e.iter().any(|m| m.contains("\".0f\"")), "{e:?}");
        let e = run("format(\"x\", \".1f\")\n");
        assert!(e.iter().any(|m| m.contains("number type")), "{e:?}");
        let e = run("format(1.5, \".1q\")\n");
        assert!(e.iter().any(|m| m.contains("unknown format type")), "{e:?}");
        let e = run("format(1.5, \"..\")\n");
        assert!(!e.is_empty());
    }
}
